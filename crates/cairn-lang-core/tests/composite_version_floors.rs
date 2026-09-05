//! The minimum version of a composite is the max of its parts.
//!
//! `spec/versioning-editions.md` §10.4 gives a `def` and a `theme` a
//! `requires version>=X` line of their own, and says a composite's floor is
//! the max over its parts. A module-level `@requires` cannot say that: it
//! applies to the whole file rather than to the template, so a library of
//! `def`s had no way to carry its own requirements and every consumer
//! restated them.
//!
//! What "its parts" means is the subject of most of this file. A part the
//! build does not instantiate is not a part of it; a `theme` is a part of
//! every build that *binds* it, whether or not a member reads a slot from
//! it; and a floor that names an edition is inert in the other one, exactly
//! as the header form is.

use cairn_lang_core::ast::Item;
use cairn_lang_core::resolve::{
    FloorOrigin, VersionFloor, declared_version_floors, unscoped_version_floors,
};
use cairn_lang_core::{Edition, check, lower, parse};

/// `(version, origin)` for every floor a build of `edition` is held to.
fn floors(source: &str, edition: Edition) -> Vec<(String, FloorOrigin)> {
    render(declared_version_floors(
        &parse(source).expect("the fixtures all parse"),
        edition,
    ))
}

/// The same list for the edition-neutral `registry compatibility` row.
fn neutral_floors(source: &str) -> Vec<(String, FloorOrigin)> {
    render(unscoped_version_floors(
        &parse(source).expect("the fixtures all parse"),
    ))
}

fn render(floors: Vec<VersionFloor>) -> Vec<(String, FloorOrigin)> {
    floors
        .into_iter()
        .map(|floor| (floor.version, floor.origin))
        .collect()
}

/// Diagnostic codes `cairn check` reports for a source.
fn codes(source: &str) -> Vec<&'static str> {
    let module = parse(source).expect("the fixtures all parse");
    let ir = lower(&module);
    check(&module, &ir, None)
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

/// A theme, a def that declares a floor, and a site that places it.
const PLACED: &str = "\
theme medieval:
  slot wall -> @oak_planks

def cottage size=5x5:
  requires version>=1.21.4
  walls mat_slot=wall height=3

site hamlet:
  place id=a use=cottage theme=medieval at=origin
";

// -- what a build inherits ------------------------------------------------

/// The point of the feature: a floor written once on the template is a
/// floor on every site that uses it, and the fold names the template.
#[test]
fn a_place_inherits_the_floor_of_the_def_it_instantiates() {
    assert_eq!(
        floors(PLACED, Edition::Java),
        vec![("1.21.4".to_owned(), FloorOrigin::Def("cottage".to_owned()))],
    );
}

/// A module's floor is the max over its own `@requires` lines and every
/// part it instantiates — so both reach the caller that folds them, in
/// source order, with the header first.
#[test]
fn the_header_and_the_parts_are_folded_together() {
    let source = format!("@requires version>=1.20\n\n{PLACED}");
    assert_eq!(
        floors(&source, Edition::Java),
        vec![
            ("1.20".to_owned(), FloorOrigin::Module),
            ("1.21.4".to_owned(), FloorOrigin::Def("cottage".to_owned())),
        ],
    );
}

/// A `def` no `place` names builds nothing — it earns `W_UNUSED_DEF` — so
/// holding the build to its floor would refuse targets over a template the
/// author left in the file.
#[test]
fn a_def_nobody_places_contributes_no_floor() {
    let source = "\
def cottage size=5x5:
  requires version>=1.21.4
  walls mat_slot=wall height=3
";
    assert_eq!(floors(source, Edition::Java), vec![]);
    assert!(codes(source).contains(&"W_UNUSED_DEF"));
}

/// The decision §10.4 left open: a `theme` floor applies when the theme is
/// **bound**, not when a rule of it fires. Nothing in this module reads a
/// slot, and the floor still applies — binding a theme is the act of taking
/// on what it declares, and making the floor depend on which selectors
/// matched would let one source require 1.21 on Java and nothing on Bedrock
/// for a reason that is not about editions.
#[test]
fn a_bound_theme_imposes_its_floor_even_where_no_slot_is_read() {
    let source = "\
theme medieval:
  requires version>=1.21.4
  slot wall -> @oak_planks

def cottage size=5x5:
  floor mat_slot=unused_by_nobody

site hamlet:
  place id=a use=cottage theme=medieval at=origin
";
    assert_eq!(
        floors(source, Edition::Java),
        vec![(
            "1.21.4".to_owned(),
            FloorOrigin::Theme("medieval".to_owned()),
        )],
    );
}

/// The module-level auto-pick binds the sole theme of a file to every
/// `struct` and `def` scope in it, so a file with one theme and one struct
/// — no `site` at all — inherits that theme's floor.
#[test]
fn the_sole_theme_of_a_file_is_bound_without_a_place() {
    let source = "\
theme medieval:
  requires version>=1.21.4
  slot wall -> @oak_planks

struct keep size=5x5
  walls mat_slot=wall height=3
";
    assert_eq!(
        floors(source, Edition::Java),
        vec![(
            "1.21.4".to_owned(),
            FloorOrigin::Theme("medieval".to_owned()),
        )],
    );
}

/// The auto-pick binds to `struct` and `def` scopes, so a file of nothing
/// but themes has nothing for it to bind to and inherits nothing.
#[test]
fn a_module_with_no_scope_binds_no_theme() {
    let source = "\
theme medieval:
  requires version>=1.21.4
  slot wall -> @oak_planks
";
    assert_eq!(floors(source, Edition::Java), vec![]);
}

/// Two themes and only one bound: the auto-pick does not fire, and the
/// theme nothing names is not part of the build.
#[test]
fn a_theme_nothing_binds_contributes_no_floor() {
    let source = "\
theme medieval:
  slot wall -> @oak_planks

theme baroque:
  requires version>=1.21.4
  slot wall -> @quartz_block

def cottage size=5x5:
  walls mat_slot=wall height=3

site hamlet:
  place id=a use=cottage theme=medieval at=origin
";
    assert_eq!(floors(source, Edition::Java), vec![]);
}

// -- editions -------------------------------------------------------------

/// A scoped member-level floor behaves like a scoped header: it constrains
/// its own edition's build and is inert in the other's — inert, not
/// violated, so a def declaring one floor per edition is buildable on both.
#[test]
fn a_scoped_member_floor_is_inert_in_the_other_edition() {
    let source = "\
theme medieval:
  slot wall -> @oak_planks

def cottage size=5x5:
  requires java version>=1.21.4
  requires bedrock version>=1.21.40
  walls mat_slot=wall height=3

site hamlet:
  place id=a use=cottage theme=medieval at=origin
";
    assert_eq!(
        floors(source, Edition::Java),
        vec![("1.21.4".to_owned(), FloorOrigin::Def("cottage".to_owned()))],
    );
    assert_eq!(
        floors(source, Edition::Bedrock),
        vec![("1.21.40".to_owned(), FloorOrigin::Def("cottage".to_owned()))],
    );
    // The `registry compatibility` row reads only the unscoped floors, and
    // a floor written in one edition's numbering is not one of them.
    assert_eq!(neutral_floors(source), vec![]);
}

/// A `theme=` reference names the *logical* theme, and the pin picks the
/// variant. The floor follows the variant the build actually binds, through
/// the same rule the resolver uses — so the two cannot bind one variant and
/// weigh the other's floor.
#[test]
fn a_pin_picks_the_variant_whose_floor_applies() {
    let source = "\
theme shop_java:
  requires version>=1.21.4
  slot wall -> @oak_planks

theme shop_bedrock:
  slot wall -> @oak_planks

def cottage size=5x5:
  walls mat_slot=wall height=3

site hamlet:
  place id=a use=cottage theme=shop at=origin
";
    assert_eq!(
        floors(source, Edition::Java),
        vec![(
            "1.21.4".to_owned(),
            FloorOrigin::Theme("shop_java".to_owned()),
        )],
    );
    assert_eq!(floors(source, Edition::Bedrock), vec![]);
}

/// The neutral row runs the same composite fold with no edition pinned —
/// the question `cairn check` asks — so an unscoped floor inside a part the
/// build instantiates does reach it.
#[test]
fn the_neutral_row_reads_the_inherited_floors_too() {
    assert_eq!(
        neutral_floors(PLACED),
        vec![("1.21.4".to_owned(), FloorOrigin::Def("cottage".to_owned()))],
    );
}

// -- the line itself ------------------------------------------------------

/// A `requires` line is not a member: it is lifted onto the item, so no
/// geometry pass sees it and `E_UNKNOWN_KEYWORD` is not reported for it.
#[test]
fn the_line_leaves_the_body_it_was_written_in() {
    let module = parse(PLACED).expect("parse");
    let def = module
        .items
        .iter()
        .find(|item| item.name() == "cottage")
        .expect("the def");
    let Item::Def { requires, body, .. } = def else {
        panic!("expected a def, got {:?}", def.kind());
    };
    assert_eq!(requires.len(), 1);
    assert_eq!(requires[0].requirement.as_str(), "version>=1.21.4");
    assert_eq!(body.len(), 1, "only the `walls` line is a member");
    assert!(!codes(PLACED).contains(&"E_UNKNOWN_KEYWORD"));
}

/// An expression the compiler cannot read declares nothing, and is reported
/// wherever it is written. The message names the part, because the span
/// alone answers "which line" without answering "whose".
#[test]
fn an_unreadable_member_floor_is_reported_like_the_header_form() {
    let source = "\
def cottage size=5x5:
  requires mc>=1.20
  walls mat_slot=wall height=3
";
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let invalid: Vec<_> = check(&module, &ir, None)
        .into_iter()
        .filter(|d| d.code.as_str() == "E_INVALID_REQUIRES")
        .collect();
    assert_eq!(invalid.len(), 1);
    assert!(
        invalid[0].primary.starts_with("`requires` in def cottage"),
        "{}",
        invalid[0].primary,
    );
}

/// A `def` nobody places is still checked for a floor that states nothing:
/// the mistake is in the line, and a template nobody uses yet is a template
/// somebody is about to.
#[test]
fn an_unreadable_floor_is_reported_in_a_part_no_build_instantiates() {
    let source = "\
def cottage size=5x5:
  requires mc>=1.20
  walls mat_slot=wall height=3
";
    assert!(codes(source).contains(&"E_INVALID_REQUIRES"));
}

// -- where the line may stand ---------------------------------------------

/// A `struct` and a `site` are the build rather than a part of one, so the
/// floor on them is the file's and is spelled `@requires`. The refusal says
/// that, rather than naming the token three along.
#[test]
fn only_a_def_or_a_theme_may_declare_one() {
    for source in [
        "struct s size=3x3\n  requires version>=1.21\n",
        "site p:\n  requires version>=1.21\n",
        // A member's children are members, and a member is not a part.
        "def d size=3x3:\n  walls height=3\n    requires version>=1.21\n",
    ] {
        let error = parse(source).expect_err(source);
        assert!(
            format!("{error}").contains("is declared directly in a `def` or `theme` body"),
            "{source:?} reported {error}",
        );
    }
}

/// The word is only reserved where it declares something. A member line
/// whose keyword happens to be `requires` parses in the bodies that cannot
/// hold a floor, exactly as it did before the line existed.
#[test]
fn the_word_is_an_ordinary_keyword_where_no_floor_may_stand() {
    for source in [
        "struct s size=3x3\n  requires a=1\n",
        "site p:\n  requires\n",
    ] {
        parse(source).unwrap_or_else(|error| panic!("{source:?} reported {error}"));
    }
}

/// `requires` with nothing after it declares no floor, and in a body that
/// reads floors that is the mistake rather than a member line.
#[test]
fn a_floor_with_no_expression_is_refused_where_floors_are_read() {
    let error = parse("def d size=3x3:\n  requires\n").expect_err("no expression");
    assert!(
        format!("{error}").contains("`requires` requires a value"),
        "{error}",
    );
}
