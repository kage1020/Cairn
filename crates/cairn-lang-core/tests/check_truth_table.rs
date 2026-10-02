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

/// Each repeat that agrees is judged against the *first* row carrying its
/// pattern, so every repeat of a pattern sends the author to the same
/// place to look.
#[test]
fn a_repeat_is_judged_against_the_first_row_with_its_pattern() {
    let source = complete_plus("00->0; 00->0; 00->0");
    let found = diagnose(&source);
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["W_TRUTH_TABLE_DUPLICATE_ROW", "W_TRUTH_TABLE_DUPLICATE_ROW"],
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

/// A row that flips an assignment back agrees with the first row and
/// contradicts the one it flips back from — and a contradiction with any
/// earlier row is a conflict, whichever row the table opened with. The
/// note goes to the row it contradicts, since that is the pair the
/// author has to decide between.
#[test]
fn a_row_that_flips_back_contradicts_the_row_it_flips_back_from() {
    let source = complete_plus("00->0; 00->1; 00->0");
    let found = diagnose(&source);
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E_TRUTH_TABLE_CONFLICT", "E_TRUTH_TABLE_CONFLICT"],
    );
    let first = source.find("00->0").expect("the first row");
    assert_eq!(
        found[0].notes[0].span.as_ref().expect("a note span").start,
        first,
        "the flip contradicts the first row",
    );
    let flipped = source.find("00->1").expect("the second row");
    assert_eq!(
        found[1].notes[0].span.as_ref().expect("a note span").start,
        flipped,
        "the flip back contradicts the second row, not the first",
    );
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

/// A broad `-` row written first does not make the table empty.
///
/// The shape every other `-`-output fixture here misses, because each
/// pads its table with concrete rows the overlap scan keeps. Acceptance
/// is first-come, so the row dropped for overlapping is the *later*
/// one — write `--` first and the concrete row after it is the one that
/// goes, leaving nothing but `-` outputs among the survivors while the
/// author is looking at a `1` in the source.
///
/// `-` first with the exceptions after is the ordinary way to write a
/// default-plus-exception table, so this is a habit rather than a
/// contrivance.
#[test]
fn a_broad_dash_row_written_first_does_not_empty_the_table() {
    assert_eq!(
        codes(&table("sig.a, sig.b", "-- -> -; 01 -> 1")),
        vec!["W_TRUTH_TABLE_DUPLICATE_ROW"],
        "the overlap is the finding; the table plainly has a `1` in it",
    );
}

/// And the coverage count stays right through it.
///
/// `0-` covers `00` and `01` and nothing else, so `10` and `11` really
/// are unassigned — the overlap that drops `01` is subsumed, which is
/// what keeps the count exact.
#[test]
fn a_broad_dash_row_written_first_still_earns_its_coverage_finding() {
    let found = diagnose(&table("sig.a, sig.b", "0- -> -; 01 -> 1"));
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        vec!["W_TRUTH_TABLE_PARTIAL", "W_TRUTH_TABLE_DUPLICATE_ROW"],
    );
    assert!(
        rendered(&found[0]).contains("assigns 2 of the 4")
            && rendered(&found[0]).contains("`10`")
            && rendered(&found[0]).contains("`11`"),
        "the two the `0-` row does not cover: {}",
        rendered(&found[0]),
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

// -- row order ------------------------------------------------------------

/// Every order of `rows`, by Heap's algorithm.
fn permutations<'a>(rows: &[&'a str]) -> Vec<Vec<&'a str>> {
    fn heap<'a>(k: usize, rows: &mut Vec<&'a str>, out: &mut Vec<Vec<&'a str>>) {
        if k <= 1 {
            out.push(rows.clone());
            return;
        }
        for i in 0..k {
            heap(k - 1, rows, out);
            let j = if k.is_multiple_of(2) { i } else { 0 };
            rows.swap(j, k - 1);
        }
    }
    let mut rows = rows.to_vec();
    let mut out = Vec::new();
    heap(rows.len(), &mut rows, &mut out);
    out
}

/// A row as the enumeration reads it: its pattern and its output digit.
fn parsed(row: &str) -> (String, char) {
    let (pattern, output) = row.split_once(" -> ").expect("rows are written `p -> o`");
    (
        pattern.to_owned(),
        output.chars().next().expect("an output digit"),
    )
}

/// The codes reported on each row of `source`, in row order, found by
/// where each row's text starts. `written` is the rows as they appear in
/// the source, in order.
fn per_row_codes(source: &str, written: &[String], found: &[Diagnostic]) -> Vec<Vec<&'static str>> {
    let mut offset = source.find('{').expect("the table opens");
    written
        .iter()
        .map(|text| {
            let start = offset
                + source[offset..]
                    .find(text.as_str())
                    .expect("each row is written");
            offset = start + text.len();
            found
                .iter()
                .filter(|d| d.span.start == start)
                .map(|d| d.code.as_str())
                .collect()
        })
        .collect()
}

/// Every order of `rows` reports, row by row, what the enumeration says
/// that order should — not only that some conflict survives somewhere.
fn assert_every_order_matches_the_enumeration(rows: &[&str], orders_expected: usize) {
    let orders = permutations(rows);
    assert_eq!(
        orders
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        orders_expected,
        "every order of the rows, each once",
    );
    for order in orders {
        let written: Vec<String> = order.iter().map(|r| (*r).to_owned()).collect();
        let source = table("sig.a, sig.b", &written.join("; "));
        let found = diagnose(&source);
        let expected: Vec<Vec<&str>> =
            expected_verdicts(&order.iter().map(|r| parsed(r)).collect::<Vec<_>>())
                .into_iter()
                .map(|verdict| verdict.into_iter().collect())
                .collect();
        assert_eq!(
            per_row_codes(&source, &written, &found),
            expected,
            "{source}"
        );
        assert!(
            found
                .iter()
                .any(|d| d.code.as_str() == "E_TRUTH_TABLE_CONFLICT"),
            "the table is refused in every order: {source}",
        );
    }
}

/// Whether a table is refused cannot depend on the order its rows are
/// written in.
///
/// `0- -> 1` agrees with `00 -> 1` and contradicts `01 -> 0`. Compared
/// with only the first earlier row it overlaps, it is refused when that
/// row is `01` and passes when it is `00`.
#[test]
fn a_row_overlapping_several_earlier_rows_is_compared_with_each() {
    assert_every_order_matches_the_enumeration(
        &["00 -> 1", "01 -> 0", "0- -> 1", "10 -> 0", "11 -> 0"],
        120,
    );
}

/// A row dropped for overlapping is still a row the author wrote, and a
/// later row contradicting it is a conflict.
///
/// `-0` is dropped for crossing `0-`, and `10` meets nothing else: it
/// overlaps `-0` and nothing the table kept.
#[test]
fn a_row_contradicting_a_dropped_row_is_a_conflict() {
    assert_every_order_matches_the_enumeration(&["0- -> 1", "-0 -> 1", "10 -> 0", "11 -> 0"], 24);
}

/// And the conflict is noted at that dropped row, with the output it
/// assigns — the path the note's wording argues about, since the row the
/// later one contradicts is not one the table kept.
#[test]
fn a_conflict_with_a_dropped_row_is_noted_at_the_dropped_row() {
    let source = table("sig.a, sig.b", "0- -> 1; -0 -> 1; 10 -> 0; 11 -> 0");
    let found = diagnose(&source);
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["W_TRUTH_TABLE_DUPLICATE_ROW", "E_TRUTH_TABLE_CONFLICT"],
    );
    assert_eq!(underlined(&source, &found[1].span), "10 -> 0");
    let note = &found[1].notes[0];
    assert_eq!(
        underlined(
            &source,
            note.span.as_ref().expect("the note carries a span")
        ),
        "-0 -> 1",
    );
    assert_eq!(note.message, "first row assigning `10` the output `1` here");
}

/// The conflict names the row it contradicts, and the output that row
/// assigns, rather than the first row the later one happens to overlap.
#[test]
fn a_conflict_is_noted_at_the_row_it_contradicts() {
    let source = table(
        "sig.a, sig.b",
        "00 -> 1; 01 -> 0; 0- -> 1; 10 -> 0; 11 -> 0",
    );
    let found = only(&source);
    assert_eq!(found.code.as_str(), "E_TRUTH_TABLE_CONFLICT");
    assert_eq!(underlined(&source, &found.span), "0- -> 1");
    let note = &found.notes[0];
    assert_eq!(
        underlined(
            &source,
            note.span.as_ref().expect("the note carries a span")
        ),
        "01 -> 0",
    );
    assert_eq!(note.message, "first row assigning `01` the output `0` here");
    assert!(
        found.primary.contains("assigns `01` the output `1`"),
        "the sentence names the combination the two disagree on: {}",
        found.primary,
    );
}

/// A row contradicting several earlier rows is noted at the first of
/// them, which is the first row assigning the combination the other
/// output: `00 -> 0` contradicts both `0-` and `-0`.
#[test]
fn a_conflict_with_several_rows_is_noted_at_the_first() {
    let source = table("sig.a, sig.b", "0- -> 1; -0 -> 1; 00 -> 0; 11 -> 0");
    let found = diagnose(&source);
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["W_TRUTH_TABLE_DUPLICATE_ROW", "E_TRUTH_TABLE_CONFLICT"],
    );
    assert_eq!(underlined(&source, &found[1].span), "00 -> 0");
    assert_eq!(
        underlined(
            &source,
            found[1].notes[0].span.as_ref().expect("a note span")
        ),
        "0- -> 1",
    );
}

/// A `-` output written first does not stand between two rows that
/// contradict each other under it: both are compared with the `-` row,
/// which neither contradicts, and the second is compared with the first.
#[test]
fn a_dash_output_row_written_first_does_not_hide_a_conflict_under_it() {
    let source = table("sig.a, sig.b", "0- -> -; 00 -> 0; 00 -> 1; 1- -> 0");
    let found = diagnose(&source);
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["W_TRUTH_TABLE_DUPLICATE_ROW", "E_TRUTH_TABLE_CONFLICT"],
    );
    let noted = |d: &Diagnostic| {
        underlined(
            &source,
            d.notes[0].span.as_ref().expect("the note carries a span"),
        )
    };
    assert_eq!(underlined(&source, &found[0].span), "00 -> 0");
    assert_eq!(noted(&found[0]), "0- -> -");
    assert_eq!(underlined(&source, &found[1].span), "00 -> 1");
    assert_eq!(noted(&found[1]), "00 -> 0");
}

/// Every finding about one combination sends the author to the same row,
/// and that row is the first to assign it — even when that row was
/// dropped for overlapping a row before it.
///
/// `0-` crosses `00` and is dropped; both `01` rows fall inside it, and
/// both are noted there rather than the second at the first `01`.
#[test]
fn every_finding_about_a_combination_is_noted_at_the_first_row_assigning_it() {
    let source = table(
        "sig.a, sig.b",
        "00 -> 1; 0- -> 1; 01 -> 1; 01 -> 1; 1- -> 0",
    );
    let found = diagnose(&source);
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        [
            "W_TRUTH_TABLE_DUPLICATE_ROW",
            "W_TRUTH_TABLE_DUPLICATE_ROW",
            "W_TRUTH_TABLE_DUPLICATE_ROW"
        ],
    );
    let dash = source.find("0- -> 1").expect("the `0-` row");
    for d in &found[1..] {
        assert_eq!(
            d.notes[0].span.as_ref().expect("a note span").start,
            dash,
            "`0-` is the first row assigning `01`: {}",
            rendered(d),
        );
        assert_eq!(d.notes[0].message, "first row assigning `01` here");
    }
}

/// The advice a row gets never leans on an earlier row that is itself
/// being asked to change.
///
/// `10` is inside `-0`, so on its own it would be told to delete itself
/// because `-0` stands for it. But `-0` is told to narrow away from
/// `0-`, and following both would leave `10` unassigned. The repair is
/// to settle `-0` first.
#[test]
fn a_row_inside_a_reported_row_is_told_to_settle_that_row_first() {
    let source = table("sig.a, sig.b", "0- -> 1; -0 -> 1; 10 -> 1; 11 -> 0");
    let found = diagnose(&source);
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["W_TRUTH_TABLE_DUPLICATE_ROW", "W_TRUTH_TABLE_DUPLICATE_ROW"],
    );
    assert_eq!(underlined(&source, &found[1].span), "10 -> 1");
    assert_eq!(
        underlined(
            &source,
            found[1].notes[0].span.as_ref().expect("a note span")
        ),
        "-0 -> 1",
    );
    let text = rendered(&found[1]);
    assert!(
        text.contains("settle the earlier row `-0` first") && !text.contains("delete"),
        "the fix may not tell the author to delete a row the noted one is not keeping: {text}",
    );
}

/// The same for a `-` output under a reported row, whose usual repair —
/// delete whichever of the two you did not mean — would also send the
/// author to a row that is separately being asked to narrow.
#[test]
fn a_dash_output_inside_a_reported_row_is_told_to_settle_that_row_first() {
    let source = table("sig.a, sig.b", "0- -> 1; -0 -> 1; 10 -> -");
    let found: Vec<Diagnostic> = diagnose(&source)
        .into_iter()
        .filter(|d| d.code.as_str() == "W_TRUTH_TABLE_DUPLICATE_ROW")
        .collect();
    assert_eq!(found.len(), 2, "one finding each for `-0` and `10`");
    assert_eq!(underlined(&source, &found[1].span), "10 -> -");
    let text = rendered(&found[1]);
    assert!(
        text.contains("this row leaves `10` unconstrained")
            && text.contains("settle the earlier row `-0` first")
            && !text.contains("delete"),
        "the unconstrained pair under a reported row settles that row first: {text}",
    );
}

/// A row can be accepted and still carry a finding, when every row it
/// overlaps was dropped — and that does not make the coverage count
/// speak.
///
/// `0-0` crosses `00-` and is dropped; `010` overlaps only `0-0`, so it is
/// accepted with a finding. The accepted rows cover 3 of 8, but the
/// crossing already withheld the count, and it stays withheld.
#[test]
fn a_row_accepted_with_a_finding_does_not_earn_a_coverage_count() {
    assert_eq!(
        codes(&table(
            "sig.a, sig.b, sig.c",
            "00- -> 1; 0-0 -> 1; 010 -> 1"
        )),
        ["W_TRUTH_TABLE_DUPLICATE_ROW", "W_TRUTH_TABLE_DUPLICATE_ROW"],
    );
}

/// The combinations a pattern stands for, spelled out.
fn combinations_of(pattern: &str) -> Vec<String> {
    pattern
        .chars()
        .fold(vec![String::new()], |prefixes, digit| {
            let choices: &[char] = match digit {
                '-' => &['0', '1'],
                '0' => &['0'],
                _ => &['1'],
            };
            prefixes
                .iter()
                .flat_map(|prefix| {
                    choices.iter().map(move |c| {
                        let mut next = prefix.clone();
                        next.push(*c);
                        next
                    })
                })
                .collect()
        })
}

/// Whether two patterns share a combination, by intersecting what each
/// spells out.
fn share_a_combination(a: &str, b: &str) -> bool {
    let theirs = combinations_of(b);
    combinations_of(a).iter().any(|c| theirs.contains(c))
}

/// Whether two outputs contradict: two concrete digits that differ.
fn outputs_contradict(a: char, b: char) -> bool {
    a != '-' && b != '-' && a != b
}

/// What each row should earn, worked out combination by combination
/// rather than pattern by pattern: a conflict when some combination it
/// assigns an earlier row assigns a different concrete output, otherwise
/// a duplicate when some combination it assigns an earlier row assigns
/// at all, otherwise nothing.
fn expected_verdicts(rows: &[(String, char)]) -> Vec<Option<&'static str>> {
    rows.iter()
        .enumerate()
        .map(|(index, (pattern, output))| {
            let mut verdict = None;
            for (earlier, earlier_output) in &rows[..index] {
                if !share_a_combination(pattern, earlier) {
                    continue;
                }
                if outputs_contradict(*output, *earlier_output) {
                    return Some("E_TRUTH_TABLE_CONFLICT");
                }
                verdict = Some("W_TRUTH_TABLE_DUPLICATE_ROW");
            }
            verdict
        })
        .collect()
}

/// How many rows of a table agree with the first earlier row they overlap
/// and contradict a later one, and how many are noted at a row that
/// itself overlaps a row before it — the shapes an order-dependent or
/// accepted-only comparison gets wrong.
fn interesting_shapes(rows: &[(String, char)]) -> (usize, usize) {
    let mut contradicts_past_its_first_overlap = 0;
    let mut noted_at_an_overlapping_row = 0;
    for (index, (pattern, output)) in rows.iter().enumerate() {
        let overlapping: Vec<usize> = (0..index)
            .filter(|at| share_a_combination(pattern, &rows[*at].0))
            .collect();
        let contradicting = overlapping
            .iter()
            .find(|at| outputs_contradict(*output, rows[**at].1));
        if let (Some(first), Some(_)) = (overlapping.first(), contradicting)
            && !outputs_contradict(*output, rows[*first].1)
        {
            contradicts_past_its_first_overlap += 1;
        }
        if let Some(noted) = contradicting.or(overlapping.first())
            && (0..*noted).any(|at| share_a_combination(&rows[*noted].0, &rows[at].0))
        {
            noted_at_an_overlapping_row += 1;
        }
    }
    (
        contradicts_past_its_first_overlap,
        noted_at_an_overlapping_row,
    )
}

/// Checks the findings that are about the table rather than a row against
/// the enumeration, and returns how many coverage findings there were.
fn assert_table_findings_are_sound(
    source: &str,
    rows: &[(String, char)],
    arity: usize,
    found: &[Diagnostic],
    row_findings: usize,
) -> usize {
    let mut partial_findings = 0;
    let table_findings: Vec<&Diagnostic> = found
        .iter()
        .filter(|d| {
            matches!(
                d.code.as_str(),
                "E_TRUTH_TABLE_EMPTY" | "W_TRUTH_TABLE_PARTIAL"
            )
        })
        .collect();
    assert_eq!(
        row_findings + table_findings.len(),
        found.len(),
        "every finding is either on a row or about the table: {source}",
    );
    let assigned: std::collections::BTreeSet<String> = rows
        .iter()
        .flat_map(|(pattern, _)| combinations_of(pattern))
        .collect();
    let total = 1usize << arity;
    let all_dash = rows.iter().all(|(_, output)| *output == '-');
    for finding in table_findings {
        match &finding.data {
            None => {
                assert_eq!(finding.code.as_str(), "E_TRUTH_TABLE_EMPTY");
                assert!(all_dash, "empty only when no row asserts: {source}");
            }
            Some(DiagnosticData::TruthTablePartial {
                covered, missing, ..
            }) => {
                partial_findings += 1;
                assert!(!all_dash, "{source}");
                assert!(
                    assigned.len() < total,
                    "a complete table is not partial: {source}"
                );
                assert_eq!(
                    usize::try_from(*covered).expect("small"),
                    assigned.len(),
                    "the coverage count is the true count: {source}",
                );
                for combination in missing {
                    assert!(
                        !assigned.contains(combination),
                        "`{combination}` is named missing but a row assigns it: {source}",
                    );
                }
            }
            Some(other) => panic!("unexpected payload {other:?}: {source}"),
        }
    }
    if all_dash {
        assert!(
            found
                .iter()
                .any(|d| d.code.as_str() == "E_TRUTH_TABLE_EMPTY"),
            "a table of `-` outputs verifies nothing: {source}",
        );
    }
    partial_findings
}

/// The overlap verdicts, compared with a combination-by-combination
/// enumeration over tables of one to four inputs, and the table-level
/// findings checked against the same enumeration.
///
/// The tables are drawn from a fixed-seed generator, so a failure replays.
/// Every row gets a verdict: which finding, if any, is reported on it.
/// Every other finding is about the table, and has to be one the
/// enumeration allows: the empty-table error exactly when no row has a
/// concrete output, and a coverage warning only with the true count and
/// only naming combinations no row assigns.
#[test]
fn every_row_verdict_matches_an_enumeration_of_its_combinations() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |bound: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % bound
    };
    let digit = |n: u64| ['0', '1', '-'][usize::try_from(n).expect("below three")];
    // The shapes this check exists for, counted so the generator cannot
    // drift away from them while still producing ordinary conflicts: a
    // row that agrees with the first earlier row it overlaps and
    // contradicts a later one, and a row whose noted row itself overlaps
    // a row before it. The seed reaches 709 and 1366 of them, and 1021
    // coverage findings; the floors below are about half of each.
    let mut contradicts_past_its_first_overlap = 0;
    let mut noted_at_an_overlapping_row = 0;
    let mut partial_findings = 0;
    for _ in 0..4000 {
        let arity = usize::try_from(next(4) + 1).expect("small");
        let count = next(6) + 1;
        let rows: Vec<(String, char)> = (0..count)
            .map(|_| {
                let pattern: String = (0..arity).map(|_| digit(next(3))).collect();
                (pattern, digit(next(3)))
            })
            .collect();
        let written: Vec<String> = rows.iter().map(|(p, o)| format!("{p} -> {o}")).collect();
        let inputs: Vec<String> = (0..arity).map(|i| format!("sig.i{i}")).collect();
        let source = table(&inputs.join(", "), &written.join("; "));
        let found = diagnose(&source);

        let expected = expected_verdicts(&rows);
        let actual = per_row_codes(&source, &written, &found);
        for ((verdict, codes), row) in expected.iter().zip(&actual).zip(&rows) {
            assert_eq!(
                codes,
                &verdict.iter().copied().collect::<Vec<_>>(),
                "row `{}` of {source}",
                row.0,
            );
        }

        let (past_first, at_overlapping) = interesting_shapes(&rows);
        contradicts_past_its_first_overlap += past_first;
        noted_at_an_overlapping_row += at_overlapping;
        let row_findings: usize = actual.iter().map(Vec::len).sum();
        partial_findings +=
            assert_table_findings_are_sound(&source, &rows, arity, &found, row_findings);
    }
    assert!(
        contradicts_past_its_first_overlap > 350 && noted_at_an_overlapping_row > 650,
        "the generator should reach the shapes this check is about: {contradicts_past_its_first_overlap} \
         rows contradicting past their first overlap, {noted_at_an_overlapping_row} noted at a row \
         that itself overlaps",
    );
    assert!(
        partial_findings > 500,
        "and the coverage finding often enough to check it: {partial_findings}",
    );
}
