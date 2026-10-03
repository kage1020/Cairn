//! Parse refusals point at the text that is wrong and name the mistake
//! that was made.
//!
//! `spec/syntax` "Literals and separators": "A position always points at
//! the text that is wrong". Each source below was already refused; what
//! these pin is the position and the message, which is the whole of what
//! an author gets to repair the file with.

use cairn_lang_core::parse::parse;

mod common;
use common::codes;

/// The refusal for `source`, as `line:col: message`.
fn refusal(source: &str) -> String {
    let error = parse(source).expect_err("the source should be refused");
    format!("{}: {}", error.position(), error.user_message())
}

// -- truth tables ---------------------------------------------------------

/// A bad output is reported at the output, not at the `}` or the next row
/// the parser met after taking it.
#[test]
fn a_bad_truth_output_is_reported_at_the_output() {
    assert_eq!(
        refusal("struct s size=3x3\n  assert truth(a -> b) { 0 -> 7 }\n"),
        "2:31: truth-table output must be `0`, `1`, or `-`, got `7`",
    );
    assert_eq!(
        refusal("struct s size=3x3\n  assert truth(a -> b) { 0 -> 10 1 -> 0 }\n"),
        "2:31: truth-table output must be `0`, `1`, or `-`, got `10`",
    );
    // With no `}` the next token is the end of the line.
    assert_eq!(
        refusal("struct s size=3x3\n  assert truth(a -> b) { 0 -> 7\n"),
        "2:31: truth-table output must be `0`, `1`, or `-`, got `7`",
    );
}

/// A misspelled form is wrong in the word after `assert`.
#[test]
fn a_misspelled_assert_form_is_reported_at_the_form() {
    assert_eq!(
        refusal("struct s size=3x3\n  assert truht(a -> b) { 0 -> 1 }\n"),
        "2:10: expected `truth` or `always` after `assert`, got `truht`",
    );
}

// -- directives -----------------------------------------------------------

/// An unknown directive is refused by its name whether or not a value
/// follows it; "requires a value" is only for the directives that exist.
#[test]
fn an_unknown_directive_is_named_whether_or_not_it_has_a_value() {
    assert_eq!(
        refusal("@crain\nstruct s size=3x3\n"),
        "1:1: unknown directive `@crain`",
    );
    assert_eq!(
        refusal("@crain 2026.6\nstruct s size=3x3\n"),
        "1:1: unknown directive `@crain`",
    );
    assert_eq!(refusal("@cairn\n"), "1:7: @cairn requires a value");
}

/// `@intended_targets` quotes the offending value as written and points at
/// it, on whatever line the directive stands.
#[test]
fn an_intended_targets_refusal_quotes_and_points_at_the_offending_value() {
    for (source, expected) in [
        (
            "@intended_targets [\"1.21\", [x]]\n",
            "1:28: @intended_targets expects a string such as `\"1.21.4\"`, got `[x]`",
        ),
        (
            "@intended_targets \"1.21\"\n",
            "1:19: @intended_targets expects a list of strings such as `[\"1.21.4\"]`, got \
             `\"1.21\"`",
        ),
        (
            "@intended_targets [oak]\n",
            "1:20: @intended_targets expects a string such as `\"1.21.4\"`, got `oak`",
        ),
        (
            "@intended_targets 1\n",
            "1:19: @intended_targets expects a list of strings such as `[\"1.21.4\"]`, got `1`",
        ),
        (
            "@cairn 2026.06\n@intended_targets  [\"1.21\",   1]\n",
            "2:31: @intended_targets expects a string such as `\"1.21.4\"`, got `1`",
        ),
    ] {
        assert_eq!(refusal(source), expected, "{source:?}");
    }
}

/// Trailing text after the value is refused at that text, as the two
/// refusals above point at their element, however the line is spaced.
#[test]
fn an_intended_targets_trailing_token_is_reported_at_the_token() {
    for (source, expected) in [
        ("@intended_targets [\"1.21\"] junk\n", "1:28"),
        ("@intended_targets      [\"1.21\"]        junk\n", "1:40"),
        (
            "@cairn 2026.06\n@intended_targets [\"1.21\"] junk\n",
            "2:28",
        ),
    ] {
        assert_eq!(
            refusal(source),
            format!("{expected}: @intended_targets has trailing tokens after the list"),
            "{source:?}",
        );
    }
}

// -- `requires` where no floor may stand ------------------------------------

/// A member's children under a `struct` or a `site` get the `@requires`
/// repair: a dedent would land the line in a body that refuses it too.
#[test]
fn a_floor_under_a_member_of_the_build_is_sent_to_the_file_directive() {
    for source in [
        "struct s size=3x3\n  level y=0\n    requires version>=1.21\n",
        "site p:\n  place use=d\n    requires version>=1.21\n",
    ] {
        let message = refusal(source);
        assert!(
            message.starts_with("3:5: a member may not declare"),
            "{message}"
        );
        assert!(
            message.contains("written `@requires version>=X` at the top of the file"),
            "{source:?}: {message}",
        );
        assert!(!message.contains("`def`"), "{source:?}: {message}");
    }
}

/// Under a `def` the floor's level is one dedent away.
#[test]
fn a_floor_under_a_member_of_a_part_is_sent_one_level_up() {
    let message = refusal("def d size=3x3\n  level y=0\n    requires version>=1.21\n");
    assert!(
        message.starts_with("3:5: a member may not declare"),
        "{message}"
    );
    assert!(message.contains("the `def` body's own level"), "{message}");
    assert!(!message.contains("@requires"), "{message}");
}

/// A member's children hand their own children the same repair, however
/// deep: a grandchild of a member is in the same body as the member, so
/// the body decides, not the depth.
#[test]
fn a_floor_two_members_deep_gets_its_bodys_repair() {
    for (source, to_the_file) in [
        (
            "struct s size=3x3\n  level y=0\n    walls x=1\n      requires version>=1.21\n",
            true,
        ),
        (
            "site p:\n  place use=d\n    walls x=1\n      requires version>=1.21\n",
            true,
        ),
        (
            "def d size=3x3\n  level y=0\n    walls x=1\n      requires version>=1.21\n",
            false,
        ),
    ] {
        let message = refusal(source);
        assert!(
            message.starts_with("4:7: a member may not declare"),
            "{source:?}: {message}",
        );
        assert_eq!(
            message.contains("written `@requires version>=X` at the top of the file"),
            to_the_file,
            "{source:?}: {message}",
        );
        assert_eq!(
            message.contains("the `def` body's own level"),
            !to_the_file,
            "{source:?}: {message}",
        );
    }
}

/// A floor with an indented line under it is still recognised as a floor
/// where it stands, not read as a member whose `>=` is then unexpected.
#[test]
fn a_floor_with_an_indented_line_under_it_is_refused_as_a_floor() {
    let message = refusal("struct s size=3x3\n  requires version>=1.21\n    floor a=1\n");
    assert!(
        message.starts_with("2:3: a `struct` or a `site` may not declare"),
        "{message}",
    );
    let message = refusal("def d size=3x3\n  level y=0\n    requires version>=1.21\n      x a=1\n");
    assert!(
        message.starts_with("3:5: a member may not declare"),
        "{message}"
    );
}

/// A line that states no floor is still a member where no floor may stand,
/// indented body and all.
#[test]
fn a_requires_member_with_a_body_still_parses_where_no_floor_may_stand() {
    parse("struct s size=3x3\n  requires a=1\n    floor b=1\n").expect("a member line");
}

// -- size literals ----------------------------------------------------------

/// `9x` is an integer and an `x`. In the forms where every argument needs
/// its `=` — an item header, a `[…]` list, a `theme` binding — the `x`
/// would be a key with no `=`, and the refusal names the literal rather
/// than the `=` such a key would need. Each form reaches the check, so
/// moving it into one of them cannot pass.
#[test]
fn a_size_with_no_height_is_named_as_one() {
    for (source, at) in [
        ("struct s size=9x\n", "1:15"),
        ("struct s size=9x 7\n", "1:15"),
        ("def d size=9x\n", "1:12"),
        ("struct s size=3x3\n  floor[size=9x] mat=f\n", "2:14"),
        ("theme t:\n  block[a=1] -> size=9x\n", "2:22"),
    ] {
        assert_eq!(
            refusal(source),
            format!("{at}: size literal `9x` has no height; a size is two extents, as in `9x7`"),
            "{source:?}",
        );
    }
}

/// The refusal is keyed off the glued `x`, not off `size=`: digits-`x`
/// read as nothing but a size, under any key.
#[test]
fn a_size_with_no_height_is_named_under_any_key() {
    assert_eq!(
        refusal("struct s a=9x\n"),
        "1:12: size literal `9x` has no height; a size is two extents, as in `9x7`",
    );
}

/// An `x` that has its `=` is an argument of its own, so `size=9x=2` is a
/// `size=9` and an `x=2`, judged by the checks as before. `codes` panics
/// if the source does not parse.
#[test]
fn a_glued_x_with_its_own_value_is_an_argument() {
    let found = codes("struct s size=9x=2\n");
    assert!(found.contains(&"E_TYPE_MISMATCH_SIZE"), "{found:?}");
}

/// An `x` that is not glued to a value before it is an ordinary key, as
/// is any other word glued to one, and a key with no `=` gets the ordinary
/// refusal.
#[test]
fn an_x_key_apart_from_a_size_is_an_ordinary_key() {
    assert_eq!(
        refusal("struct s size=9 x\n"),
        "1:18: expected `=`, got end of line",
    );
    assert_eq!(
        refusal("struct s size=9y\n"),
        "1:17: expected `=`, got end of line",
    );
}

// -- logic expressions ------------------------------------------------------

/// `and` and `or` are operators, never a signal's name, so a missing
/// operand is refused where it is missing.
#[test]
fn an_operator_is_not_read_as_a_signal() {
    for (expression, expected) in [
        (
            "sig.a and or",
            "2:27: expected a signal after `and`, got the operator `or`",
        ),
        (
            "and",
            "2:17: expected a signal after `=`, got the operator `and`",
        ),
        (
            "sig.a or and and sig.b",
            "2:26: expected a signal after `or`, got the operator `and`",
        ),
        (
            "not or",
            "2:21: expected a signal after `not`, got the operator `or`",
        ),
        (
            "(or)",
            "2:18: expected a signal after `(`, got the operator `or`",
        ),
        (
            "sig.a and or.x",
            "2:27: expected a signal after `and`, got the operator `or`",
        ),
    ] {
        let source = format!("struct s size=3x3\n  logic sig.x = {expression}\n");
        assert_eq!(refusal(&source), expected, "{expression:?}");
    }
}

/// A dotted name may still end in an operator word: only a signal's head
/// stands where an operator could.
#[test]
fn an_operator_word_may_end_a_dotted_name() {
    parse("struct s size=3x3\n  logic sig.x = sig.a and c.or\n").expect("a dotted name");
}
