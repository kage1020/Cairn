//! Which versions of a built-in pack's edition declare an id its pinned target does not, nearest
//! on either side (`spec/versioning-editions` "Which registry an unknown ID is unknown to").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{DeclaredElsewhere, TargetRegistry};
use cairn_lang_formats::registry::builtin_pack;

fn around(edition: Edition, version: &str, id: &str) -> Option<DeclaredElsewhere> {
    builtin_pack(edition)
        .view(Some(version))
        .declared_elsewhere(&format!("minecraft:{id}"))
}

fn declared(edition: Edition, until: Option<&str>, since: Option<&str>) -> DeclaredElsewhere {
    DeclaredElsewhere {
        edition,
        until: until.map(str::to_owned),
        since: since.map(str::to_owned),
    }
}

#[test]
fn a_block_a_later_version_adds_names_the_first_version_after_the_target() {
    assert_eq!(
        around(Edition::Java, "1.20.4", "pale_oak_planks"),
        Some(declared(Edition::Java, None, Some("1.21.4"))),
    );
    assert_eq!(
        around(Edition::Java, "1.21", "pale_oak_planks"),
        Some(declared(Edition::Java, None, Some("1.21.4"))),
    );
    assert_eq!(
        around(Edition::Bedrock, "1.21.0", "stone_bricks"),
        Some(declared(Edition::Bedrock, None, Some("1.21.40"))),
    );
}

#[test]
fn a_block_a_later_version_drops_names_the_last_version_before_the_target() {
    assert_eq!(
        around(Edition::Java, "26.3", "chain"),
        Some(declared(Edition::Java, Some("1.21.4"), None)),
    );
}

#[test]
fn a_block_no_version_declares_or_an_unpinned_view_names_none() {
    assert_eq!(around(Edition::Java, "1.21.4", "totally_not_a_block"), None);
    assert_eq!(around(Edition::Bedrock, "1.26.60", "light"), None);
    assert_eq!(
        builtin_pack(Edition::Java)
            .view(None)
            .declared_elsewhere("minecraft:pale_oak_planks"),
        None,
    );
}
