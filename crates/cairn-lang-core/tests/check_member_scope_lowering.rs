//! What a misplaced member actually costs, measured on the build rather
//! than on the diagnostic.
//!
//! `check_member_scope.rs` pins the wording and the anchoring of
//! `E_MISPLACED_MEMBER`; every assertion there filters to that code and
//! never lowers, so none of them would notice if the premise — "nothing
//! in this body reads that keyword" — stopped being true. These do the
//! lowering and compare against the same source with the row moved to
//! the body that does read it.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::{lower, parse, resolve};

fn build(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

fn solid_cells(array: &BlockArray) -> usize {
    array.voxels.iter().filter(|c| c.0 != 0).count()
}

const PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n";

/// A geometry row among a site's placements lays no voxels. Same source,
/// same site, one row apart — and the site's own placement is unchanged,
/// so the row contributed nothing rather than displacing something.
#[test]
fn msl_1_a_geometry_row_in_a_site_body_builds_nothing() {
    let without = format!("{PRELUDE}site s:\n  place id=anchor use=hut theme=plain at=origin\n");
    let with = format!(
        "{PRELUDE}site s:\n  place id=anchor use=hut theme=plain at=origin\n  \
         floor id=stray mat_slot=floor\n  \
         walls id=stray_walls mat_slot=wall height=3\n"
    );
    let base = build(&without);
    let extended = build(&with);
    assert_eq!(
        extended.placements.len(),
        base.placements.len(),
        "the stray geometry rows must not become placements",
    );
    assert_eq!(
        extended.structures.len(),
        base.structures.len(),
        "nor structures of their own",
    );
    let anchor = "site::s::anchor";
    assert_eq!(
        extended.placements[anchor].dims, base.placements[anchor].dims,
        "and the placement that is there must be untouched by them",
    );
    assert_eq!(
        solid_cells(&extended.structures[anchor]),
        solid_cells(&base.structures[anchor]),
    );
}

/// A `place` in a `def` body produces no placement, and a `connect`
/// there lays no walkway. Both are what `W_DEFERRED_MEMBER` says during
/// `cairn lower` — the code `cairn check` never sees, which is why the
/// same fact needs a check-layer owner.
#[test]
fn msl_2_a_site_row_in_a_geometry_body_builds_nothing() {
    let src = format!(
        "{PRELUDE}def lodge size=5x5:\n  floor id=floor mat_slot=floor\n  \
         place id=inner use=hut theme=plain at=origin\n  \
         connect inner.entry to inner.entry path=@gravel\n\n\
         site s:\n  place id=anchor use=lodge theme=plain at=origin\n"
    );
    let built = build(&src);
    assert_eq!(
        built.placements.len(),
        1,
        "only the site's own `place` reaches the build",
    );
    assert!(
        built.walkways.is_empty(),
        "the `connect` in the def body lays no walkway",
    );
}

/// The other half of the message's claim: a `place` in a geometry body
/// is dropped from the build, not from resolution. The struct's own
/// members still resolve around it, so the finding is about the one row.
#[test]
fn msl_3_a_misplaced_row_does_not_disturb_its_siblings() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         place id=inner use=hut theme=plain at=origin\n  \
         walls mat_slot=wall height=3\n"
    );
    let clean = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls mat_slot=wall height=3\n"
    );
    assert_eq!(
        solid_cells(&build(&src).structures["struct::s"]),
        solid_cells(&build(&clean).structures["struct::s"]),
        "the misplaced row is the only thing lost",
    );
}
