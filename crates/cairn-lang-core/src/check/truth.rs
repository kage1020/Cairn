//! `truth` pass — flags an `assert truth(...)` table that verifies less
//! than it looks like it does.
//!
//! The parser already refuses a row it cannot read: a digit that is not
//! `0`/`1`, and a pattern whose width is not the number of signals left of
//! the arrow. Both are properties of one row against the header, which is
//! why they live where the header is in hand. What no row can see is the
//! table around it, and three shapes of table verify nothing while reading
//! — in a diff, in a review — exactly like one that passes:
//!
//! * no rows at all;
//! * a pattern assigned twice, the two rows disagreeing;
//! * combinations left unassigned, which are the ones a bug hides in.
//!
//! Severity follows what is provable, the split [`super::diagnostic`]
//! records per code. A table with no rows can never assert anything — no
//! context around it and no pass written later changes that — so it is an
//! error, the same argument `E_INVALID_REQUIRES` makes for a `@requires`
//! the compiler cannot read. Two rows that assign one input combination
//! different outputs describe a circuit that cannot exist, so that is an
//! error too. A table merely short of rows still asserts everything its
//! rows say, and a four-input table is sixteen rows an author part way
//! through should not be blocked on, so that is a warning — as is a row
//! that repeats one already written and agrees with it, where the repair
//! is to delete a line and nothing else moves.
//!
//! Nothing evaluates a truth table yet — the tick simulator is unbuilt
//! — and none of this waits on it. Which of
//! two disagreeing rows an evaluator would read is the one thing these
//! messages will not say: there is no evaluator to describe, and the
//! author's next action is to decide which row is wrong either way.
//!
//! Scope of the walk: the `asserts` of every `def`, `struct`, and `site`,
//! and those of every member body under them. A shape fault belongs to the
//! statement wherever it is written, and redstone synthesis already reads a
//! nested `assert` — its `collect_member` extends the scope's list with
//! `children.asserts` — so a table under a `level` is checked against its
//! signal names today and would be the one place a shape went unreported.

use std::collections::BTreeMap;

use crate::ast::TruthRow;
use crate::error::Span;
use crate::intent::{AssertIr, IntentModule, Member, MemberBody};
use crate::prose::and_list;

use super::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote, DiagnosticSink};

/// How many unassigned combinations a finding names, in the sentence and
/// in the payload alike.
///
/// A sample rather than the set. Four shows the shape of what is missing
/// for the table sizes anyone writes by hand, and the cap is what keeps a
/// twenty-input table from building a million strings to describe a
/// one-row mistake.
const MISSING_SAMPLE: usize = 4;

/// Past this many inputs the number of combinations is written `2^n`.
///
/// `2^32` is ten digits and already far past any table written by hand; a
/// longer decimal is not a number a reader compares a row count against.
/// Beyond 127 inputs there is no integer to render at all, which is the
/// same branch.
const DECIMAL_TOTAL_MAX_INPUTS: u32 = 32;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for def in &ir.defs {
        walk(&def.members, &def.asserts, sink);
    }
    for item in &ir.structs {
        walk(&item.members, &item.asserts, sink);
    }
    for site in &ir.sites {
        walk(&site.placements, &site.asserts, sink);
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
    let arity = u32::try_from(inputs.len()).expect("an input list is bounded by the source length");
    if rows.is_empty() {
        // And nothing else. An empty table is trivially missing every
        // combination, and a coverage finding beside this one would bill
        // one repair twice.
        sink.push(empty_table(span, arity));
        return;
    }

    // Rows do not overlap. A `-` is shorthand for the rows it stands
    // for, so two rows that share a combination write that combination
    // twice — the same thing writing one pattern twice has always been,
    // now read through the don't-cares. Nothing orders the rows, so
    // there is no reading under which the second one wins.
    //
    // A row is compared with every earlier row for a contradiction,
    // dropped or not. A row with a `-` can overlap several earlier rows
    // at once, and those rows can disagree with each other: `0- -> 1`
    // after `00 -> 1; 01 -> 0` agrees with the first and contradicts the
    // second. Asking only one of them would make whether the table is
    // refused depend on the order its rows are written in, and the error
    // is defined by the combination, not by the order. A dropped row is
    // still a row the author wrote, so `10 -> 0` after `0- -> 1; -0 -> 1`
    // contradicts the `-0` it never meets in the accepted set.
    //
    // A conflict is noted at the first earlier row that contradicts this
    // one. Any other finding is noted at the first earlier row this one
    // overlaps, dropped or not. No row before that one shares a
    // combination with this one, so it is the first row assigning the
    // combination the finding names, and every finding naming that
    // combination sends the author to the same row.
    //
    // A row that overlaps an accepted one is not accepted, so what
    // survives is a set of patterns no two of which share a combination.
    // Coverage is then the sum of their sizes and needs no union.
    let mut accepted: Vec<&TruthRow> = Vec::with_capacity(rows.len());
    // Whether what was dropped is still accounted for. A row inside the
    // row it answers to — `01` under `0-`, and the exact repeat that has
    // always been this — assigns nothing the accepted set does not, so
    // dropping it leaves the count right. A row that merely crosses one
    // does assign something new, and counting without it would name a
    // combination as missing that the source assigns on the line above.
    // A wrong coverage finding is worse than none, and the author's next
    // edit is the overlap either way — the argument the empty table
    // already makes against billing one repair twice.
    //
    // A row can be accepted and still carry a finding, when every row it
    // overlaps was dropped. That leaves the count sound: a dropped row D
    // overlapped an accepted row A, and were D inside A, any row
    // overlapping D would overlap A too and not be accepted. So D crossed
    // A, which already cleared this flag.
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

    // A table every row of which declines to assert an output is the
    // empty table written at length, so it earns the empty table's
    // finding — and not the coverage one beside it, which would bill the
    // same repair twice.
    //
    // Asked of `rows` rather than of `accepted`, because acceptance is
    // first-come: a row is dropped for overlapping an *earlier* one, so
    // a broad `-` row written first survives and the concrete row after
    // it is the one that goes. Reading the survivors calls the table
    // empty while the author is looking at a `0` in it, and `-` first
    // with the exceptions after is the ordinary way to write one. What
    // the author wrote is what this sentence is about; that the two rows
    // overlap is a separate finding, already raised above.
    if rows.iter().all(|row| row.output.is_none()) {
        sink.push(asserts_nothing(span, arity));
    } else if coverage_is_countable
        && let Some(finding) = unassigned_combinations(span, arity, &accepted)
    {
        sink.push(finding);
    }
}

/// Whether two patterns assign a combination in common.
///
/// Position by position: they share one unless both are concrete and
/// differ. A `-` agrees with anything, which is what makes `0-` and `-1`
/// overlap at `01` while `00` and `01` overlap nowhere.
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
/// Where both are don't-cares any value would do and `0` is chosen, so
/// the witness is the lowest combination the two share. The caller has
/// checked they overlap, so no position disagrees.
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
/// declines to, or the other way round.
///
/// Not a conflict: no circuit is being asked for two outputs, so the code
/// stays in the duplicate-row family. But the repair is not the
/// duplicate-row family's either — "delete either row" is what a pair
/// saying the same thing earns, and these two do not say the same thing,
/// so which one goes changes the table.
const UNCONSTRAINED_FIX: &str = "Fix: delete whichever of the two you did not mean — a `-` output says the combination is \
     deliberately unconstrained, and the other row constrains it, so the table reads differently \
     depending on which one stays";

/// A table with rows, none of which asserts an output.
///
/// The same code as [`empty_table`], because it is the same table: a row
/// whose output is `-` marks its combinations as deliberately
/// unconstrained, and a table of nothing but those constrains nothing.
/// The sentence differs because the repair does — there are rows to
/// change here, not rows to add.
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
/// One code when the two outputs contradict each other and another when
/// they do not: the repairs are different work, and severity is a property
/// of the code. Only two concrete outputs can contradict — a `-` asserts
/// nothing to be contradicted, so a `-` meeting a `0` is the second code,
/// a row written twice rather than a circuit asked for two things.
///
/// The agreeing case carries three sentences rather than one, because the
/// repair is three different edits. A row repeating an earlier pattern
/// exactly is a line to delete. A row inside an earlier one — `01` under
/// `0-` — is also a line to delete, but the reader has to be told which
/// row already covers it, since the two do not read alike. A row merely
/// crossing an earlier one — `-1` against `0-` — asserts something the
/// earlier row does not, so deleting it would lose coverage, and the edit
/// is to narrow one of the two.
///
/// A `-` output beside a concrete one is a fourth sentence and neither
/// code's usual repair: the rows do not contradict, since a `-` asserts
/// nothing to contradict, but they do not agree either, and deleting the
/// wrong one changes what the table says. Splitting it off is also what
/// lets the three sentences below name one output for both rows, which
/// they have to, since each is about what the table already says.
///
/// For a conflict, `noted` is the first earlier row contradicting this
/// one, so the note names the output it assigns: no row before it assigns
/// the shared combination that output, since such a row would contradict
/// this one too and would have been chosen instead. The row the table
/// opened the combination with may well agree with this one, which is
/// why the note cannot just say "first row assigning" as the other
/// findings' notes do.
///
/// Every other sentence's repair assumes `noted` stays as written. When
/// `noted` carries an overlap finding of its own (`noted_is_reported`),
/// it is being asked to change too, and "delete this row, the earlier
/// one stands for it" can cancel against that: `10 -> 1` under the `-0`
/// of `0- -> 1; -0 -> 1` is inside `-0`, while `-0` is told to narrow
/// away from `0-`, and following both leaves `10` unassigned. So the
/// repair there is to settle the earlier row first, and this row's fate
/// follows from how that one changes.
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
/// `None` covers two cases, and only the first is "the table is
/// complete". The other is a count the compiler cannot state: a covered
/// total past `u64`, which the payload carries. That needs 64 or more
/// don't-cares in one row, an input list far past anything written by
/// hand, and a finding whose numbers are guesses is worse than one that
/// is missing — so the finding is withheld rather than approximated.
///
/// `accepted` is pairwise non-overlapping, which is what `check_table`
/// drops a row to keep, so the covered total is the sum of the rows'
/// sizes and no union is computed.
fn unassigned_combinations(span: &Span, arity: u32, accepted: &[&TruthRow]) -> Option<Diagnostic> {
    let total = combination_count(arity);
    let covered = assigned_count(accepted)?;
    if total == Some(covered) {
        return None;
    }
    let covered_count = u64::try_from(covered).ok()?;
    let sample = missing_sample(arity, accepted);
    if sample.is_empty() {
        return None;
    }
    let quoted: Vec<String> = sample.iter().map(|p| format!("`{p}`")).collect();
    let missing_total = total.map(|total| total - covered);
    // The sample stops at the cap, so the sentence has to say it is a
    // sample — a list of four that reads as the whole set is the opposite
    // of what a coverage finding is for. The count is arithmetic and needs
    // no walk; past 127 inputs there is no integer for it, and the shorter
    // sentence is the honest one.
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
/// A plain sum, because no two accepted rows share a combination. A row
/// with `d` don't-cares stands for `2^d` of them.
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
/// Found by descending through prefixes rather than by counting up
/// through combinations. A prefix stands for every combination that
/// starts with it, and the rows assign all of those exactly when the
/// combinations they assign under the prefix add up to that many — a
/// sum, because no two accepted rows share a combination. The descent
/// tries `0` before `1` and passes over a prefix the rows fill, so every
/// prefix it enters has a missing combination below it: it never backs
/// out of one empty-handed. That bounds the work by the sample, the
/// width and the rows, whatever the number of combinations — two rows
/// fixing one input each of forty leave half of `2^40` missing, and the
/// first of those is forty steps down.
fn missing_sample(arity: u32, accepted: &[&TruthRow]) -> Vec<String> {
    let width = usize::try_from(arity).expect("an input list is bounded by the source length");
    let mut prefix = Vec::with_capacity(width);
    let mut sample = Vec::new();
    descend(&mut prefix, width, accepted, &mut sample);
    sample
}

/// Add to `sample` the lowest combinations under `prefix` that no row
/// assigns, until it holds [`MISSING_SAMPLE`]. `under` is the rows that
/// assign some combination starting with `prefix`.
fn descend(prefix: &mut Vec<u8>, width: usize, under: &[&TruthRow], sample: &mut Vec<String>) {
    if prefix.len() == width {
        sample
            .push(String::from_utf8(prefix.clone()).expect("the descent writes `0` and `1` only"));
        return;
    }
    for value in *b"01" {
        if sample.len() == MISSING_SAMPLE {
            return;
        }
        prefix.push(value);
        let at = prefix.len() - 1;
        let still: Vec<&TruthRow> = under
            .iter()
            .copied()
            .filter(|row| {
                row.inputs
                    .as_bytes()
                    .get(at)
                    .is_some_and(|fixed| *fixed == b'-' || *fixed == value)
            })
            .collect();
        if !fills(&still, prefix.len(), width) {
            descend(prefix, width, &still, sample);
        }
        prefix.pop();
    }
}

/// Whether `rows`, every one of which assigns some combination starting
/// with a prefix `len` long, assign every combination starting with it.
///
/// Under the prefix a row assigns `2^d` combinations, `d` its
/// don't-cares past the prefix, and no two rows share one, so they fill
/// the `2^(width - len)` combinations there exactly when those powers
/// add up to it. Added in binary, one carry at a time, so no width
/// overflows an integer.
fn fills(rows: &[&TruthRow], len: usize, width: usize) -> bool {
    let mut bits: BTreeMap<usize, usize> = BTreeMap::new();
    for row in rows {
        let dashes = row
            .inputs
            .get(len..)
            .unwrap_or_default()
            .matches('-')
            .count();
        *bits.entry(dashes).or_default() += 1;
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

/// `2^arity`, or `None` when no integer the compiler carries holds it.
///
/// The grammar puts no ceiling on the input list — a twenty-input table is
/// already in the tree-sitter corpus — so this is a real branch and not a
/// defensive one.
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
