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

/// How many steps the walk for a missing combination may take before it
/// gives the finding up.
///
/// Generous on purpose: it is a backstop against a table no one writes,
/// not a budget any real table comes near.
///
/// A step leaves a row behind only when [`fill_free_tail`] can jump to
/// the row's last combination, and it fills only the positions *below*
/// the row's lowest fixed one. Don't-cares above that are re-entered one
/// combination at a time, so the cost is nearer
/// `rows × 2^(don't-cares above the lowest fixed position) ×
/// MISSING_SAMPLE`. `{ --00; --01; --10 }` is three rows and sixteen
/// steps for its four missing combinations, not three.
const MISSING_WALK_STEPS: usize = 10_000;

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
    // Each overlap is judged against the *first* accepted row it meets,
    // not against the row before it. Two rows that flip an assignment
    // and then flip it back are then a conflict and a repeat rather than
    // two conflicts, and — the reason to prefer it — every finding about
    // a combination sends the author to the same row to compare
    // against, which is the row that has to stay if any of them do.
    //
    // A row that overlaps is not accepted, so what survives is a set of
    // patterns no two of which share a combination. Coverage is then the
    // sum of their sizes and needs no union.
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
    let mut coverage_is_countable = true;
    for row in rows {
        match accepted
            .iter()
            .find(|earlier| overlap(&earlier.inputs, &row.inputs))
        {
            None => accepted.push(row),
            Some(first) => {
                coverage_is_countable &= subsumes(&first.inputs, &row.inputs);
                sink.push(overlapping_row(row, first));
            }
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
fn overlapping_row(row: &TruthRow, first: &TruthRow) -> Diagnostic {
    let pattern = &row.inputs;
    let earlier = &first.inputs;
    let shared = shared_combination(earlier, pattern);
    // Everything below the match reads one output for both rows, which is
    // only sound once the cases where they say different things are gone.
    let disagreement = match (row.output, first.output) {
        (Some(later), Some(earlier_output)) if later != earlier_output => Some((
            DiagnosticCode::TruthTableConflict,
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
            format!(
                "this row leaves `{shared}` unconstrained, and the earlier row `{earlier}` \
                 assigns it the output `{output}`",
                output = bit(assigned),
            ),
            UNCONSTRAINED_FIX.to_owned(),
        )),
        (Some(assigned), None) => Some((
            DiagnosticCode::TruthTableDuplicateRow,
            format!(
                "this row assigns `{shared}` the output `{output}`, and the earlier row \
                 `{earlier}` leaves it unconstrained",
                output = bit(assigned),
            ),
            UNCONSTRAINED_FIX.to_owned(),
        )),
        _ => None,
    };
    if let Some((code, primary, fix)) = disagreement {
        return finding(code, row, first, &shared, primary, fix);
    }
    let (primary, fix) = if pattern == earlier {
        (
            format!(
                "this row repeats an earlier one: `{pattern}` is already assigned {outcome}",
                outcome = outcome(first.output),
            ),
            "Fix: delete either row — the table asserts the same thing without it".to_owned(),
        )
    } else if subsumes(earlier, pattern) {
        (
            format!(
                "this row asserts nothing new: the earlier row `{earlier}` already assigns \
                 `{pattern}` {outcome}",
                outcome = outcome(first.output),
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
    finding(
        DiagnosticCode::TruthTableDuplicateRow,
        row,
        first,
        &shared,
        primary,
        fix,
    )
}

/// The shape every overlap finding shares: reported on the later row,
/// with the earlier one noted and the fix last.
fn finding(
    code: DiagnosticCode,
    row: &TruthRow,
    first: &TruthRow,
    shared: &str,
    primary: String,
    fix: String,
) -> Diagnostic {
    Diagnostic {
        code,
        span: row.span.clone(),
        primary,
        notes: vec![
            DiagnosticNote {
                span: Some(first.span.clone()),
                message: format!("first row assigning `{shared}` here"),
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
/// `None` covers three cases, and only the first is "the table is
/// complete". The other two are counts the compiler cannot state: a
/// covered total past `u64`, which the payload carries, and a walk that
/// ran out of steps before it found a missing combination. Both need an
/// input list far past anything written by hand, and a finding whose
/// numbers are guesses is worse than one that is missing — so the finding
/// is withheld rather than approximated.
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
    let sample = missing_sample(arity, accepted)?;
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
/// order, or `None` when the walk ran out of steps.
///
/// Counting up and skipping what is covered, rather than building the
/// space and subtracting. The skip is what keeps that affordable: landing
/// inside a row, the walk does not step through the row one combination
/// at a time but jumps to the last one it assigns without leaving it, so
/// a row standing for a million combinations costs one step rather than a
/// million.
///
/// [`MISSING_WALK_STEPS`] is the backstop. A step either yields a missing
/// combination or advances past one the rows cover, and only the second
/// of those leaves a row behind for good — see the constant for what
/// that costs a row whose don't-cares sit above its fixed positions. A
/// table that runs the budget out is one whose sample would not help
/// anyone read it.
fn missing_sample(arity: u32, accepted: &[&TruthRow]) -> Option<Vec<String>> {
    let width = usize::try_from(arity).expect("an input list is bounded by the source length");
    let mut combination = vec![b'0'; width];
    let mut sample = Vec::new();
    let mut steps = 0usize;
    while sample.len() < MISSING_SAMPLE {
        steps += 1;
        if steps > MISSING_WALK_STEPS {
            return None;
        }
        match accepted
            .iter()
            .find(|row| assigns(&row.inputs, &combination))
        {
            None => sample.push(
                String::from_utf8(combination.clone()).expect("the walk writes `0` and `1` only"),
            ),
            Some(row) => fill_free_tail(&row.inputs, &mut combination),
        }
        if !increment(&mut combination) {
            break;
        }
    }
    Some(sample)
}

/// Whether `pattern` assigns this combination: every position it fixes
/// agrees, and the rest are its don't-cares.
fn assigns(pattern: &str, combination: &[u8]) -> bool {
    pattern
        .bytes()
        .zip(combination)
        .all(|(p, c)| p == b'-' || p == *c)
}

/// Raise a combination the row assigns to the last one it assigns before
/// the row's lowest fixed position has to change.
///
/// Every position below that one is a don't-care, so setting them all to
/// `1` skips only combinations this row already assigns, and the
/// increment that follows carries into the fixed position and leaves the
/// row. A row that fixes nothing assigns everything, so the walk is never
/// called on one — it would mean a complete table, which returns earlier.
fn fill_free_tail(pattern: &str, combination: &mut [u8]) {
    let Some(lowest_fixed) = pattern.bytes().rposition(|b| b != b'-') else {
        return;
    };
    for (slot, _) in combination
        .iter_mut()
        .zip(pattern.bytes())
        .skip(lowest_fixed + 1)
    {
        *slot = b'1';
    }
}

/// Step to the next combination, or report there is none left.
fn increment(combination: &mut [u8]) -> bool {
    for slot in combination.iter_mut().rev() {
        if *slot == b'0' {
            *slot = b'1';
            return true;
        }
        *slot = b'0';
    }
    false
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
