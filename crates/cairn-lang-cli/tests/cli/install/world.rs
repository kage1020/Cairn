//! The world `--world` names: found by its `level.dat`, read for its version, and refused when it
//! is no Java world.

use std::path::Path;

use crate::install::{Saves, bedrock_level_dat, java_level_dat};

/// The note and fix of a `level.dat` that does not decode.
const RESTORE: &str = "  note: a Java world's `level.dat` is gzip NBT, and the save before the \
     last is kept beside it as `level.dat_old`\n  fix: restore `level.dat` from `level.dat_old`, \
     then install again\n";

#[test]
fn a_folder_of_worlds_names_the_ones_inside_it() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    saves.java_world("New World", 3700, "1.20.4");
    saves.java_world("Second", 3700, "1.20.4");
    let run = saves.install(&[&structure, "--world", "saves"]);
    assert_eq!(run.code, Some(2));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        format!(
            "error[E_WORLD_UNREADABLE]: `saves` holds no `level.dat`, so it is no world's \
             folder\n  note: it holds the worlds `New World`, `Second`\n  fix: name the world's \
             own folder, as `--world \"{}\"`\n",
            Path::new("saves").join("New World").display(),
        )
    );
}

#[test]
fn a_misspelled_world_is_matched_to_the_one_beside_it() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    saves.java_world("New World", 3700, "1.20.4");
    let run = saves.install(&[&structure, "--world", "saves/New Wrld"]);
    assert_eq!(run.code, Some(2));
    assert_eq!(
        run.stderr,
        format!(
            "error[E_WORLD_UNREADABLE]: `saves/New Wrld` holds no `level.dat`, so it is no \
             world's folder\n  note: did you mean `--world \"{}\"`?\n  fix: name the folder that \
             holds the world's `level.dat`\n",
            Path::new("saves").join("New World").display(),
        )
    );
}

#[test]
fn a_folder_with_no_world_near_it_is_reported_before_any_structure_file() {
    let saves = Saves::new();
    let run = saves.install(&["missing.nbt", "--world", "nowhere"]);
    assert_eq!(run.code, Some(2));
    assert_eq!(
        run.stderr,
        "error[E_WORLD_UNREADABLE]: `nowhere` holds no `level.dat`, so it is no world's folder\n  \
         note: a world's folder, inside the game's `saves` folder, holds its `level.dat`\n  fix: \
         name the folder that holds the world's `level.dat`\n"
    );
}

#[test]
fn a_level_dat_that_is_not_gzip() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    let world = saves.world_with("Broken", b"not a level.dat");
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        [
            "error[E_WORLD_UNREADABLE]: `saves/Broken` has a `level.dat` that is not \
             gzip-compressed\n",
            RESTORE,
        ]
        .concat()
    );
}

#[test]
fn a_level_dat_that_records_no_data_version() {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    let world = saves.world_with("Before 1.9", &java_level_dat(None, None));
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        "error[E_WORLD_UNREADABLE]: `saves/Before 1.9` has a `level.dat` with no integer \
         `Data.DataVersion`\n  note: a world last saved before 1.9 records none\n  fix: open the \
         world in a current release and save it, then install again\n"
    );
}

#[test]
fn a_bedrock_world_is_refused() {
    let saves = Saves::new();
    let structure = saves.compiled("bedrock", "1.21.60");
    let world = saves.world_with("Bedrock", &bedrock_level_dat());
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        "error[E_INSTALL_EDITION_UNSUPPORTED]: `saves/Bedrock` is a Bedrock world, last opened \
         with 1.21.60\n  note: `cairn install` installs into Java worlds; a Bedrock world keeps \
         the structures it saves in its database, and loads none from files in its folder\n  \
         fix: compile with `--edition bedrock` and put the `.mcstructure` in a behavior pack's \
         `structures` folder, which `/structure load` reads\n"
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}
