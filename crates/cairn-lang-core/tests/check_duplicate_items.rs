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
//!    single-valued `@directive`, is an error anchored on the repeat
//!    with a note on the first declaration. Names are per-kind: the
//!    resolver keys scopes `struct::` / `def::` / `site::NAME::` and
//!    holds themes separately, so `theme x` alongside `struct x` is not
//!    a collision.
//! 2. **Resolution** — the first declaration to claim a *binding key*
//!    wins. The key is the name for `theme` / `struct` / `def`, but
//!    `site::NAME::PLACE_ID` for a placement, so two `site` blocks of
//!    one name merge rather than shadow. `FIRST_BINDING_WINS` on
//!    `resolve` records why that direction was picked.

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
            "t",
            format!(
                "{THEME}theme t:
  slot floor -> @stone

struct s size=3x3
  floor
"
            ),
        ),
        (
            "def",
            "hut",
            format!(
                "{THEME}def hut size=3x3:
  floor id=f

def hut size=5x5:
  floor id=f

site s:
  place id=a use=hut theme=t at=origin
"
            ),
        ),
        (
            "struct",
            "s",
            format!(
                "{THEME}struct s size=3x3
  floor

struct s size=5x5
  floor
"
            ),
        ),
        (
            "site",
            "s",
            format!(
                "{THEME}def hut size=3x3:
  floor id=f

site s:
  place id=a use=hut theme=t at=origin

site s:
  place id=b use=hut theme=t at=origin
"
            ),
        ),
    ];
    for (keyword, name, src) in cases {
        let found = of_code(&src, DiagnosticCode::DuplicateItem);
        assert_eq!(found.len(), 1, "{keyword}: got {found:#?}");
        let d = &found[0];
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(
            slice(&src, d),
            name,
            "{keyword}: span should cover the name token",
        );
        assert!(
            d.span.start > nth(&src, &format!("{keyword} {name}"), 1),
            "{keyword}: span should be inside the second declaration",
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

/// The anchor survives extra whitespace between keyword and name.
///
/// This is the case `name_span` exists for: the alternative was to
/// rebuild the header line as `span.start + keyword.len() + 1`, which
/// silently slides off the name as soon as the author lines their
/// declarations up.
#[test]
fn di_1b_the_anchor_follows_the_name_through_padded_whitespace() {
    let src = format!(
        "{THEME}struct   s size=3x3
  floor

struct   s size=5x5
  floor
"
    );
    let found = of_code(&src, DiagnosticCode::DuplicateItem);
    assert_eq!(found.len(), 1, "got {found:#?}");
    assert_eq!(slice(&src, &found[0]), "s");
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

/// A single-valued `@directive` may be declared once.
///
/// `@cairn` and `@intended_targets` each answer a one-answer question,
/// and neither has a consumer in the compiler yet — so nothing would
/// choose between two of them.
#[test]
fn di_5_each_repeated_single_valued_header_is_reported() {
    let cases = [
        (
            "@cairn",
            "@cairn 2026.06
@cairn 2026.07
",
        ),
        (
            "@intended_targets",
            "@intended_targets [\"1.20.4\"]
@intended_targets [\"1.21.4\"]
",
        ),
    ];
    for (directive, headers) in cases {
        let src = format!(
            "{headers}
{THEME}struct s size=3x3
  floor
"
        );
        let found = of_code(&src, DiagnosticCode::DuplicateHeader);
        assert_eq!(found.len(), 1, "{directive}: got {found:#?}");
        let d = &found[0];
        assert_eq!(d.severity(), Severity::Error);
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

/// `@requires` is exempt, and that is not an oversight.
///
/// `resolve::version_axes` folds every `version>=X` floor to the
/// strictest across all of them — the conjunction of the constraints,
/// not a choice between them — which `RegistryRange`'s doc states and
/// that module's tests pin. Reporting a repeat here would make an error
/// of a shape the rest of the crate defines as meaningful, and the
/// collision would not show up in either test suite because neither
/// crosses the other's layer. This test is that crossing.
#[test]
fn di_5b_repeated_requires_is_legal_because_its_floors_compose() {
    let src = format!(
        "@requires version>=1.20
@requires version>=1.21

{THEME}struct s size=3x3
  floor
"
    );
    let found = of_code(&src, DiagnosticCode::DuplicateHeader);
    assert!(found.is_empty(), "got {found:#?}");
    let module = parse(&src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let axes =
        cairn_lang_core::resolve::compute_axes(&module, &ir, &resolution, Vec::new(), Vec::new());
    assert_eq!(
        axes.registry_compat.min, "1.21",
        "the strictest floor must still win",
    );
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

/// A duplicate `theme` name: the first binding supplies the slot value,
/// so the lowered palette carries oak rather than stone.
#[test]
fn di_9a_the_first_theme_supplies_the_slot_value() {
    let src = "theme t:
  slot floor -> @oak_planks

theme t:
  slot floor -> @stone

struct s size=3x3
  floor mat_slot=floor
";
    let (_, block) = resolved(src);
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
}

/// A duplicate `struct` name: the first header supplies `size=`.
///
/// This one goes through block-array lowering's own `structures` map,
/// not the resolver's. The resolver's half is invisible in the dims, so
/// a fix applied only there would leave this test red.
#[test]
fn di_9b_the_first_struct_supplies_the_size() {
    let src = format!(
        "{THEME}struct s size=9x9
  floor

struct s size=3x3
  floor
"
    );
    let (_, block) = resolved(&src);
    assert_eq!(dims(&block, "struct::s"), (9, 1, 9));
}

/// Two `site` blocks of one name whose places collide on `id=`: the
/// binding key is `site::NAME::PLACE_ID`, so this is the only site shape
/// where first-wins has anything to decide.
#[test]
fn di_9c_a_place_id_repeated_across_site_blocks_keeps_the_first() {
    let src = format!(
        "{THEME}def big size=9x9:
  floor id=f

def small size=3x3:
  floor id=f

site s:
  place id=a use=big theme=t at=origin

site s:
  place id=a use=small theme=t at=origin
"
    );
    let (_, block) = resolved(&src);
    assert_eq!(dims(&block, "site::s::a"), (9, 1, 9));
    assert_eq!(
        block.placements.len(),
        1,
        "the colliding place must not produce a second entry",
    );
}

/// Two `site` blocks of one name whose places do *not* collide: nothing
/// is dropped. Both build, under one shared `site::s::` namespace.
///
/// This is the shape the diagnostic's note has to describe correctly.
/// An author told "the first `site s` is the one that resolves" would
/// delete the second block and lose a placement that was in the build —
/// so the claim is pinned here rather than left to the prose.
#[test]
fn di_9d_site_blocks_of_one_name_merge_rather_than_shadow() {
    let src = format!(
        "{THEME}def big size=9x9:
  floor id=f

def small size=3x3:
  floor id=f

site s:
  place id=a use=big theme=t at=origin

site s:
  place id=b use=small theme=t at=origin
"
    );
    let (resolution, block) = resolved(&src);
    let keys: Vec<&str> = block.placements.keys().map(String::as_str).collect();
    assert_eq!(keys, ["site::s::a", "site::s::b"]);
    assert_eq!(dims(&block, "site::s::a"), (9, 1, 9));
    assert_eq!(dims(&block, "site::s::b"), (3, 1, 3));
    assert!(
        resolution.scopes.contains_key("site::s::a")
            && resolution.scopes.contains_key("site::s::b"),
        "both places must resolve, got {:?}",
        resolution.scopes.keys().collect::<Vec<_>>(),
    );
    // The collision is still worth reporting: the namespace is shared
    // but `east_of=` is not, so the two blocks cannot reference each
    // other's places.
    assert!(codes(&src).contains(&"E_DUPLICATE_ITEM"));
}

/// Binding the first declaration does not mean ignoring the rest. The
/// duplicate's body is still walked, so problems inside it surface in the
/// same run as the collision that hides it — otherwise fixing the name
/// would reveal a fresh wave of errors the author never saw.
#[test]
fn di_10_the_duplicate_body_still_contributes_its_own_diagnostics() {
    let struct_src = format!(
        "{THEME}struct s size=3x3
  floor mat_slot=floor

struct s size=3x3
  floor mat_slot=nosuchslot
"
    );
    let found = codes(&struct_src);
    assert!(
        found.contains(&"E_DUPLICATE_ITEM") && found.contains(&"E_UNRESOLVED_SLOT"),
        "expected both the collision and the duplicate body's own error, got {found:?}",
    );

    // `theme` is the kind where this is not free. Theme findings are
    // driven by the bound map, which holds one entry per name, so the
    // shadowed body's bad slot value used to vanish with it.
    let theme_src = "theme t:
  slot floor -> @oak_planks

theme t:
  slot floor -> notatoken

struct s size=3x3
  floor mat_slot=floor
";
    let found = codes(theme_src);
    assert!(
        found.contains(&"E_DUPLICATE_ITEM") && found.contains(&"E_UNKNOWN_SLOT_TARGET"),
        "the shadowed theme body's own finding must survive, got {found:?}",
    );
}
