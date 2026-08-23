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
//! Nothing evaluates a truth table yet — the simulator `roadmap.md`
//! schedules for M6 is unbuilt — and none of this waits on it. Which of
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

use std::collections::{HashMap, HashSet};

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

    // Each repeat is judged against the *first* row carrying its pattern,
    // not against the one before it. Two rows that flip an assignment and
    // then flip it back are then a conflict and a repeat rather than two
    // conflicts, and — the reason to prefer it — every finding about a
    // pattern sends the author to the same row to compare against, which
    // is the row that has to stay if any of them do.
    let mut first_by_pattern: HashMap<&str, &TruthRow> = HashMap::new();
    for row in rows {
        match first_by_pattern.get(row.inputs.as_str()) {
            None => {
                first_by_pattern.insert(row.inputs.as_str(), row);
            }
            Some(first) => sink.push(repeated_pattern(row, first)),
        }
    }

    let covered: HashSet<&str> = first_by_pattern.keys().copied().collect();
    if let Some(finding) = unassigned_combinations(span, arity, &covered) {
        sink.push(finding);
    }
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

/// A row whose pattern an earlier row already assigned.
///
/// One code when the two outputs differ and another when they agree: the
/// repairs are different work, and severity is a property of the code.
fn repeated_pattern(row: &TruthRow, first: &TruthRow) -> Diagnostic {
    let pattern = &row.inputs;
    let conflicts = row.output != first.output;
    let (code, primary, fix) = if conflicts {
        (
            DiagnosticCode::TruthTableConflict,
            format!(
                "this row assigns `{pattern}` the output `{output}`, and an earlier row assigns \
                 it `{earlier}`",
                output = bit(row.output),
                earlier = bit(first.output),
            ),
            "Fix: decide which of the two the circuit should do and delete the other row — no \
             circuit produces both outputs for one input combination",
        )
    } else {
        (
            DiagnosticCode::TruthTableDuplicateRow,
            format!(
                "this row repeats an earlier one: `{pattern}` is already assigned the output \
                 `{output}`",
                output = bit(row.output),
            ),
            "Fix: delete either row — the table asserts the same thing without it",
        )
    };
    Diagnostic {
        code,
        span: row.span.clone(),
        primary,
        notes: vec![
            DiagnosticNote {
                span: Some(first.span.clone()),
                message: format!("first row for `{pattern}` here"),
            },
            DiagnosticNote {
                span: None,
                message: fix.to_owned(),
            },
        ],
        data: None,
    }
}

/// The combinations no row assigns, or `None` when the table is complete.
fn unassigned_combinations(span: &Span, arity: u32, covered: &HashSet<&str>) -> Option<Diagnostic> {
    let sample = missing_sample(arity, covered);
    if sample.is_empty() {
        return None;
    }
    let quoted: Vec<String> = sample.iter().map(|p| format!("`{p}`")).collect();
    let covered_count =
        u64::try_from(covered.len()).expect("a row count is bounded by the source length");
    let missing_total = combination_count(arity).map(|total| total - u128::from(covered_count));
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

/// The lowest few patterns no row assigns, in ascending order.
///
/// Counting up and skipping what is covered, rather than building the
/// space and subtracting: the loop runs at most `covered.len() +
/// MISSING_SAMPLE` times whatever the arity, so a twenty-input table costs
/// the same as a two-input one.
fn missing_sample(arity: u32, covered: &HashSet<&str>) -> Vec<String> {
    let total = combination_count(arity);
    let mut sample = Vec::new();
    let mut candidate = 0u128;
    while sample.len() < MISSING_SAMPLE && total.is_none_or(|total| candidate < total) {
        let pattern = pattern_text(candidate, arity);
        if !covered.contains(pattern.as_str()) {
            sample.push(pattern);
        }
        candidate += 1;
    }
    sample
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

/// A combination spelled the way a row spells it: one character per input,
/// leading zeros kept, because `01` and `10` are different rows.
fn pattern_text(value: u128, arity: u32) -> String {
    let width = usize::try_from(arity).expect("an input list is bounded by the source length");
    format!("{value:0width$b}")
}

fn bit(output: bool) -> char {
    if output { '1' } else { '0' }
}
