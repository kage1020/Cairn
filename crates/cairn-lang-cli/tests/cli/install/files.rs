//! The structure files a run installs: the id each goes under, the file already there, and the run
//! that writes all of them or none.

use std::fs;
use std::path::Path;

use crate::install::{Saves, structure_at};

/// The note and fix of a file that is no Java structure.
const NOT_A_STRUCTURE: &str = "  note: a Java structure file is gzip NBT whose root holds `size`, \
     `blocks`, a `palette` and an integer `DataVersion`, as `cairn compile --edition java` writes \
     it\n  fix: name a structure file `cairn compile --edition java` wrote\n";

/// A world at 1.20.4 and the cottage built for it.
fn world_and_cottage() -> (Saves, String, String) {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    let world = saves.java_world("New World", 3700, "1.20.4");
    (saves, structure, world)
}

#[test]
fn a_structure_file_that_is_not_there() {
    let (saves, _, world) = world_and_cottage();
    let run = saves.install(&["missing.nbt", "--world", &world]);
    assert_eq!(run.code, Some(2));
    let not_found = std::io::Error::from(std::io::ErrorKind::NotFound);
    let reason = fs::read(saves.path().join("missing.nbt")).map_or_else(|err| err, |_| not_found);
    assert_eq!(
        run.stderr,
        format!(
            "error[E_STRUCTURE_UNREADABLE]: `missing.nbt` cannot be read: {reason}\n  fix: name \
             a structure file `cairn compile --edition java` wrote\n"
        )
    );
}

#[test]
fn a_file_that_is_no_structure() {
    let (saves, _, world) = world_and_cottage();
    let run = saves.install(&["cottage.crn", "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        [
            "error[E_STRUCTURE_UNREADABLE]: `cottage.crn` is not gzip-compressed\n",
            NOT_A_STRUCTURE
        ]
        .concat()
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn a_bedrock_mcstructure_is_refused() {
    let (saves, _, world) = world_and_cottage();
    let structure = saves.compiled("bedrock", "1.21.60");
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        "error[E_INSTALL_EDITION_UNSUPPORTED]: `1.21.60/cottage.mcstructure` is a Bedrock \
         `.mcstructure`, and `cairn install` installs Java structure files\n  fix: compile its \
         source with `--edition java --target 1.20.4` and install the `.nbt` it writes\n"
    );
}

#[test]
fn a_structure_already_there_is_kept_unless_replace_is_given() {
    let (saves, structure, world) = world_and_cottage();
    let installed = structure_at(&world, "minecraft", &["cottage.nbt"]);
    fs::create_dir_all(saves.path().join(installed.parent().expect("folder"))).expect("mkdir");
    fs::write(saves.path().join(&installed), b"saved by a structure block").expect("write");
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        format!(
            "error[E_STRUCTURE_EXISTS]: `1.20.4/cottage.nbt` would install as \
             `minecraft:cottage`, and the world already has a structure file at `{}`\n  fix: \
             pass `--replace` to overwrite it, or `--name` to install under another name\n",
            installed.display(),
        )
    );
    assert_eq!(
        saves.read(&installed).as_deref(),
        Some(&b"saved by a structure block"[..])
    );
    let run = saves.install(&[&structure, "--world", &world, "--replace"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        run.stdout,
        format!(
            "installed minecraft:cottage at {}, replacing the file there\n",
            installed.display()
        )
    );
    assert_eq!(saves.read(&installed), saves.read(Path::new(&structure)));
}

#[test]
fn a_folder_where_the_file_goes_is_refused_even_with_replace() {
    let (saves, structure, world) = world_and_cottage();
    let installed = structure_at(&world, "minecraft", &["cottage.nbt"]);
    fs::create_dir_all(saves.path().join(&installed)).expect("mkdir");
    let run = saves.install(&[&structure, "--world", &world, "--replace"]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        format!(
            "error[E_STRUCTURE_EXISTS]: `1.20.4/cottage.nbt` would install as \
             `minecraft:cottage`, at `{}`, which is a folder\n  fix: move that folder aside, or \
             install under another id\n",
            installed.display(),
        )
    );
}

#[test]
fn a_dry_run_says_where_each_file_would_go_and_writes_nothing() {
    let (saves, structure, world) = world_and_cottage();
    let run = saves.install(&[&structure, "--world", &world, "--dry-run"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(run.stderr, "");
    assert_eq!(
        run.stdout,
        format!(
            "would install minecraft:cottage at {}\n",
            structure_at(&world, "minecraft", &["cottage.nbt"]).display()
        )
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn several_files_install_together_or_not_at_all() {
    let (saves, structure, world) = world_and_cottage();
    for copy in ["a.nbt", "b.nbt"] {
        fs::copy(saves.path().join(&structure), saves.path().join(copy)).expect("copy");
    }
    let run = saves.install(&["a.nbt", "cottage.crn", "b.nbt", "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        [
            "error[E_STRUCTURE_UNREADABLE]: `cottage.crn` is not gzip-compressed\n",
            NOT_A_STRUCTURE
        ]
        .concat()
    );
    assert!(!saves.path().join(&world).join("generated").exists());
    let run = saves.install(&[
        "a.nbt",
        "b.nbt",
        "--world",
        &world,
        "--namespace",
        "village",
    ]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        run.stdout,
        format!(
            "installed village:a at {}\ninstalled village:b at {}\n",
            structure_at(&world, "village", &["a.nbt"]).display(),
            structure_at(&world, "village", &["b.nbt"]).display(),
        )
    );
}

#[test]
fn a_name_holding_folders_installs_nested() {
    let (saves, structure, world) = world_and_cottage();
    let run = saves.install(&[&structure, "--world", &world, "--name", "houses/v1.2"]);
    let installed = structure_at(&world, "minecraft", &["houses", "v1.2.nbt"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        run.stdout,
        format!(
            "installed minecraft:houses/v1.2 at {}\n",
            installed.display()
        )
    );
    assert_eq!(saves.read(&installed), saves.read(Path::new(&structure)));
}

#[test]
fn an_id_the_game_does_not_load_is_refused_with_one_it_does() {
    let (saves, structure, world) = world_and_cottage();
    let run = saves.install(&[&structure, "--world", &world, "--name", "Oak Cottage"]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        "error[E_INVALID_STRUCTURE_ID]: `1.20.4/cottage.nbt` would install as \
         `minecraft:Oak Cottage`, which the game does not load: the name `Oak Cottage` holds \
         `O`\n  note: a name holds only `a`-`z`, `0`-`9`, `_`, `-` and `.`, and `/` between its \
         folders\n  fix: --name oak_cottage\n"
    );
    let run = saves.install(&[&structure, "--world", &world, "--name", "houses/con"]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        "error[E_INVALID_STRUCTURE_ID]: `1.20.4/cottage.nbt` would install as \
         `minecraft:houses/con`, which the game does not load: `con` in the name names a device \
         Windows reserves\n  note: the game refuses a folder or file name Windows reserves for a \
         device, such as `con` or `com1`, with or without an extension\n  fix: --name \
         houses/con_\n"
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn a_namespace_the_game_does_not_load_is_reported_once() {
    let (saves, structure, world) = world_and_cottage();
    fs::copy(saves.path().join(&structure), saves.path().join("b.nbt")).expect("copy");
    let run = saves.install(&[
        &structure,
        "b.nbt",
        "--world",
        &world,
        "--namespace",
        "My Pack",
    ]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        "error[E_INVALID_STRUCTURE_ID]: `--namespace My Pack` names no namespace the game loads: \
         the namespace `My Pack` holds `M`\n  note: a namespace holds only `a`-`z`, `0`-`9`, `_`, \
         `-` and `.`\n  fix: --namespace my_pack\n"
    );
}

#[test]
fn name_takes_one_file() {
    let (saves, structure, world) = world_and_cottage();
    let run = saves.install(&[&structure, "cottage.crn", "--world", &world, "--name", "x"]);
    assert_eq!(run.code, Some(2));
    assert_eq!(
        run.stderr,
        "error: `--name` names one structure file, and 2 were given\n  fix: install each file in \
         a run of its own with `--name`, or leave `--name` out to install each under its file \
         name\n"
    );
}

#[test]
fn two_files_under_one_id_are_refused() {
    let (saves, structure, world) = world_and_cottage();
    let other = saves.compiled("java", "1.21.4");
    let run = saves.install(&[&structure, &other, "--world", &world]);
    assert_eq!(run.code, Some(2));
    assert_eq!(
        run.stderr,
        "error: `1.20.4/cottage.nbt` and `1.21.4/cottage.nbt` would both install as \
         `minecraft:cottage`\n  fix: install one of them in a run of its own, with `--name`\n"
    );
}
