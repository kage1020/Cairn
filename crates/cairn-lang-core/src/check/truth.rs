//! The `truth` pass: an `assert truth(...)` table that verifies less than it looks like it does —
//! a signal listed twice, no rows, two rows assigning one combination, or combinations left
//! unassigned.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use crate::ast::{DottedRef, TruthRow};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;
use crate::intent::{AssertIr, IntentModule, Member, MemberBody};
use crate::prose::and_list;

use super::DiagnosticSink;

/// How many unassigned combinations a finding names, in the sentence and the payload.
const MISSING_SAMPLE: usize = 4;

/// How many of a repeated signal's positions a finding names before it counts the rest.
const POSITION_SAMPLE: usize = 4;

/// Past this many inputs the number of combinations is written `2^n`.
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
    // The parser refuses any other width; the zips and the sample search rely on it.
    debug_assert!(
        rows.iter().all(|row| row.inputs.len() == inputs.len()),
        "a truth-table row is as wide as its input list",
    );
    if rows.is_empty() {
        sink.push(empty_table(span, arity));
        return;
    }

    // Each row meets every earlier row, dropped or not, so the verdict ignores row order.
    // Accepted rows are pairwise disjoint, so coverage is the sum of their sizes.
    let mut accepted: Vec<&TruthRow> = Vec::with_capacity(rows.len());
    // Cleared once a dropped row crosses an accepted one, since the sum then misses what it adds.
    let mut coverage_is_countable = true;
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

    // Asked of `rows`: a broad first row with a `-` output can leave only `-` outputs accepted.
    if rows.iter().all(|row| row.output.is_none()) {
        sink.push(asserts_nothing(span, arity));
    } else if coverage_is_countable
        && let Some(finding) = unassigned_combinations(span, arity, &accepted)
    {
        sink.push(finding);
    }
}

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

/// Whether two patterns share a combination, as `0-` and `-1` share `01`.
fn overlap(a: &str, b: &str) -> bool {
    a.bytes()
        .zip(b.bytes())
        .all(|(x, y)| x == b'-' || y == b'-' || x == y)
}

fn contradicts(a: &TruthRow, b: &TruthRow) -> bool {
    matches!((a.output, b.output), (Some(x), Some(y)) if x != y)
}

/// Whether every combination `inner` assigns, `outer` assigns too.
fn subsumes(outer: &str, inner: &str) -> bool {
    outer
        .bytes()
        .zip(inner.bytes())
        .all(|(o, i)| o == b'-' || o == i)
}

/// The lowest combination both patterns assign; they must overlap.
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

const UNCONSTRAINED_FIX: &str = "Fix: delete whichever of the two you did not mean — a `-` output says the combination is \
     deliberately unconstrained, and the other row constrains it, so the table reads differently \
     depending on which one stays";

/// A table with rows, none of which asserts an output.
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

/// The finding for `row`, which assigns a combination the earlier `noted` row already assigns.
///
/// `noted` is the first earlier row that contradicts `row`, else the first it overlaps.
fn overlapping_row(row: &TruthRow, noted: &TruthRow, noted_is_reported: bool) -> Diagnostic {
    let pattern = &row.inputs;
    let earlier = &noted.inputs;
    let shared = shared_combination(earlier, pattern);
    // Past this match the two rows agree on the output.
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

/// `fix`, unless the noted row is told to change too, when following both repairs could leave a
/// combination unassigned.
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

/// The combinations no accepted row assigns, or `None` when complete or past a `u64` count.
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

/// A plain sum, since no two accepted rows share a combination; `None` past `u128`.
fn assigned_count(accepted: &[&TruthRow]) -> Option<u128> {
    accepted.iter().try_fold(0u128, |sum, row| {
        sum.checked_add(pattern_size(&row.inputs)?)
    })
}

/// `2^d` for a pattern with `d` don't-cares, or `None` past `u128`.
fn pattern_size(pattern: &str) -> Option<u128> {
    let dashes = u32::try_from(pattern.bytes().filter(|b| *b == b'-').count())
        .expect("a pattern is bounded by the source length");
    1u128.checked_shl(dashes)
}

/// Up to [`MISSING_SAMPLE`] of the lowest combinations no accepted row assigns, ascending.
///
/// Iterative, not recursive: a frame per input could overflow the stack on a wide table.
fn missing_sample(arity: u32, accepted: &[&TruthRow]) -> Vec<String> {
    let width = usize::try_from(arity)
        .expect("`arity` was converted from a `usize` length, so it fits one");
    let mut descent = Descent::new(accepted);
    let mut sample = Vec::new();
    // Untried branch points: the prefix length to retract to, and the digit to append.
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

/// Stacks both digits for position `len`, `0` on top so the sample comes out ascending.
fn branch(pending: &mut Vec<(usize, u8)>, len: usize) {
    pending.extend(b"01".iter().rev().map(|&digit| (len, digit)));
}

/// A prefix, and what each accepted row says under it.
struct Descent<'a> {
    rows: &'a [&'a TruthRow],
    /// The digits chosen so far, as ASCII `0` and `1`.
    prefix: Vec<u8>,
    /// Per row, the first position where it disagrees with the prefix.
    parted_at: Vec<Option<usize>>,
    /// Per row, its don't-cares past the prefix.
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

    /// Undoes [`Descent::extend`] exactly, back to a `len`-digit prefix.
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

    /// Whether the rows that have not parted assign every combination starting with the prefix.
    ///
    /// Sums their `2^free` in binary, a carry at a time, so no width overflows an integer.
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

/// `2^arity`, or `None` past `u128`, since the grammar puts no ceiling on the input list.
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

fn outcome(output: Option<bool>) -> String {
    match output {
        Some(value) => format!("the output `{}`", bit(value)),
        None => "no output, by its `-`".to_owned(),
    }
}
