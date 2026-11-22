//! How a structure's `DataVersion` stands to its world's, and the target it is recompiled at.

use cairn_lang_core::Edition;
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_formats::world::{CrossVersion, WorldVersion, recompile_target};

#[test]
fn a_structure_stands_to_its_world_by_data_version() {
    let world = WorldVersion {
        data_version: 3953,
        name: Some("1.21".to_owned()),
    };
    assert_eq!(world.cross_version(3953), CrossVersion::Same);
    assert_eq!(world.cross_version(3700), CrossVersion::Upgrade);
    assert_eq!(world.cross_version(4189), CrossVersion::Downgrade);
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
