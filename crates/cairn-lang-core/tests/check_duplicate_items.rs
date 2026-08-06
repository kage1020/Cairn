//! Acceptance tests for the top-level-name and header scopes of
//! `cairn_lang_core::check`'s `duplicate` pass.
//!
//! The pass used to look only *inside* items — header args, theme slots,
//! statement args, member `id=`. Two `def hut` blocks, or two `@cairn`
//! lines, reached the resolver with no diagnostic, and one of them was
//! discarded: the resolver binds a name once, so the loser's body never
//! reaches an artifact. `spec/lint.md` §11.3 forbids exactly that.
//!
//! Two invariants are pinned here:
//!
//! 1. **Reporting** — a repeat within one kind, or a repeated
//!    `@directive`, is an error anchored on the repeat with a note on the
//!    first declaration. Names are per-kind: the resolver keys scopes
//!    `struct::` / `def::` / `site::NAME::` and holds themes separately,
//!    so `theme x` alongside `struct x` is not a collision.
//! 2. **Resolution** — the *first* declaration binds. That direction is
//!    not arbitrary: `def` used to bind first for a `place use=` lookup
//!    and last in the scopes map, so one duplicate produced a placement
//!    sized from one body and a scope resolved from the other.

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse, resolve};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn codes(source: &str) -> Vec<&'static str> {
    diagnose(source).iter().map(|d| d.code.as_str()).collect()
}

fn of_code(source: &str, code: DiagnosticCode) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == code)
        .collect()
}

fn slice<'a>(source: &'a str, diag: &Diagnostic) -> &'a str {
    &source[diag.span.clone()]
}

/// Byte offset of the `n`th (0-based) occurrence of `needle`.
fn nth(source: &str, needle: &str, n: usize) -> usize {
    source
        .match_indices(needle)
        .nth(n)
        .unwrap_or_else(|| panic!("occurrence {n} of `{needle}` not found in:\n{source}"))
        .0
}

const THEME: &str = "theme t:\n  slot floor -> @oak_planks\n\n";

/// Two items of the same kind sharing a name earn one error, anchored on
/// the *second* name token — the word the author would rename — with a
/// note on the first.
///
/// One case per kind because the four are separate namespaces with
/// separate resolver code paths; a fix applied to the map for one kind
/// says nothing about the other three.
#[test]
fn di_1_each_item_kind_reports_its_own_duplicate_name() {
    let cases = [
        (
            "theme",
            format!("{THEME}theme t:\n  slot floor -> @stone\n\nstruct s size=3x3\n  floor\n"),
        ),
        (
            "def",
            format!(
                "{THEME}def hut size=3x3:\n  floor id=f\n\ndef hut size=5x5:\n  floor id=f\n\nsite s:\n  place id=a use=hut theme=t at=origin\n"
            ),
        ),
        (
            "struct",
            format!("{THEME}struct s size=3x3\n  floor\n\nstruct s size=5x5\n  floor\n"),
        ),
        (
            "site",
            format!(
                "{THEME}def hut size=3x3:\n  floor id=f\n\nsite s:\n  place id=a use=hut theme=t at=origin\n\nsite s:\n  place id=b use=hut theme=t at=origin\n"
            ),
        ),
    ];
    for (keyword, src) in cases {
        let found = of_code(&src, DiagnosticCode::DuplicateItem);
        assert_eq!(found.len(), 1, "{keyword}: got {found:#?}");
        let d = &found[0];
        assert_eq!(d.severity, Severity::Error);
        let name = match keyword {
            "def" => "hut",
            _ => match keyword {
                "theme" => "t",
                _ => "s",
            },
        };
        assert_eq!(
            d.span.start,
            nth(&src, &format!("{keyword} {name}"), 1) + keyword.len() + 1,
            "{keyword}: span should start at the second name token",
        );
        assert_eq!(
            slice(&src, d),
            name,
            "{keyword}: span should cover the name"
        );
        assert!(
            d.primary.contains(&format!("`{keyword} {name}`")),
            "{keyword}: message should name the kind and the name, got: {}",
            d.primary,
        );
        assert!(
            d.notes.iter().any(|n| n.span.is_some()),
            "{keyword}: a note should point at the first declaration",
        );
    }
}

/// Three declarations earn two errors, not one. Reporting only the first
/// repeat would send the author round the edit-check loop once per extra
/// copy.
#[test]
fn di_2_every_repeat_after_the_first_is_reported() {
    let src = format!(
        "{THEME}struct s size=3x3\n  floor\n\nstruct s size=4x4\n  floor\n\nstruct s size=5x5\n  floor\n"
    );
    let found = of_code(&src, DiagnosticCode::DuplicateItem);
    assert_eq!(found.len(), 2, "got {found:#?}");
    assert_eq!(
        found[0].span.start,
        nth(&src, "struct s", 1) + "struct ".len()
    );
    assert_eq!(
        found[1].span.start,
        nth(&src, "struct s", 2) + "struct ".len()
    );
}

/// The four kinds are separate namespaces, so one name on all four is
/// legal and must stay silent. Without this, the obvious implementation —
/// one map keyed by name — would look correct against every other test
/// here.
#[test]
fn di_3_the_same_name_on_different_kinds_is_not_a_collision() {
    let src = "theme x:\n  slot floor -> @oak_planks\n\n\
struct x size=3x3\n  floor mat_slot=floor\n\n\
def x size=3x3:\n  floor id=f mat_slot=floor\n\n\
site x:\n  place id=a use=x theme=x at=origin\n";
    let found = of_code(src, DiagnosticCode::DuplicateItem);
    assert!(found.is_empty(), "got {found:#?}");
}

/// Duplicates need not be adjacent. A pass that compared each item with
/// its predecessor rather than with everything seen so far would pass the
/// cases above and miss this one.
#[test]
fn di_4_duplicates_separated_by_other_items_are_still_reported() {
    let src = format!(
        "{THEME}struct s size=3x3\n  floor\n\nstruct other size=4x4\n  floor\n\nstruct s size=5x5\n  floor\n"
    );
    let found = of_code(&src, DiagnosticCode::DuplicateItem);
    assert_eq!(found.len(), 1, "got {found:#?}");
    assert_eq!(
        found[0].span.start,
        nth(&src, "struct s", 1) + "struct ".len()
    );
}

/// Each `@directive` may be declared once. `@cairn` and
/// `@intended_targets` have one reader each, which takes the first match;
/// `@requires` floors are folded by taking the strictest, so a second
/// line asking for less leaves no trace at all.
#[test]
fn di_5_each_repeated_header_directive_is_reported() {
    let cases = [
        ("@cairn", "@cairn 2026.06\n@cairn 2026.07\n"),
        (
            "@requires",
            "@requires version>=1.20\n@requires version>=1.19\n",
        ),
        (
            "@intended_targets",
            "@intended_targets [\"1.20.4\"]\n@intended_targets [\"1.21.4\"]\n",
        ),
    ];
    for (directive, headers) in cases {
        let src = format!("{headers}\n{THEME}struct s size=3x3\n  floor\n");
        let found = of_code(&src, DiagnosticCode::DuplicateHeader);
        assert_eq!(found.len(), 1, "{directive}: got {found:#?}");
        let d = &found[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.span.start,
            nth(&src, directive, 1),
            "{directive}: span should start at the repeat",
        );
        assert!(
            d.primary.contains(directive),
            "{directive}: message should name the directive, got: {}",
            d.primary,
        );
        assert!(
            d.notes.iter().any(|n| n.span.is_some()),
            "{directive}: a note should point at the first declaration",
        );
    }
}

/// Three different directives together are the normal case and must stay
/// silent — the negative space for the test above.
#[test]
fn di_6_distinct_header_directives_are_not_duplicates() {
    let src = format!(
        "@cairn 2026.06\n@requires version>=1.20\n@intended_targets [\"1.20.4\"]\n\n{THEME}struct s size=3x3\n  floor\n"
    );
    let found = of_code(&src, DiagnosticCode::DuplicateHeader);
    assert!(found.is_empty(), "got {found:#?}");
}

/// The shipped examples must stay clean. They are the corpus a
/// false-positive here would break loudest.
#[test]
fn di_7_shipped_examples_declare_no_duplicates() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("read examples dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "crn") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read example");
        let found: Vec<_> = diagnose(&src)
            .into_iter()
            .filter(|d| {
                matches!(
                    d.code,
                    DiagnosticCode::DuplicateItem | DiagnosticCode::DuplicateHeader
                )
            })
            .collect();
        assert!(found.is_empty(), "{}: got {found:#?}", path.display());
        checked += 1;
    }
    assert!(checked > 0, "no examples were checked in {}", dir.display());
}

fn resolved(source: &str) -> (cairn_lang_core::resolve::Resolution, BlockArrayIr) {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let block = lower_to_block_array(&ir, &resolution, None);
    (resolution, block)
}

/// Dimensions of a lowered scope, as `(x, y, z)`.
fn dims(block: &BlockArrayIr, key: &str) -> (u32, u32, u32) {
    let d = block
        .structures
        .get(key)
        .map(|a| a.dims)
        .or_else(|| block.placements.get(key).map(|p| p.dims))
        .unwrap_or_else(|| {
            panic!(
                "no scope `{key}` in {:?} / {:?}",
                block.structures.keys().collect::<Vec<_>>(),
                block.placements.keys().collect::<Vec<_>>(),
            )
        });
    (d.x, d.y, d.z)
}

/// The first `def` of a name is the one a `place use=` instantiates *and*
/// the one the scopes map holds.
///
/// The two used to disagree: the placement lookup walked the def list and
/// took the first match, while the scopes map was overwritten by the
/// last. Sizing the placement from one body and resolving its members
/// from the other is not a diagnostic problem, it is a wrong build, so it
/// is pinned separately from the diagnostic above.
#[test]
fn di_8_the_first_def_binds_for_both_the_placement_and_the_scope() {
    let src = format!(
        "{THEME}def hut size=9x9:\n  floor id=first mat_slot=floor\n\ndef hut size=3x3:\n  floor id=second mat_slot=floor\n\nsite s:\n  place id=a use=hut theme=t at=origin\n"
    );
    let (resolution, block) = resolved(&src);
    assert_eq!(
        dims(&block, "site::s::a"),
        (9, 1, 9),
        "the placement must be sized from the first def",
    );
    let scope = resolution
        .scopes
        .get("def::hut")
        .expect("def scope present");
    let second_decl = nth(&src, "def hut", 1);
    for member_start in scope.members.keys() {
        assert!(
            *member_start < second_decl,
            "the scope must hold the first def's members; got one at byte {member_start}, \
             which is inside the second declaration starting at {second_decl}",
        );
    }
}

/// Same rule for the other three kinds, observed through whatever each
/// one actually decides.
#[test]
fn di_9_the_first_declaration_binds_for_every_kind() {
    // theme: the first binding supplies the slot value, so the lowered
    // palette carries oak rather than stone.
    let theme_src = "theme t:\n  slot floor -> @oak_planks\n\n\
theme t:\n  slot floor -> @stone\n\n\
struct s size=3x3\n  floor mat_slot=floor\n";
    let (_, block) = resolved(theme_src);
    let palette: Vec<&str> = block.structures["struct::s"]
        .palette
        .entries
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    assert!(
        palette.iter().any(|id| id.contains("oak")),
        "the first theme must supply the slot value, got palette {palette:?}",
    );

    // struct: the first header supplies `size=`.
    let struct_src = format!("{THEME}struct s size=9x9\n  floor\n\nstruct s size=3x3\n  floor\n");
    let (_, block) = resolved(&struct_src);
    assert_eq!(dims(&block, "struct::s"), (9, 1, 9));

    // site: two blocks of one name put two `place id=a` rows under the
    // same scope key, and the first wins there too.
    let site_src = format!(
        "{THEME}def big size=9x9:\n  floor id=f\n\ndef small size=3x3:\n  floor id=f\n\nsite s:\n  place id=a use=big theme=t at=origin\n\nsite s:\n  place id=a use=small theme=t at=origin\n"
    );
    let (_, block) = resolved(&site_src);
    assert_eq!(dims(&block, "site::s::a"), (9, 1, 9));
}

/// Binding the first declaration does not mean ignoring the rest. The
/// duplicate's body is still walked, so problems inside it surface in the
/// same run as the collision that hides it — otherwise fixing the name
/// would reveal a fresh wave of errors the author never saw.
#[test]
fn di_10_the_duplicate_body_still_contributes_its_own_diagnostics() {
    let src = format!(
        "{THEME}struct s size=3x3\n  floor mat_slot=floor\n\nstruct s size=3x3\n  floor mat_slot=nosuchslot\n"
    );
    let found = codes(&src);
    assert!(
        found.contains(&"E_DUPLICATE_ITEM") && found.contains(&"E_UNRESOLVED_SLOT"),
        "expected both the collision and the duplicate body's own error, got {found:?}",
    );
}
