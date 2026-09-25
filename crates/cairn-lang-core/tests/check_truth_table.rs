//! A truth table is checked as a table, not only row by row.
//!
//! Row-level checks live in the parser, where the input arity is in hand:
//! a row's digits and its width are refused there (`parse_truth_rows.rs`).
//! What no row can see is the table around it — that it has no rows at
//! all, that another row already assigned the same inputs, or that the
//! combinations it leaves out are the ones a bug would hide in. All three
//! read, in a diff, exactly like a table that verifies something.
//!
//! Severity follows what is provable. A table with no rows can never
//! assert anything, whatever is written later around it, so it is an
//! error. A table missing rows asserts everything its rows say — the
//! finding is about coverage, not about the statement being void — so it
//! is a warning. Two rows that assign the same inputs different outputs
//! describe a circuit that cannot exist, so that is an error again, while
//! two that agree cost nothing but the line.

use cairn_lang_core::Diagnostic;
use cairn_lang_core::check::{DiagnosticData, Severity};

mod common;
use common::{codes, diagnose};

fn table(inputs: &str, rows: &str) -> String {
    format!("struct s size=3x3\n  assert truth({inputs} -> sig.o) {{ {rows} }}\n")
}

/// Every combination of two inputs, so a fixture can add a repeat without
/// also going partial and earning a second finding for it.
fn complete_plus(extra: &str) -> String {
    table("sig.a, sig.b", &format!("{extra}; 01->0; 10->0; 11->0"))
}

/// The one finding a source is expected to raise, with nothing else
/// alongside it — the count is part of what these tests pin.
fn only(source: &str) -> Diagnostic {
    let found = diagnose(source);
    assert_eq!(
        found.len(),
        1,
        "{source:?} should raise exactly one finding, got {:?}",
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
    );
    found[0].clone()
}

/// Everything a finding renders, so a test can ask what the author reads
/// without caring whether it landed in the sentence or in a note.
fn rendered(found: &Diagnostic) -> String {
    let mut text = found.primary.clone();
    for note in &found.notes {
        text.push(' ');
        text.push_str(&note.message);
    }
    text
}

/// The source text a span underlines, which is how these tests say *which*
/// row a finding is about: the row number is not in the message and a byte
/// offset is not readable in a failure.
fn underlined(source: &str, span: &std::ops::Range<usize>) -> String {
    source[span.clone()].to_owned()
}

/// The coverage clause alone, so a test can compare two tables' counts
/// without either assertion carrying the rest of the sentence.
fn found_covers(found: &Diagnostic) -> String {
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_PARTIAL");
    let text = found.primary.clone();
    let start = text
        .find("assigns")
        .expect("the coverage sentence says `assigns`");
    let end = text
        .find(" combinations")
        .expect("and `combinations` after the counts");
    text[start..end].to_owned()
}

// -- a table that says something -----------------------------------------

/// The shape every other test is a deviation from.
#[test]
fn a_complete_table_is_quiet() {
    assert!(codes(&table("sig.a, sig.b", "00->0; 01->0; 10->0; 11->0")).is_empty());
    assert!(codes(&table("sig.a", "0->1; 1->0")).is_empty());
}

/// `01` and `10` assign different inputs, whatever an integer reading of
/// the lexeme would say about them.
#[test]
fn two_patterns_that_differ_only_in_order_are_different_rows() {
    assert!(codes(&table("sig.a, sig.b", "00->0; 01->1; 10->0; 11->1")).is_empty());
}

// -- no rows --------------------------------------------------------------

#[test]
fn a_table_with_no_rows_is_refused() {
    let found = only(&table("sig.a, sig.b", ""));
    assert_eq!(found.code.as_str(), "E_TRUTH_TABLE_EMPTY");
    assert_eq!(found.severity(), Severity::Error);
}

/// The author's next move is to write rows, so the message says how many
/// the inputs they declared call for.
#[test]
fn an_empty_table_says_how_many_rows_its_inputs_need() {
    let found = only(&table("sig.a, sig.b", ""));
    let text = rendered(&found);
    assert!(
        text.contains('4'),
        "the message should name the four combinations two inputs have: {text:?}",
    );
}

/// An empty table is also, trivially, a table missing every row. Reporting
/// both would bill one repair twice.
#[test]
fn an_empty_table_is_not_also_reported_as_partial() {
    assert_eq!(codes(&table("sig.a, sig.b", "")), ["E_TRUTH_TABLE_EMPTY"]);
}

/// One input is one input, and the sentence around the count has to
/// agree with it. The two messages that carry a count are the two that
/// have somewhere for a verb to disagree.
#[test]
fn a_table_with_one_input_reads_as_a_sentence() {
    for source in [table("sig.a", ""), table("sig.a", "0->1")] {
        let text = rendered(&only(&source));
        assert!(
            text.contains("1 input can take") && !text.contains("inputs"),
            "a one-input table should not be described in the plural: {text:?}",
        );
    }
}

// -- a pattern assigned twice ---------------------------------------------

#[test]
fn a_pattern_assigned_two_different_outputs_is_refused() {
    let source = complete_plus("00->0; 00->1");
    let found = only(&source);
    assert_eq!(found.code.as_str(), "E_TRUTH_TABLE_CONFLICT");
    assert_eq!(found.severity(), Severity::Error);
    assert_eq!(
        underlined(&source, &found.span),
        "00->1",
        "the finding belongs to the row that repeats, not to the first one",
    );
    let note = found
        .notes
        .first()
        .expect("the conflict should point at the row it disagrees with");
    assert_eq!(
        underlined(
            &source,
            note.span.as_ref().expect("the note carries a span")
        ),
        "00->0",
    );
}

/// Nothing reads a truth table yet — the evaluator is unbuilt — so a
/// message that says which of the two rows would be used is describing a
/// program that does not exist.
#[test]
fn the_conflict_does_not_say_which_row_would_win() {
    let text = rendered(&only(&complete_plus("00->0; 00->1")));
    for claim in ["wins", "takes precedence", "overrides", "the last"] {
        assert!(
            !text.contains(claim),
            "the message claims an outcome no implementation decides: {text:?}",
        );
    }
}

#[test]
fn a_pattern_assigned_the_same_output_twice_is_a_warning() {
    let source = complete_plus("00->0; 00->0");
    let found = only(&source);
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_DUPLICATE_ROW");
    assert_eq!(found.severity(), Severity::Warning);
    assert_eq!(underlined(&source, &found.span), "00->0");
    assert!(
        found.span.start > source.find("00->0").expect("the first row"),
        "the finding belongs to the repeat, and the two rows read alike",
    );
}

/// Each repeat is judged against the *first* row carrying its pattern, so
/// a table that flips back reports what each row does to the assignment
/// the table opened with, and both point at the same place to look.
#[test]
fn a_repeat_is_judged_against_the_first_row_with_its_pattern() {
    let source = complete_plus("00->0; 00->1; 00->0");
    let found = diagnose(&source);
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E_TRUTH_TABLE_CONFLICT", "W_TRUTH_TABLE_DUPLICATE_ROW"],
    );
    let first = source.find("00->0").expect("the first row");
    for d in &found {
        assert_eq!(
            d.notes[0].span.as_ref().expect("a note span").start,
            first,
            "every repeat should send the author to the row that set the pattern",
        );
    }
}

/// The other half of that rule: two rows that agree with each other and
/// disagree with the first are two conflicts, not one conflict and one
/// duplicate.
#[test]
fn two_repeats_that_agree_with_each_other_still_answer_to_the_first_row() {
    let found = codes(&complete_plus("00->0; 00->1; 00->1"));
    assert_eq!(found, ["E_TRUTH_TABLE_CONFLICT", "E_TRUTH_TABLE_CONFLICT"]);
}

// -- combinations left out ------------------------------------------------

#[test]
fn a_table_short_of_a_combination_is_a_warning() {
    let found = only(&table("sig.a, sig.b", "00->0"));
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_PARTIAL");
    assert_eq!(found.severity(), Severity::Warning);
    let text = rendered(&found);
    for missing in ["01", "10", "11"] {
        assert!(
            text.contains(missing),
            "the message should name the rows to write, missing {missing}: {text:?}",
        );
    }
    assert!(
        text.contains('4'),
        "and the number of combinations two inputs have: {text:?}",
    );
    assert!(
        !text.contains("more"),
        "three missing rows are the whole set, not a sample of it: {text:?}",
    );
}

/// The sentence says "and N more" only when there is an N, and the
/// boundary is the cap itself. Three inputs are eight combinations, so a
/// table covering four leaves exactly the cap and one covering three
/// leaves one past it — the pair that tells a cap of four from a cap of
/// three or five, which nothing else here does.
#[test]
fn the_sample_says_it_is_a_sample_only_when_it_is_one() {
    let whole = rendered(&only(&table(
        "sig.a, sig.b, sig.c",
        "000->0; 001->0; 010->0; 011->0",
    )));
    assert!(
        !whole.contains("more"),
        "four missing rows are exactly what the sentence lists: {whole:?}",
    );
    let sampled = rendered(&only(&table(
        "sig.a, sig.b, sig.c",
        "000->0; 001->0; 010->0",
    )));
    assert!(
        sampled.contains("and 1 more"),
        "five missing rows are one past what the sentence lists: {sampled:?}",
    );
}

/// A repeated row does not fill the slot it repeats, so the two findings
/// stand together: one row to delete, three to write.
///
/// The coverage finding comes first because `check` sorts by span and the
/// statement opens before any of its rows — the row-level finding sits
/// inside the range of the table-level one.
#[test]
fn a_repeated_row_leaves_the_combination_it_repeats_uncovered() {
    let found = diagnose(&table("sig.a, sig.b", "00->0; 00->0"));
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["W_TRUTH_TABLE_PARTIAL", "W_TRUTH_TABLE_DUPLICATE_ROW"],
    );
    assert!(
        rendered(&found[0]).contains("11"),
        "the partial finding counts distinct patterns, not rows: {:?}",
        found[0],
    );
}

/// Twenty inputs is a million combinations. The finding still fires and
/// still names the total, but nothing here walks that space: the count is
/// arithmetic and the sample stops at the cap.
#[test]
fn a_wide_table_is_counted_rather_than_enumerated() {
    let names: Vec<String> = (0..20).map(|i| format!("sig.a{i}")).collect();
    let found = only(&table(&names.join(", "), &format!("{}->1", "0".repeat(20))));
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_PARTIAL");
    let text = rendered(&found);
    assert!(
        text.contains("1048576"),
        "the total is exact whether or not the rows are listed: {text:?}",
    );
    assert!(
        text.contains("more"),
        "and the sample says it is a sample: {text:?}",
    );
}

/// Wide enough that the number of combinations does not fit any integer
/// the compiler carries. The grammar permits it, so it must not panic and
/// must not print a number it cannot compute.
#[test]
fn a_table_too_wide_to_count_says_so_symbolically() {
    let names: Vec<String> = (0..130).map(|i| format!("sig.a{i}")).collect();
    let found = only(&table(
        &names.join(", "),
        &format!("{}->1", "0".repeat(130)),
    ));
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_PARTIAL");
    let text = rendered(&found);
    assert!(
        text.contains("2^130"),
        "a total no integer holds is written as a power: {text:?}",
    );
}

/// The decimal gives way to a power at the boundary the pass chooses, not
/// at the point where no integer holds the count — those sit 95 inputs
/// apart, and only a table between them tells the two apart.
#[test]
fn the_total_becomes_a_power_before_it_stops_fitting_an_integer() {
    for (arity, expected, absent) in [(32usize, "4294967296", "2^32"), (33, "2^33", "8589934592")] {
        let names: Vec<String> = (0..arity).map(|i| format!("sig.a{i}")).collect();
        let source = table(&names.join(", "), &format!("{}->1", "0".repeat(arity)));
        let text = rendered(&only(&source));
        assert!(
            text.contains(expected) && !text.contains(absent),
            "{arity} inputs should have their total written as {expected}: {text:?}",
        );
    }
}

// -- where the table sits -------------------------------------------------

/// The findings are about the statement, so indentation does not change
/// them. Redstone synthesis already reads a nested `assert` — its
/// `collect_member` extends the scope's list with `children.asserts` — so
/// a table under a `level` is as real as one at the top of the body.
#[test]
fn a_table_nested_under_a_level_is_checked() {
    let source = "struct s size=3x3\n  level y=1\n    assert truth(sig.a -> sig.o) { }\n";
    assert_eq!(codes(source), ["E_TRUTH_TABLE_EMPTY"]);
}

#[test]
fn a_table_in_a_site_body_is_checked() {
    let source = "site s:\n  assert truth(sig.a -> sig.o) { }\n";
    assert_eq!(codes(source), ["E_TRUTH_TABLE_EMPTY"]);
}

/// A `def` no `site` places also earns `W_UNUSED_DEF`, which is about the
/// block and not about the table, so this one reads the truth findings out
/// of the list rather than asserting its length.
#[test]
fn a_table_in_a_def_body_is_checked() {
    let source = "def d size=3x3:\n  assert truth(sig.a -> sig.o) { }\n";
    let truth: Vec<&str> = codes(source)
        .into_iter()
        .filter(|c| c.contains("TRUTH"))
        .collect();
    assert_eq!(truth, ["E_TRUTH_TABLE_EMPTY"]);
}

// -- the payload ----------------------------------------------------------

/// "Write the missing rows" is the repair, and recovering the rows from the
/// sentence is the prose-parsing `spec/lint` "Machine-readable payload"
/// tells consumers to avoid.
#[test]
fn the_partial_finding_carries_the_rows_to_write() {
    let found = only(&table("sig.a, sig.b", "00->0"));
    let Some(DiagnosticData::TruthTablePartial {
        inputs,
        covered,
        missing,
    }) = found.data.clone()
    else {
        panic!("the partial finding should carry its payload, got {found:?}");
    };
    assert_eq!(inputs, 2);
    assert_eq!(covered, 1);
    assert_eq!(missing, ["01", "10", "11"]);
}

/// The payload's sample is capped for the same reason the sentence's is,
/// so a consumer must read the total off `inputs` and `covered` rather
/// than off the length of the list.
#[test]
fn the_payload_sample_is_capped_and_says_nothing_about_the_total() {
    let names: Vec<String> = (0..20).map(|i| format!("sig.a{i}")).collect();
    let found = only(&table(&names.join(", "), &format!("{}->1", "0".repeat(20))));
    let Some(DiagnosticData::TruthTablePartial {
        inputs,
        covered,
        missing,
    }) = found.data.clone()
    else {
        panic!("the partial finding should carry its payload, got {found:?}");
    };
    assert_eq!((inputs, covered), (20, 1));
    assert!(
        missing.len() < 20,
        "a million missing rows must not be materialised, got {}",
        missing.len(),
    );
    assert!(missing.iter().all(|p| p.chars().count() == 20));
}

// -- don't-care rows ------------------------------------------------------

/// A `-` stands for both values of its input, so one row can close what
/// would otherwise be two.
#[test]
fn a_dont_care_row_stands_for_the_rows_it_replaces() {
    assert!(codes(&table("sig.a, sig.b", "0- -> 0; 1- -> 1")).is_empty());
    assert!(codes(&table("sig.a, sig.b, sig.c", "--0 -> 0; --1 -> 1")).is_empty());
}

/// And a row of nothing but don't-cares closes the table on its own.
///
/// The case the coverage walk must never be handed: a pattern that fixes
/// nothing assigns every combination, so there is no lowest missing one
/// to step towards.
#[test]
fn a_row_of_only_dont_cares_completes_the_table() {
    assert!(codes(&table("sig.a, sig.b, sig.c", "--- -> 1")).is_empty());
}

/// Coverage counts combinations rather than rows, so a partial table with
/// a don't-care reports what the row actually assigns.
#[test]
fn a_dont_care_row_is_counted_as_the_combinations_it_assigns() {
    let found = only(&table("sig.a, sig.b, sig.c", "0-- -> 0"));
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_PARTIAL");
    assert!(
        rendered(&found).contains("assigns 4 of the 8"),
        "one row with two don't-cares assigns four combinations: {}",
        rendered(&found),
    );
}

/// Two rows may not both assign one combination, and a `-` is what makes
/// that possible without writing the combination out.
///
/// The conflict names the combination the two share rather than either
/// pattern: `0-` and `-1` are different strings, and quoting one of them
/// would send the author looking for a row that disagrees with itself.
#[test]
fn two_rows_that_cross_and_disagree_are_a_conflict() {
    let found = only(&table("sig.a, sig.b", "0- -> 0; -1 -> 1"));
    assert_eq!(found.code.as_str(), "E_TRUTH_TABLE_CONFLICT");
    assert_eq!(found.severity(), Severity::Error);
    let text = rendered(&found);
    assert!(
        text.contains("`01`"),
        "the conflict should name the combination both rows assign: {text}",
    );
}

/// A row inside an earlier one asserts nothing the earlier one does not,
/// and is told so in those words — the repair is to delete this row,
/// which is not what the reader of a bare "repeats an earlier one" would
/// go looking for.
#[test]
fn a_row_inside_an_earlier_one_asserts_nothing_new() {
    let found = only(&table("sig.a, sig.b", "0- -> 0; 01 -> 0; 10 -> 0; 11 -> 0"));
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_DUPLICATE_ROW");
    let text = rendered(&found);
    assert!(
        text.contains("asserts nothing new") && text.contains("`0-`"),
        "the finding should name the row that already covers it: {text}",
    );
}

/// An exact repeat keeps the sentence it has always had, because the
/// repair has not changed: delete either line.
#[test]
fn an_exact_repeat_still_reads_as_a_repeat() {
    let found = only(&complete_plus("00->0; 00->0"));
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_DUPLICATE_ROW");
    assert!(
        rendered(&found).contains("repeats an earlier one"),
        "an exact repeat should not be reworded: {}",
        rendered(&found),
    );
}

/// Two rows that merely cross are reported even when they agree.
///
/// Nothing orders the rows, so there is no reading under which the second
/// wins and none under which the pair is shorthand for anything. The fix
/// says to narrow one rather than to delete one, because deleting either
/// would lose the combinations only it assigns.
#[test]
fn two_rows_that_cross_are_reported_even_when_they_agree() {
    let found = only(&table("sig.a, sig.b", "0- -> 0; -1 -> 0"));
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_DUPLICATE_ROW");
    let text = rendered(&found);
    assert!(
        text.contains("`01`") && text.contains("narrow one of the two"),
        "a crossing pair should be told to narrow rather than to delete: {text}",
    );
}

/// And a crossing pair silences the coverage finding rather than
/// answering it with a count that is wrong.
///
/// `0-` and `-1` assign `00`, `01` and `11` between them, so the only
/// combination missing is `10`. Counting the accepted rows alone would
/// report two of four and name `11` — which the source assigns on the
/// line above. The assertion is that exactly one finding comes back, and
/// that it is the overlap.
#[test]
fn a_crossing_pair_does_not_also_earn_a_coverage_count() {
    let found = only(&table("sig.a, sig.b", "0- -> 0; -1 -> 0"));
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_DUPLICATE_ROW");
}

/// A row that is dropped for sitting inside another still leaves the
/// count right, so the coverage finding stays.
#[test]
fn a_row_inside_another_still_leaves_the_coverage_finding() {
    let found = codes(&table("sig.a, sig.b", "0- -> 0; 01 -> 0"));
    // Ordered by where each one points: the coverage finding underlines
    // the whole `assert truth`, which opens before the row the repeat is
    // reported on.
    assert_eq!(
        found,
        vec!["W_TRUTH_TABLE_PARTIAL", "W_TRUTH_TABLE_DUPLICATE_ROW"],
    );
}

/// The walk for a missing combination steps over a row rather than
/// through it.
///
/// Nineteen don't-cares stand for half a million combinations. A walk
/// that visited them one at a time would still return the right answer,
/// which is why the assertion is on the answer *and* on the finding being
/// the only one: the sample has to be the four lowest combinations the
/// row does not assign, and those all sit above every combination it
/// does.
#[test]
fn the_walk_steps_over_a_row_rather_than_through_it() {
    let names: Vec<String> = (0..20).map(|i| format!("sig.a{i}")).collect();
    let pattern = format!("0{}", "-".repeat(19));
    let found = only(&table(&names.join(", "), &format!("{pattern} -> 1")));
    let Some(DiagnosticData::TruthTablePartial {
        inputs,
        covered,
        missing,
    }) = found.data.clone()
    else {
        panic!("the partial finding should carry its payload, got {found:?}");
    };
    assert_eq!((inputs, covered), (20, 1 << 19));
    assert_eq!(
        missing,
        vec![
            format!("1{}", "0".repeat(19)),
            format!("1{}1", "0".repeat(18)),
            format!("1{}10", "0".repeat(17)),
            format!("1{}11", "0".repeat(17)),
        ],
    );
}

// -- don't-care outputs ---------------------------------------------------

/// The shape this construct exists for: a table whose author means to
/// constrain half the combinations and to say nothing about the other
/// half.
///
/// Written out of the issue that asked for it — three inputs, one of them
/// an enable, and every combination with the enable low deliberately
/// unconstrained. Without the `-` output the four enabled rows raise
/// `W_TRUTH_TABLE_PARTIAL`, and the only ways to answer it were to assert
/// four outputs the author does not mean or to leave the warning standing.
#[test]
fn a_dash_output_answers_the_coverage_finding() {
    let enabled = "001 -> 0; 011 -> 1; 101 -> 1; 111 -> 1";
    assert_eq!(
        codes(&table("sig.a, sig.b, sig.enable", enabled)),
        vec!["W_TRUTH_TABLE_PARTIAL"],
    );
    assert!(
        codes(&table(
            "sig.a, sig.b, sig.enable",
            &format!("{enabled}; --0 -> -"),
        ))
        .is_empty(),
    );
}

/// A `-` output covers its combinations without asserting anything about
/// them, so it moves the coverage count and nothing else.
#[test]
fn a_dash_output_counts_toward_coverage_like_any_other_row() {
    let without = only(&table("sig.a, sig.b, sig.c", "00- -> 0"));
    let with = only(&table("sig.a, sig.b, sig.c", "00- -> 0; 01- -> -"));
    assert_eq!(found_covers(&without), "assigns 2 of the 8");
    assert_eq!(found_covers(&with), "assigns 4 of the 8");
}

/// A table of nothing but `-` outputs is the empty table written at
/// length, and earns the empty table's error rather than a coverage
/// warning it would pass.
#[test]
fn a_table_of_only_dash_outputs_verifies_nothing() {
    let found = only(&table("sig.a, sig.b", "0- -> -; 1- -> -"));
    assert_eq!(found.code.as_str(), "E_TRUTH_TABLE_EMPTY");
    assert!(
        rendered(&found).contains("every row")
            && rendered(&found).contains("give at least one row a `0` or `1` output"),
        "the sentence has to send the author to the rows, not ask for more of them: {}",
        rendered(&found),
    );
}

/// The table above is complete by the coverage arithmetic, which is
/// exactly why the finding cannot be left to it.
#[test]
fn a_complete_table_of_dash_outputs_is_still_refused() {
    assert_eq!(
        codes(&table("sig.a, sig.b", "-- -> -")),
        vec!["E_TRUTH_TABLE_EMPTY"],
    );
}

/// A `-` asserts nothing for a concrete output to contradict, so the pair
/// is not a circuit asked for two things — but it is not two rows saying
/// the same thing either, and the finding has to say which.
#[test]
fn a_dash_output_contradicts_nothing_and_agrees_with_nothing() {
    let source = complete_plus("00 -> 0; 00 -> -");
    let found = only(&source);
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_DUPLICATE_ROW");
    assert!(
        rendered(&found).contains("this row leaves `00` unconstrained")
            && rendered(&found).contains("assigns it the output `0`"),
        "the sentence has to say which row does which: {}",
        rendered(&found),
    );
    assert_eq!(underlined(&source, &found.span), "00 -> -");
}

/// And the same pair the other way round, which is the reading the fix
/// sentence turns on: whichever row stays, the table says something
/// different, so "delete either row" would be wrong here.
#[test]
fn a_row_under_an_earlier_dash_output_names_the_earlier_row_as_the_lenient_one() {
    let source = table("sig.a, sig.b", "0- -> -; 01 -> 1; 10 -> 0; 11 -> 0");
    let found = only(&source);
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_DUPLICATE_ROW");
    assert!(
        rendered(&found).contains("this row assigns `01` the output `1`")
            && rendered(&found).contains("the earlier row `0-` leaves it unconstrained"),
        "the earlier row's `-` output is what the author has to see: {}",
        rendered(&found),
    );
    assert!(
        rendered(&found).contains("delete whichever of the two you did not mean")
            && !rendered(&found).contains("delete either row"),
        "deleting either row changes the table, so the fix may not offer both: {}",
        rendered(&found),
    );
    assert_eq!(underlined(&source, &found.span), "01 -> 1");
}

/// Two `-` rows over one combination *do* say the same thing, so that
/// pair keeps the duplicate-row family's own repair.
#[test]
fn two_dash_outputs_over_one_combination_are_an_ordinary_repeat() {
    let source = table("sig.a, sig.b", "0- -> -; 01 -> -; 10 -> 0; 11 -> 0");
    let found = only(&source);
    assert_eq!(found.code.as_str(), "W_TRUTH_TABLE_DUPLICATE_ROW");
    assert!(
        rendered(&found).contains("already assigns `01` no output, by its `-`")
            && rendered(&found).contains("delete this row"),
        "two rows that both decline are a line to delete: {}",
        rendered(&found),
    );
}

/// A `-` output does not launder a real contradiction sitting beside it.
#[test]
fn a_dash_output_elsewhere_leaves_a_conflict_a_conflict() {
    assert_eq!(
        codes(&table("sig.a, sig.b", "0- -> -; 10 -> 0; 10 -> 1; 11 -> 0")),
        vec!["E_TRUTH_TABLE_CONFLICT"],
    );
}
