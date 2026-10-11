//! `truth` pass — flags an `assert truth(...)` table that verifies less
//! than it looks like it does.
//!
//! The parser refuses a row it cannot read against the header (a digit
//! other than `0`/`1`, or the wrong width). This pass reads the header
//! against itself and the table as a whole, which read in a diff exactly
//! like a table that passes: a signal listed twice, no rows, two rows
//! assigning one combination, and combinations left unassigned.
//!
//! Severity follows what is provable ([`crate::diagnostic`] records it per
//! code). A repeated input is an error, since the rows then describe
//! combinations the circuit does not have, and it is reported alone because
//! every other finding reads the rows against that list. A table with no
//! rows asserts nothing and is an error, as is a pair of rows giving one
//! combination two outputs. A table short of rows still asserts what its
//! rows say, and a four-input table is sixteen rows an author part way
//! through should not be blocked on, so that is a warning, as is a row that
//! repeats an earlier one and agrees with it.
//!
//! Nothing evaluates a truth table yet, so no message says which of two
//! disagreeing rows an evaluator would read.
//!
//! The walk covers the `asserts` of every body and of every member body
//! under them: redstone synthesis reads a nested `assert` too, so a table
//! under a `level` would otherwise be the one place a shape went unreported.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use crate::ast::{DottedRef, TruthRow};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;
use crate::intent::{AssertIr, IntentModule, Member, MemberBody};
use crate::prose::and_list;

use super::DiagnosticSink;

/// How many unassigned combinations a finding names, in the sentence and
/// in the payload alike.
///
/// A sample, so a twenty-input table does not build a million strings to
/// describe a one-row mistake.
const MISSING_SAMPLE: usize = 4;

/// How many of a repeated signal's positions an
/// `E_TRUTH_TABLE_DUPLICATE_INPUT` names before it counts the rest, so a
/// signal listed thousands of times does not put kilobytes into a sentence.
const POSITION_SAMPLE: usize = 4;

/// Past this many inputs the number of combinations is written `2^n`: a
/// longer decimal is not one a reader compares a row count against. Past
/// 127 inputs there is no integer to render, which takes the same branch.
const DECIMAL_TOTAL_MAX_INPUTS: u32 = 32;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for body in ir.bodies() {
        walk(body.members, body.asserts, sink);
    }
}

fn walk(members: &[Member], asserts: &[AssertIr], sink: &mut DiagnosticSink) {
    for assertion in asserts {
        check_table(assertion, sink);
    }
    for member in members {
        let MemberBody {
            members: children,
            asserts: nested,
            ..
        } = &member.children;
        walk(children, nested, sink);
    }
}

fn check_table(assertion: &AssertIr, sink: &mut DiagnosticSink) {
    let AssertIr::Truth {
        inputs, rows, span, ..
    } = assertion
    else {
        return;
    };
    let repeated = repeated_inputs(inputs, span);
    if !repeated.is_empty() {
        for finding in repeated {
            sink.push(finding);
        }
        return;
    }
    let arity = u32::try_from(inputs.len()).expect("an input list is bounded by the source length");
    // The parser refuses any other width, and the rest of this file relies
    // on it: comparisons zip two patterns, and the sample search indexes
    // every position of its prefix.
    debug_assert!(
        rows.iter().all(|row| row.inputs.len() == inputs.len()),
        "a truth-table row is as wide as its input list",
    );
    if rows.is_empty() {
        // Alone: a coverage finding beside it would bill one repair twice.
        sink.push(empty_table(span, arity));
        return;
    }

    // A `-` stands for the rows it expands to, so two rows sharing a
    // combination write it twice, and nothing orders rows to let the later
    // one win.
    //
    // Each row is compared with every earlier row, dropped or not: a `-`
    // row can overlap several that disagree with each other (`0- -> 1`
    // after `00 -> 1; 01 -> 0`), and whether the table is refused must not
    // depend on row order. A conflict is noted at the first earlier row that
    // contradicts this one, any other finding at the first it overlaps; no
    // row before that one shares the combination, so every finding naming
    // it sends the author to the same row.
    //
    // A row overlapping an accepted one is not accepted, so the accepted
    // patterns are pairwise disjoint and coverage is the sum of their sizes.
    let mut accepted: Vec<&TruthRow> = Vec::with_capacity(rows.len());
    // Whether dropped rows are still accounted for. A row inside the one it
    // answers to (`01` under `0-`) assigns nothing new; one that only
    // crosses it does, and counting without it would report as missing a
    // combination the source assigns, so then no coverage finding is made.
    //
    // A row can be accepted and still carry a finding when every row it
    // overlaps was dropped. The count stays sound: a dropped row D overlapped
    // an accepted A, and were D inside A, a row overlapping D would overlap
    // A and not be accepted, so D crossed A and already cleared this flag.
    let mut coverage_is_countable = true;
    // Whether each row carries an overlap finding of its own, which
    // changes the advice a later row noted against it can be given.
    let mut reported = vec![false; rows.len()];
    for (index, row) in rows.iter().enumerate() {
        let mut first_overlap = None;
        let mut contradicted = None;
        for (at, other) in rows[..index].iter().enumerate() {
            if !overlap(&other.inputs, &row.inputs) {
                continue;
            }
            first_overlap.get_or_insert(at);
            if contradicts(other, row) {
                contradicted = Some(at);
                break;
            }
        }
        if let Some(at) = contradicted.or(first_overlap) {
            sink.push(overlapping_row(row, &rows[at], reported[at]));
            reported[index] = true;
        }
        match accepted
            .iter()
            .find(|kept| overlap(&kept.inputs, &row.inputs))
        {
            None => accepted.push(row),
            Some(first) => coverage_is_countable &= subsumes(&first.inputs, &row.inputs),
        }
    }

    // An all-`-` table is the empty table written at length, so it earns
    // that finding and not the coverage one. Asked of `rows`, not
    // `accepted`: acceptance is first-come, so a broad `-` row written
    // first would leave the survivors looking empty while the author sees
    // a `0` in the table.
    if rows.iter().all(|row| row.output.is_none()) {
        sink.push(asserts_nothing(span, arity));
    } else if coverage_is_countable
        && let Some(finding) = unassigned_combinations(span, arity, &accepted)
    {
        sink.push(finding);
    }
}

/// One `E_TRUTH_TABLE_DUPLICATE_INPUT` per signal the input list names
/// more than once, in the order the signals first appear.
///
/// Anchored on the whole `assert`, since the input list has no span of its
/// own, so the sentence names the positions. Linear in the list's length.
fn repeated_inputs(inputs: &[DottedRef], span: &Span) -> Vec<Diagnostic> {
    let mut positions: Vec<(&DottedRef, Vec<usize>)> = Vec::new();
    let mut slots: HashMap<&DottedRef, usize> = HashMap::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        match slots.entry(input) {
            Entry::Occupied(slot) => positions[*slot.get()].1.push(index + 1),
            Entry::Vacant(slot) => {
                slot.insert(positions.len());
                positions.push((input, vec![index + 1]));
            }
        }
    }
    positions
        .into_iter()
        .filter(|(_, at)| at.len() > 1)
        .map(|(name, at)| {
            let shown: Vec<String> = at
                .iter()
                .take(POSITION_SAMPLE)
                .map(ToString::to_string)
                .collect();
            let listed = match at.len().checked_sub(POSITION_SAMPLE) {
                Some(more) if more > 0 => format!("{}, and {more} more", shown.join(", ")),
                _ => and_list(&shown).expect("a repeated input holds two positions or more"),
            };
            Diagnostic {
                code: DiagnosticCode::TruthTableDuplicateInput,
                span: span.clone(),
                primary: format!(
                    "this `assert truth` lists `{name}` as inputs {listed}, and one signal is \
                     one input, so the table has {columns} columns for it and its rows \
                     describe combinations the circuit does not have",
                    columns = at.len(),
                ),
                notes: vec![DiagnosticNote {
                    span: None,
                    message: format!(
                        "Fix: list `{name}` once and drop its other columns from every row, or \
                         name the signal you meant in its place"
                    ),
                }],
                data: None,
            }
        })
        .collect()
}

/// Whether two patterns assign a combination in common: `0-` and `-1`
/// overlap at `01`, while `00` and `01` overlap nowhere.
fn overlap(a: &str, b: &str) -> bool {
    a.bytes()
        .zip(b.bytes())
        .all(|(x, y)| x == b'-' || y == b'-' || x == y)
}

/// Whether two rows' outputs contradict each other, read only once
/// [`overlap`] says the rows share a combination.
///
/// Only two concrete outputs can: a `-` output asserts nothing for the
/// other row to contradict.
fn contradicts(a: &TruthRow, b: &TruthRow) -> bool {
    matches!((a.output, b.output), (Some(x), Some(y)) if x != y)
}

/// Whether every combination `inner` assigns, `outer` assigns too.
///
/// Read only after [`overlap`] says yes, to pick which sentence the
/// finding gets: a row inside an earlier one is deleted, a row merely
/// crossing it is narrowed.
fn subsumes(outer: &str, inner: &str) -> bool {
    outer
        .bytes()
        .zip(inner.bytes())
        .all(|(o, i)| o == b'-' || o == i)
}

/// One combination both patterns assign, for a message to name.
///
/// The lowest combination the two share. The caller has checked they
/// overlap, so no position disagrees.
fn shared_combination(a: &str, b: &str) -> String {
    a.bytes()
        .zip(b.bytes())
        .map(|(x, y)| {
            let bit = if x == b'-' {
                if y == b'-' { b'0' } else { y }
            } else {
                x
            };
            char::from(bit)
        })
        .collect()
}

fn empty_table(span: &Span, arity: u32) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::TruthTableEmpty,
        span: span.clone(),
        primary: "this `assert truth` has no rows, so it verifies nothing".to_owned(),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "Fix: give the table a row for each of the {total} combinations its {arity} \
                 input{plural} can take, or delete the assertion",
                total = total_combinations(arity),
                plural = if arity == 1 { "" } else { "s" },
            ),
        }],
        data: None,
    }
}

/// What to do about a row that constrains a combination an overlapping row
/// leaves unconstrained, or the other way round.
///
/// Not a conflict, so the code is the duplicate-row one, but "delete either
/// row" would not do: which one goes changes the table.
const UNCONSTRAINED_FIX: &str = "Fix: delete whichever of the two you did not mean — a `-` output says the combination is \
     deliberately unconstrained, and the other row constrains it, so the table reads differently \
     depending on which one stays";

/// A table with rows, none of which asserts an output.
///
/// [`empty_table`]'s code, since a table of `-` outputs constrains nothing;
/// the sentence differs because the repair is to change rows, not add them.
fn asserts_nothing(span: &Span, arity: u32) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::TruthTableEmpty,
        span: span.clone(),
        primary: "every row of this `assert truth` has a `-` output, so it verifies nothing"
            .to_owned(),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "Fix: give at least one row a `0` or `1` output, or delete the assertion — a \
                 `-` output says a combination is deliberately unconstrained, which is only \
                 worth writing beside combinations that are constrained (this table has \
                 {total} to choose from)",
                total = total_combinations(arity),
            ),
        }],
        data: None,
    }
}

/// A row assigning a combination an earlier row already assigned.
///
/// Two different concrete outputs are `E_TRUTH_TABLE_CONFLICT`; anything
/// else is the duplicate-row code, with one of four repairs. A row
/// repeating an earlier pattern, or lying inside one (`01` under `0-`), is
/// deleted; a row crossing one (`-1` against `0-`) is narrowed, since
/// deleting it would lose coverage; and a `-` output beside a concrete one
/// ([`UNCONSTRAINED_FIX`]) needs the author to choose.
///
/// For a conflict, `noted` is the first earlier row contradicting this one,
/// so the note names its output. The row that first assigned the
/// combination may agree with this one, so that note cannot say "first row
/// assigning" as the others do.
///
/// When `noted_is_reported`, the earlier row is being asked to change too,
/// and "delete this row" can cancel against that: `10 -> 1` lies inside the
/// `-0` of `0- -> 1; -0 -> 1` while `-0` is told to narrow away from `0-`,
/// and following both leaves `10` unassigned. So the repair is to settle
/// the earlier row first.
fn overlapping_row(row: &TruthRow, noted: &TruthRow, noted_is_reported: bool) -> Diagnostic {
    let pattern = &row.inputs;
    let earlier = &noted.inputs;
    let shared = shared_combination(earlier, pattern);
    // Everything below the match reads one output for both rows, which is
    // only sound once the cases where they say different things are gone.
    let disagreement = match (row.output, noted.output) {
        (Some(later), Some(earlier_output)) if later != earlier_output => Some((
            DiagnosticCode::TruthTableConflict,
            format!(
                "first row assigning `{shared}` the output `{other}` here",
                other = bit(earlier_output),
            ),
            format!(
                "this row assigns `{shared}` the output `{output}`, and an earlier row assigns \
                 it `{other}`",
                output = bit(later),
                other = bit(earlier_output),
            ),
            "Fix: decide which of the two the circuit should do and change or delete the other \
             row — no circuit produces both outputs for one input combination"
                .to_owned(),
        )),
        (None, Some(assigned)) => Some((
            DiagnosticCode::TruthTableDuplicateRow,
            first_assigning(&shared),
            format!(
                "this row leaves `{shared}` unconstrained, and the earlier row `{earlier}` \
                 assigns it the output `{output}`",
                output = bit(assigned),
            ),
            settled_first(UNCONSTRAINED_FIX.to_owned(), earlier, noted_is_reported),
        )),
        (Some(assigned), None) => Some((
            DiagnosticCode::TruthTableDuplicateRow,
            first_assigning(&shared),
            format!(
                "this row assigns `{shared}` the output `{output}`, and the earlier row \
                 `{earlier}` leaves it unconstrained",
                output = bit(assigned),
            ),
            settled_first(UNCONSTRAINED_FIX.to_owned(), earlier, noted_is_reported),
        )),
        _ => None,
    };
    if let Some((code, note, primary, fix)) = disagreement {
        return finding(code, row, noted, note, primary, fix);
    }
    let (primary, fix) = if pattern == earlier {
        (
            format!(
                "this row repeats an earlier one: `{pattern}` is already assigned {outcome}",
                outcome = outcome(noted.output),
            ),
            "Fix: delete either row — the table asserts the same thing without it".to_owned(),
        )
    } else if subsumes(earlier, pattern) {
        (
            format!(
                "this row asserts nothing new: the earlier row `{earlier}` already assigns \
                 `{pattern}` {outcome}",
                outcome = outcome(noted.output),
            ),
            "Fix: delete this row — the earlier pattern's `-` already stands for it".to_owned(),
        )
    } else {
        (
            format!(
                "this row and the earlier row `{earlier}` both stand for `{shared}`, and a \
                 combination is covered by one row or none"
            ),
            format!(
                "Fix: narrow one of the two so no combination is written twice — `{pattern}` \
                 and `{earlier}` may not both stand for `{shared}`"
            ),
        )
    };
    let fix = settled_first(fix, earlier, noted_is_reported);
    finding(
        DiagnosticCode::TruthTableDuplicateRow,
        row,
        noted,
        first_assigning(&shared),
        primary,
        fix,
    )
}

/// `fix`, unless the noted row is itself being asked to change — see
/// [`overlapping_row`] for why the repair then waits on that row.
fn settled_first(fix: String, earlier: &str, noted_is_reported: bool) -> String {
    if !noted_is_reported {
        return fix;
    }
    format!(
        "Fix: settle the earlier row `{earlier}` first — it overlaps a row before it and is \
         reported for that, so whether this one stays, and in what shape, depends on how that \
         one changes"
    )
}

fn first_assigning(shared: &str) -> String {
    format!("first row assigning `{shared}` here")
}

/// The shape every overlap finding shares: reported on the later row,
/// with the earlier one noted and the fix last.
fn finding(
    code: DiagnosticCode,
    row: &TruthRow,
    noted: &TruthRow,
    note: String,
    primary: String,
    fix: String,
) -> Diagnostic {
    Diagnostic {
        code,
        span: row.span.clone(),
        primary,
        notes: vec![
            DiagnosticNote {
                span: Some(noted.span.clone()),
                message: note,
            },
            DiagnosticNote {
                span: None,
                message: fix,
            },
        ],
        data: None,
    }
}

/// The combinations no accepted row assigns, or `None` when there is
/// nothing to report.
///
/// `None` also when the covered total is past `u128` (128 inputs or more)
/// or past the payload's `u64` (65 inputs or more): a finding whose numbers
/// are guesses is worse than none. The `sample.is_empty()` return is
/// unreachable, since [`missing_sample`] enters only a prefix with a
/// missing combination under it; it stands so the `expect` below has a
/// check to point to, and the `debug_assert!` makes a breach loud.
///
/// `accepted` is pairwise non-overlapping, so the covered total is the sum
/// of the rows' sizes.
fn unassigned_combinations(span: &Span, arity: u32, accepted: &[&TruthRow]) -> Option<Diagnostic> {
    let total = combination_count(arity);
    let covered = assigned_count(accepted)?;
    if total == Some(covered) {
        return None;
    }
    let covered_count = u64::try_from(covered).ok()?;
    let sample = missing_sample(arity, accepted);
    debug_assert!(
        !sample.is_empty(),
        "an incomplete table has a missing combination for the descent to find",
    );
    if sample.is_empty() {
        return None;
    }
    let quoted: Vec<String> = sample.iter().map(|p| format!("`{p}`")).collect();
    let missing_total = total.map(|total| total - covered);
    // A capped sample must not read as the whole set, so the sentence
    // counts the rest; past 127 inputs there is no integer to count with.
    let listed = u128::try_from(sample.len()).expect("the sample is capped at a small constant");
    let rows_to_write = match missing_total {
        Some(total) if total == listed => {
            and_list(&quoted).expect("the sample was checked non-empty above")
        }
        Some(total) => format!("{}, and {} more", quoted.join(", "), total - listed),
        None => format!("{}, and more beyond those", quoted.join(", ")),
    };
    Some(Diagnostic {
        code: DiagnosticCode::TruthTablePartial,
        span: span.clone(),
        primary: format!(
            "this `assert truth` assigns {covered_count} of the {total} combinations its {arity} \
             input{plural} can take",
            total = total_combinations(arity),
            plural = if arity == 1 { "" } else { "s" },
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!("Fix: add a row for {rows_to_write}"),
        }],
        data: Some(DiagnosticData::TruthTablePartial {
            inputs: arity,
            covered: covered_count,
            missing: sample,
        }),
    })
}

/// How many combinations the accepted rows assign between them, or `None`
/// past `u128`.
///
/// A plain sum, because no two accepted rows share a combination.
fn assigned_count(accepted: &[&TruthRow]) -> Option<u128> {
    accepted.iter().try_fold(0u128, |sum, row| {
        sum.checked_add(pattern_size(&row.inputs)?)
    })
}

/// `2^d` for a pattern with `d` don't-cares, or `None` when that is past
/// `u128` — 128 or more of them, which needs an input list to match.
fn pattern_size(pattern: &str) -> Option<u128> {
    let dashes = u32::try_from(pattern.bytes().filter(|b| *b == b'-').count())
        .expect("a pattern is bounded by the source length");
    1u128.checked_shl(dashes)
}

/// The lowest few combinations no accepted row assigns, in ascending
/// order: up to [`MISSING_SAMPLE`], fewer only when fewer are missing.
///
/// Found by descending through prefixes, not by counting through
/// combinations. The descent tries `0` before `1` and skips a prefix the
/// rows fill ([`Descent::fills`]), so every prefix it enters has a missing
/// combination under it and adds one to the sample. That bounds the work
/// at `2 × MISSING_SAMPLE × width` tries of a few passes over the rows
/// each, whatever the number of combinations. It loops over a heap stack
/// rather than recursing, because a stack overflow at one frame per input
/// would abort the whole check, a language server's included.
///
/// What the loop relies on:
///
/// * The caller has returned on a complete table, so the empty prefix is
///   entered without asking `fills`.
/// * `accepted` is pairwise non-overlapping, so the sum in `fills` is a
///   count.
/// * Every pattern is `width` long (see `check_table`): [`Descent::extend`]
///   and [`Descent::retract_to`] index every row at each position.
/// * `retract_to` undoes `extend` exactly, so a branch point resumes from
///   its prefix length and digit alone.
/// * [`branch`] stacks `1` under `0`, which makes the sample ascending.
fn missing_sample(arity: u32, accepted: &[&TruthRow]) -> Vec<String> {
    let width = usize::try_from(arity)
        .expect("`arity` was converted from a `usize` length, so it fits one");
    let mut descent = Descent::new(accepted);
    let mut sample = Vec::new();
    // Untried branch points: the prefix length to retract to and the digit
    // to append. About `width` entries at the deepest.
    let mut pending: Vec<(usize, u8)> = Vec::new();
    branch(&mut pending, 0);
    while sample.len() < MISSING_SAMPLE
        && let Some((len, digit)) = pending.pop()
    {
        descent.retract_to(len);
        descent.extend(digit);
        if descent.fills(width) {
            continue;
        }
        if len + 1 == width {
            sample.push(descent.combination());
        } else {
            branch(&mut pending, len + 1);
        }
    }
    sample
}

/// Stack both digits for the position after a prefix `len` long, `0` on
/// top so it is tried first.
fn branch(pending: &mut Vec<(usize, u8)>, len: usize) {
    pending.extend(b"01".iter().rev().map(|&digit| (len, digit)));
}

/// Where the descent stands: a prefix, and what each accepted row says
/// under it, kept per row so that memory does not grow with depth.
struct Descent<'a> {
    rows: &'a [&'a TruthRow],
    /// The digits chosen so far, `0` and `1` only.
    prefix: Vec<u8>,
    /// Per row, the first position where it fixes a digit the prefix does
    /// not have, or `None` while it still assigns some combination
    /// starting with the prefix.
    parted_at: Vec<Option<usize>>,
    /// Per row, its don't-cares past the prefix. A row that has not parted
    /// assigns `2^free` of the combinations under the prefix.
    free: Vec<usize>,
}

impl<'a> Descent<'a> {
    fn new(rows: &'a [&'a TruthRow]) -> Self {
        Self {
            rows,
            prefix: Vec::new(),
            parted_at: vec![None; rows.len()],
            free: rows
                .iter()
                .map(|row| row.inputs.bytes().filter(|b| *b == b'-').count())
                .collect(),
        }
    }

    /// Lengthen the prefix by one digit, reading that position of every
    /// row once.
    fn extend(&mut self, digit: u8) {
        let at = self.prefix.len();
        self.prefix.push(digit);
        for ((row, parted), free) in self
            .rows
            .iter()
            .zip(&mut self.parted_at)
            .zip(&mut self.free)
        {
            match row.inputs.as_bytes()[at] {
                b'-' => *free -= 1,
                fixed => {
                    if parted.is_none() && fixed != digit {
                        *parted = Some(at);
                    }
                }
            }
        }
    }

    /// Shorten the prefix to `len` digits, undoing [`Descent::extend`] a
    /// digit at a time, so every row reads as it did when the prefix was
    /// last that long.
    fn retract_to(&mut self, len: usize) {
        while self.prefix.len() > len {
            self.prefix.pop();
            let at = self.prefix.len();
            for ((row, parted), free) in self
                .rows
                .iter()
                .zip(&mut self.parted_at)
                .zip(&mut self.free)
            {
                if row.inputs.as_bytes()[at] == b'-' {
                    *free += 1;
                }
                if *parted == Some(at) {
                    *parted = None;
                }
            }
        }
    }

    /// Whether the rows that have not parted assign every combination
    /// starting with the prefix.
    ///
    /// Each assigns `2^free` of them and no two share one, so they fill
    /// the `2^(width - len)` combinations there exactly when those powers
    /// add up to it. The sum is done in binary, a carry at a time, so no
    /// width overflows an integer. No `free` reaches 64, since
    /// [`unassigned_combinations`] searches only once the covered total fits
    /// a `u64`, so the cost does not grow with the width.
    fn fills(&self, width: usize) -> bool {
        let len = self.prefix.len();
        let mut bits: BTreeMap<usize, usize> = BTreeMap::new();
        for (free, parted) in self.free.iter().zip(&self.parted_at) {
            if parted.is_none() {
                *bits.entry(*free).or_default() += 1;
            }
        }
        let mut carry = 0;
        let mut position = 0;
        let mut sum = Vec::new();
        while carry > 0 || bits.range(position..).next().is_some() {
            let here = carry + bits.get(&position).copied().unwrap_or(0);
            if here % 2 == 1 {
                sum.push(position);
            }
            carry = here / 2;
            position += 1;
        }
        sum == [width - len]
    }

    /// The prefix, once it is a whole combination.
    fn combination(&self) -> String {
        String::from_utf8(self.prefix.clone()).expect("the descent writes `0` and `1` only")
    }
}

/// `2^arity`, or `None` past `u128`: the grammar puts no ceiling on the
/// input list.
fn combination_count(arity: u32) -> Option<u128> {
    1u128.checked_shl(arity)
}

/// How a sentence writes the number of combinations `arity` inputs have.
fn total_combinations(arity: u32) -> String {
    match combination_count(arity) {
        Some(total) if arity <= DECIMAL_TOTAL_MAX_INPUTS => total.to_string(),
        _ => format!("2^{arity}"),
    }
}

fn bit(output: bool) -> char {
    if output { '1' } else { '0' }
}

/// How a row's output reads inside a sentence about what it assigns.
///
/// A `-` output is not a third value the circuit can produce, so it is
/// named as the absence it is rather than rendered as a bit.
fn outcome(output: Option<bool>) -> String {
    match output {
        Some(value) => format!("the output `{}`", bit(value)),
        None => "no output, by its `-`".to_owned(),
    }
}
