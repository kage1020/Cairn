//! A composite's minimum version: the max over the `requires` floors of the parts a build
//! instantiates, per `spec/versioning-editions` "Fail-loud and minimum-version inference".

use cairn_lang_core::ast::{Item, Statement};
use cairn_lang_core::resolve::{
    FloorOrigin, VersionFloor, declared_version_floors, unscoped_version_floors,
};
use cairn_lang_core::{DiagnosticCode, Edition, check, lower, parse};

use crate::common::codes;

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

#[test]
fn a_place_inherits_the_floor_of_the_def_it_instantiates() {
    assert_eq!(
        floors(PLACED, Edition::Java),
        vec![("1.21.4".to_owned(), FloorOrigin::Def("cottage".to_owned()))],
    );
}

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

/// Settles what `spec/versioning-editions` "Fail-loud and minimum-version inference" leaves open.
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

#[test]
fn a_module_with_no_scope_binds_no_theme() {
    let source = "\
theme medieval:
  requires version>=1.21.4
  slot wall -> @oak_planks
";
    assert_eq!(floors(source, Edition::Java), vec![]);
}

#[test]
fn an_unplaced_def_binds_no_theme_either() {
    let source = "\
theme medieval:
  requires version>=1.21.4
  slot wall -> @oak_planks

def cottage size=5x5:
  walls mat_slot=wall height=3
";
    assert_eq!(floors(source, Edition::Java), vec![]);
    // The `def` is unplaced, as in `a_def_nobody_places_contributes_no_floor`.
    assert!(codes(source).contains(&"W_UNUSED_DEF"));
}

/// Two themes in the file, so the auto-pick cannot cover for the `place`.
#[test]
fn a_place_binds_the_theme_it_names() {
    let source = "\
theme medieval:
  requires version>=1.21.4
  slot wall -> @oak_planks

theme baroque:
  slot wall -> @quartz_block

def cottage size=5x5:
  walls mat_slot=wall height=3

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
    // The `registry compatibility` row reads only the unscoped floors.
    assert_eq!(neutral_floors(source), vec![]);
}

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

#[test]
fn the_neutral_row_reads_the_inherited_floors_too() {
    assert_eq!(
        neutral_floors(PLACED),
        vec![("1.21.4".to_owned(), FloorOrigin::Def("cottage".to_owned()))],
    );
}

/// The neutral row may report against both editions at once, so a floor only one inherits is out.
#[test]
fn the_neutral_row_leaves_out_a_theme_the_two_editions_bind_differently() {
    // Over-reporting: Java binds `shop` and inherits the floor, Bedrock binds `shop_bedrock`.
    let over = "\
theme shop:
  requires version>=1.21.4
  slot wall -> @oak_planks

theme shop_bedrock:
  slot wall -> @oak_planks

struct keep size=5x5
  walls mat_slot=wall height=3
";
    assert_eq!(
        floors(over, Edition::Java),
        vec![("1.21.4".to_owned(), FloorOrigin::Theme("shop".to_owned()))],
    );
    assert_eq!(floors(over, Edition::Bedrock), vec![]);
    assert_eq!(neutral_floors(over), vec![]);

    // Under-reporting: the floor is on the variant only Java binds.
    let under = "\
theme shop:
  slot wall -> @oak_planks

theme shop_java:
  requires version>=1.21.4
  slot wall -> @oak_planks

struct keep size=5x5
  walls mat_slot=wall height=3
";
    assert_eq!(
        floors(under, Edition::Java),
        vec![(
            "1.21.4".to_owned(),
            FloorOrigin::Theme("shop_java".to_owned()),
        )],
    );
    assert_eq!(floors(under, Edition::Bedrock), vec![]);
    assert_eq!(neutral_floors(under), vec![]);
}

#[test]
fn the_neutral_row_keeps_a_theme_both_editions_bind() {
    let source = "\
theme medieval:
  requires version>=1.21.4
  slot wall -> @oak_planks

struct keep size=5x5
  walls mat_slot=wall height=3
";
    for edition in [Edition::Java, Edition::Bedrock] {
        assert_eq!(
            floors(source, edition),
            vec![(
                "1.21.4".to_owned(),
                FloorOrigin::Theme("medieval".to_owned()),
            )],
            "{edition:?}",
        );
    }
    assert_eq!(
        neutral_floors(source),
        vec![(
            "1.21.4".to_owned(),
            FloorOrigin::Theme("medieval".to_owned()),
        )],
    );
}

// -- the line itself ------------------------------------------------------

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
        .filter(|d| d.code == DiagnosticCode::InvalidRequires)
        .collect();
    assert_eq!(invalid.len(), 1);
    assert!(
        invalid[0].primary.starts_with("`requires` in def cottage"),
        "{}",
        invalid[0].primary,
    );
}

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

#[test]
fn only_a_def_or_a_theme_may_declare_one() {
    for source in [
        "struct s size=3x3\n  requires version>=1.21\n",
        "site p:\n  requires version>=1.21\n",
    ] {
        let error = parse(source).expect_err(source);
        assert!(
            format!("{error}").contains("a `struct` or a `site` may not declare"),
            "{source:?} reported {error}",
        );
    }
    let nested = parse("def d size=3x3:\n  walls height=3\n    requires version>=1.21\n")
        .expect_err("a member may not carry a floor");
    assert!(
        format!("{nested}").contains("a member may not declare"),
        "{nested}",
    );
    assert!(
        format!("{nested}").contains("the `def` body's own level"),
        "the repair is a dedent, not a different directive: {nested}",
    );
}

#[test]
fn the_word_is_an_ordinary_keyword_where_no_floor_may_stand() {
    for source in [
        "struct s size=3x3\n  requires a=1\n",
        "site p:\n  requires\n",
        "def d size=3x3:\n  walls height=3\n    requires a=1\n",
    ] {
        let module = parse(source).unwrap_or_else(|error| panic!("{source:?} reported {error}"));
        let mut keywords = Vec::new();
        collect_keywords(&module, &mut keywords);
        assert!(
            keywords.contains(&"requires".to_owned()),
            "{source:?} lost the member: {keywords:?}",
        );
    }
}

/// A `requires` selector row is `check::keyword_allowlist`'s to refuse, so it still parses.
#[test]
fn a_theme_body_reads_the_line_through_the_same_policy() {
    let source = "\
theme t:
  requires version>=1.21.4
  slot wall -> @oak_planks
  window[class=small] -> frame=@spruce
";
    let module = parse(source).expect("parse");
    let Item::Theme { requires, body, .. } = &module.items[0] else {
        panic!("expected a theme");
    };
    assert_eq!(requires.len(), 1);
    assert_eq!(body.len(), 2, "the slot and the selector both survive");
}

/// The caret is on the keyword, since an empty expression has no token but the newline.
#[test]
fn a_floor_with_no_expression_is_refused_where_floors_are_read() {
    let error = parse("def d size=3x3:\n  requires\n").expect_err("no expression");
    let rendered = format!("{error}");
    assert!(
        rendered.contains("`requires` requires a value"),
        "{rendered}"
    );
    assert!(
        rendered.contains("2:3"),
        "the caret is on the keyword: {rendered}"
    );
}

#[test]
fn a_floor_takes_no_indented_body() {
    let error = parse("def d size=3x3:\n  requires version>=1.21\n    x=1\n")
        .expect_err("a floor takes no body");
    assert!(
        format!("{error}").contains("takes no indented body"),
        "{error}",
    );
}

/// Every `Generic` statement keyword in the module, at any depth.
fn collect_keywords(module: &cairn_lang_core::ast::Module, out: &mut Vec<String>) {
    fn walk(body: &[Statement], out: &mut Vec<String>) {
        for statement in body {
            if let Statement::Generic {
                keyword, children, ..
            } = statement
            {
                out.push(keyword.clone());
                walk(children, out);
            }
        }
    }
    for item in &module.items {
        match item {
            Item::Def { body, .. } | Item::Site { body, .. } | Item::Struct { body, .. } => {
                walk(body, out);
            }
            _ => {}
        }
    }
}
