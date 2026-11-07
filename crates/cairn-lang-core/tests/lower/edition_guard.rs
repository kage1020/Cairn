//! A build keeps the members of the `@edition` guards for its own edition, and an unpinned check
//! reports a guarded line's findings from that edition's lowering
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use cairn_lang_core::block_array::{BlockArrayIr, findings_without_target, lower_to_block_array};
use cairn_lang_core::{Edition, lower, parse, resolve};

use crate::common::{bin, slice};

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
