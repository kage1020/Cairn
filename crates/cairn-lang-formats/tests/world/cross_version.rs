//! How a structure's `DataVersion` stands to its world's, and the target it is recompiled at.

use cairn_lang_core::Edition;
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_formats::world::{CrossVersion, recompile_target};

#[test]
fn a_structure_stands_to_its_world_by_data_version() {
    assert_eq!(CrossVersion::of(3953, 3953), CrossVersion::Same);
    assert_eq!(CrossVersion::of(3700, 3953), CrossVersion::Upgrade);
    assert_eq!(CrossVersion::of(4189, 3953), CrossVersion::Downgrade);
}

#[test]
fn the_recompile_target_is_the_newest_at_or_below_the_world() {
    let pack = builtin_pack(Edition::Java);
    for (world, target) in [
        (3953, Some("1.21")),
        (4000, Some("1.21")),
        (4189, Some("1.21.4")),
        (5023, Some("26.3")),
        (9999, Some("26.3")),
        (3700, Some("1.20.4")),
        (3699, None),
    ] {
        assert_eq!(
            recompile_target(pack, world).map(|row| row.mc_version.as_str()),
            target,
            "{world}"
        );
    }
}
