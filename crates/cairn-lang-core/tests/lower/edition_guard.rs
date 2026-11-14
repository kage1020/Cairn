//! A build keeps the members of the `@edition` guards for its own edition, and an unpinned check
//! reports a guarded line's findings from that edition's lowering
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use cairn_lang_core::block_array::{BlockArrayIr, findings_without_target, lower_to_block_array};
use cairn_lang_core::{Edition, lower, parse, resolve};

use crate::common::{bin, blocks_where, only_structure, slice};

/// `source` lowered for `edition`, or for none, with no registry pack.
fn lowered_for(source: &str, edition: Option<Edition>) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    lower_to_block_array(&ir, &resolve(&ir, edition), None)
}

/// A block id a raw member built, with its cells.
type Built = (String, Vec<(u32, u32, u32)>);

/// The ids the raw members of `out` built, with their cells.
fn raw_blocks(out: &BlockArrayIr) -> Vec<Built> {
    out.members["struct::s"]
        .iter()
        .filter(|member| member.role.starts_with("raw_"))
        .flat_map(|member| &member.resolved_state)
        .map(|block| (block.state.id.clone(), block.cells().collect()))
        .collect()
}

/// `source`'s findings with no target, for `edition` or for both, as code and text.
fn unpinned(source: &str, edition: Option<Edition>) -> Vec<(&'static str, &str)> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    findings_without_target(&lower(&module), edition, &|_, _| true)
        .iter()
        .map(|d| (d.code.as_str(), slice(source, d)))
        .collect()
}

const LAMP: &str = "@edition java\n    raw_block mat=minecraft:light at=2,1,1\n  \
                    @edition bedrock\n    raw_block mat=minecraft:light_block_15 at=2,1,1";

#[test]
fn a_build_keeps_the_guards_for_its_edition_and_a_build_of_none_keeps_none() {
    let source = bin(LAMP);
    let built = |edition| raw_blocks(&lowered_for(&source, edition));
    assert_eq!(
        built(Some(Edition::Java)),
        [("minecraft:light".to_owned(), vec![(3, 1, 2)])],
    );
    assert_eq!(
        built(Some(Edition::Bedrock)),
        [("minecraft:light_block_15".to_owned(), vec![(3, 1, 2)])],
    );
    assert_eq!(built(None), []);
}

#[test]
fn a_build_keeps_the_tier0_parts_of_its_editions_guard_and_a_build_of_none_keeps_none() {
    let source = bin("@edition java\n    repeater at=1,1,1 facing=east\n  \
         @edition bedrock\n    redstone_torch at=1,1,1");
    let parts = |edition| {
        blocks_where(only_structure(&lowered_for(&source, edition)), |id| {
            id == "minecraft:repeater" || id == "minecraft:redstone_torch"
        })
    };
    assert_eq!(
        parts(Some(Edition::Java)),
        [(
            (2, 1, 2),
            "minecraft:repeater[delay=1,facing=west]".to_owned()
        )],
    );
    assert_eq!(
        parts(Some(Edition::Bedrock)),
        [((2, 1, 2), "minecraft:redstone_torch".to_owned())],
    );
    assert_eq!(parts(None), []);
}

#[test]
fn two_branches_writing_one_cell_never_contest_it() {
    let source = bin(LAMP);
    for edition in [Some(Edition::Java), Some(Edition::Bedrock), None] {
        assert_eq!(lowered_for(&source, edition).diagnostics, [], "{edition:?}");
        assert_eq!(unpinned(&source, edition), [], "{edition:?}");
    }
}

#[test]
fn an_unpinned_check_reports_a_guarded_lines_finding_from_its_own_edition() {
    let source = bin(
        "@edition bedrock\n    raw_block mat=minecraft:light_block_15 at=2,9,1\n  \
         @edition java\n    raw_block mat=minecraft:light at=2,1,1",
    );
    let refused = [(
        "E_INVALID_MEMBER",
        "raw_block mat=minecraft:light_block_15 at=2,9,1",
    )];
    assert_eq!(unpinned(&source, None), refused);
    assert_eq!(unpinned(&source, Some(Edition::Bedrock)), refused);
    assert_eq!(unpinned(&source, Some(Edition::Java)), []);
}

#[test]
fn a_conflict_a_guarded_line_loses_is_that_editions_finding_whichever_line_comes_first() {
    let guarded = "  @edition java\n    raw_block mat=minecraft:stone at=2,0,2\n";
    let bare = "  raw_block mat=minecraft:dirt at=2,0,2\n";
    for (body, later) in [
        (
            format!("{guarded}{bare}"),
            "raw_block mat=minecraft:dirt at=2,0,2",
        ),
        (
            format!("{bare}{guarded}"),
            "raw_block mat=minecraft:stone at=2,0,2",
        ),
    ] {
        let source = format!("struct s size=5x5\n{body}");
        assert_eq!(
            unpinned(&source, None),
            [("W_PHASE_CONFLICT", later)],
            "{body}"
        );
        assert_eq!(
            unpinned(&source, Some(Edition::Bedrock)),
            [],
            "Bedrock reads only the unguarded line",
        );
    }
}

#[test]
fn a_guard_under_a_level_is_measured_from_the_level() {
    let source = bin("level y=1\n    @edition java\n      raw_block mat=minecraft:stone at=1,0,1");
    assert_eq!(
        raw_blocks(&lowered_for(&source, Some(Edition::Java))),
        [("minecraft:stone".to_owned(), vec![(2, 1, 2)])],
    );
    assert_eq!(
        raw_blocks(&lowered_for(&source, Some(Edition::Bedrock))),
        []
    );
}

#[test]
fn a_guard_in_a_def_body_is_kept_where_the_def_is_placed_for_its_edition() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  def hut size=3x3:\n  walls mat_slot=wall height=2\n  \
                  @edition java\n    raw_block mat=minecraft:glowstone at=1,0,1\n\n\
                  site s:\n  place id=a use=hut theme=t at=origin\n";
    let glowstone = |edition| {
        lowered_for(source, Some(edition))
            .members
            .values()
            .flatten()
            .filter(|member| member.role == "raw_block")
            .count()
    };
    assert_eq!(glowstone(Edition::Java), 1);
    assert_eq!(glowstone(Edition::Bedrock), 0);
}

#[test]
fn a_refused_line_is_lowered_by_no_edition() {
    let source = bin("@edition java\n    circuit id=c");
    for edition in [None, Some(Edition::Java), Some(Edition::Bedrock)] {
        assert_eq!(unpinned(&source, edition), [], "{edition:?}");
        assert_eq!(lowered_for(&source, edition).diagnostics, [], "{edition:?}");
    }
}

#[test]
fn a_build_keeps_a_raw_repeat_of_its_editions_guard_with_every_copy() {
    let source = bin("@edition java\n    raw_repeat count=2 step=2,0,0\n      \
         raw_block mat=minecraft:light at=0,1,1");
    let built = |edition| raw_blocks(&lowered_for(&source, edition));
    assert_eq!(
        built(Some(Edition::Java)),
        [("minecraft:light".to_owned(), vec![(1, 1, 2), (3, 1, 2)])],
    );
    assert_eq!(built(Some(Edition::Bedrock)), []);
}
