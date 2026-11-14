//! A structure file held to the world's version (`spec/versioning-editions` "Cross-version
//! application"): newer is refused, older needs `--allow-cross-version`, and the stamp stays.

use std::path::Path;

use crate::common::{int_tags, nbt_body};
use crate::install::{Saves, java_level_dat, structure_at};

/// The note every older structure's report carries.
const DFU_NOTE: &str = "  note: it keeps DataVersion 3700, so the game upgrades it through \
     DataFixerUpper as it loads, which can lose items, signs, paintings and block entities\n";

/// The note every newer structure's refusal carries.
const DOWNGRADE_NOTE: &str = "  note: a release cannot read a structure newer than itself, and \
     `--allow-cross-version` installs only an older one\n";

#[test]
fn a_structure_newer_than_the_world_is_refused_with_or_without_the_flag() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.21.4");
    let world = saves.java_world("New World", 3953, "1.21");
    for flag in [None, Some("--allow-cross-version")] {
        let mut args = vec![structure.as_str(), "--world", &world];
        args.extend(flag);
        let run = saves.install(&args);
        assert_eq!(run.code, Some(1), "{flag:?}");
        assert_eq!(run.stdout, "");
        assert_eq!(
            run.stderr,
            [
                "error[E_CROSS_VERSION_DOWNGRADE]: `1.21.4/cottage.nbt` is built for DataVersion \
                 4189 (1.21.4), newer than the world's 3953 (1.21)\n",
                DOWNGRADE_NOTE,
                "  fix: recompile its source for the world, with `--edition java --target 1.21`\n",
            ]
            .concat(),
            "{flag:?}"
        );
    }
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn a_structure_older_than_the_world_is_refused_without_the_flag() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    let world = saves.java_world("New World", 3953, "1.21");
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        [
            "error[E_CROSS_VERSION_UPGRADE]: `1.20.4/cottage.nbt` is built for DataVersion 3700 \
             (1.20.4), older than the world's 3953 (1.21)\n",
            DFU_NOTE,
            "  fix: recompile its source for the world, with `--edition java --target 1.21`, or \
             install this file as it is with `--allow-cross-version`\n",
        ]
        .concat()
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn the_flag_installs_an_older_structure_byte_for_byte_keeping_its_stamp() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    let world = saves.java_world("New World", 3953, "1.21");
    let run = saves.install(&[&structure, "--world", &world, "--allow-cross-version"]);
    let installed = structure_at(&world, "minecraft", &["cottage.nbt"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        run.stderr,
        [
            "warning[W_CROSS_VERSION_UPGRADE]: `1.20.4/cottage.nbt` is built for DataVersion \
             3700 (1.20.4), older than the world's 3953 (1.21), as `--allow-cross-version` \
             allows\n",
            DFU_NOTE,
            "  fix: recompile its source for the world, with `--edition java --target 1.21`\n",
        ]
        .concat()
    );
    assert_eq!(
        run.stdout,
        format!("installed minecraft:cottage at {}\n", installed.display())
    );
    assert_eq!(saves.read(&installed), saves.read(Path::new(&structure)));
    assert_eq!(
        int_tags(
            &nbt_body(&saves.path().join(&installed)),
            "DataVersion",
            false
        ),
        [3700]
    );
}

#[test]
fn a_structure_of_the_worlds_version_installs_without_a_word() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    let world = saves.java_world("New World", 3700, "1.20.4");
    let run = saves.install(&[&structure, "--world", &world]);
    let installed = structure_at(&world, "minecraft", &["cottage.nbt"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(run.stderr, "");
    assert_eq!(
        run.stdout,
        format!("installed minecraft:cottage at {}\n", installed.display())
    );
    assert_eq!(saves.read(&installed), saves.read(Path::new(&structure)));
}

#[test]
fn a_world_between_two_targets_is_recompiled_for_at_the_older() {
    let saves = Saves::new();
    let newer = saves.compiled("java", "26.3");
    let older = saves.compiled("java", "1.20.4");
    let world = saves.java_world("Patched", 3955, "1.21.1");
    let run = saves.install(&[&newer, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        [
            "error[E_CROSS_VERSION_DOWNGRADE]: `26.3/cottage.nbt` is built for DataVersion 5023 \
             (26.3), newer than the world's 3955 (1.21.1)\n",
            DOWNGRADE_NOTE,
            "  fix: recompile its source with `--edition java --target 1.21`, the newest target \
             no newer than the world, and install that with `--allow-cross-version`\n",
        ]
        .concat()
    );
    let run = saves.install(&[&older, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        [
            "error[E_CROSS_VERSION_UPGRADE]: `1.20.4/cottage.nbt` is built for DataVersion 3700 \
             (1.20.4), older than the world's 3955 (1.21.1)\n",
            DFU_NOTE,
            "  fix: recompile its source with `--edition java --target 1.21`, the newest target \
             no newer than the world, to leave the game less to upgrade, and install it with \
             `--allow-cross-version`\n",
        ]
        .concat()
    );
}

#[test]
fn a_world_older_than_every_target_is_opened_in_a_newer_release() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    let world = saves.java_world("Ancient", 3337, "1.19.4");
    let run = saves.install(&[&structure, "--world", &world, "--allow-cross-version"]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        [
            "error[E_CROSS_VERSION_DOWNGRADE]: `1.20.4/cottage.nbt` is built for DataVersion \
             3700 (1.20.4), newer than the world's 3337 (1.19.4)\n",
            DOWNGRADE_NOTE,
            "  fix: no target Cairn builds for is as old as the world; open the world in 1.20.4 \
             or newer, which upgrades it, and install again\n",
        ]
        .concat()
    );
}

#[test]
fn a_world_without_a_release_name_is_named_by_its_data_version() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.21.4");
    let world = saves.world_with("Unnamed", &java_level_dat(Some(3953), None));
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        [
            "error[E_CROSS_VERSION_DOWNGRADE]: `1.21.4/cottage.nbt` is built for DataVersion \
             4189 (1.21.4), newer than the world's 3953\n",
            DOWNGRADE_NOTE,
            "  fix: recompile its source for the world, with `--edition java --target 1.21`\n",
        ]
        .concat()
    );
}
