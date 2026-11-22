//! The structure files a run installs: the file already at an id's path, the world that cannot
//! take one, and the run that writes all of them or none.

use std::fs;
use std::path::Path;

use crate::install::{Saves, structure_at};

/// The note and fix of a file that is no Java structure.
const NOT_A_STRUCTURE: &str = "  note: a Java structure file is gzip NBT whose root holds `size`, \
     `blocks`, a `palette` or `palettes` list and an integer `DataVersion`, as `cairn compile \
     --edition java` writes it\n  fix: name a structure file `cairn compile --edition java` \
     wrote\n";

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
    assert_eq!(run.stdout, "");
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
    assert_eq!(run.stdout, "");
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
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        "error[E_INSTALL_EDITION_UNSUPPORTED]: `1.21.60/cottage.mcstructure` is a Bedrock \
         `.mcstructure`, and `cairn install` installs Java structure files\n  fix: compile its \
         source with `--edition java --target 1.20.4` and install the `.nbt` it writes\n"
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn a_structure_already_there_is_kept_unless_replace_is_given() {
    let (saves, structure, world) = world_and_cottage();
    let installed = structure_at(&world, "structures", "minecraft", &["cottage.nbt"]);
    fs::create_dir_all(saves.path().join(installed.parent().expect("folder"))).expect("mkdir");
    fs::write(saves.path().join(&installed), b"saved by a structure block").expect("write");
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
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
    let installed = structure_at(&world, "structures", "minecraft", &["cottage.nbt"]);
    fs::create_dir_all(saves.path().join(&installed)).expect("mkdir");
    let run = saves.install(&[&structure, "--world", &world, "--replace"]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        format!(
            "error[E_STRUCTURE_EXISTS]: `1.20.4/cottage.nbt` would install as \
             `minecraft:cottage`, at `{}`, which is a folder\n  fix: move that folder aside, or \
             install under another id\n",
            installed.display(),
        )
    );
    let left = fs::read_dir(saves.path().join(&installed)).expect("folder");
    assert_eq!(left.count(), 0);
}

#[test]
fn a_file_the_install_would_write_through_is_kept_and_refused() {
    let (saves, structure, world) = world_and_cottage();
    let installed = structure_at(&world, "structures", "minecraft", &["cottage.nbt"]);
    fs::create_dir_all(saves.path().join(installed.parent().expect("folder"))).expect("mkdir");
    for (scratch, replacing) in [("cottage.nbt.bak", true), ("cottage.nbt.tmp", false)] {
        let at = installed.with_file_name(scratch);
        fs::write(saves.path().join(&at), b"the player's own").expect("write");
        let mut args = vec![structure.as_str(), "--world", &world];
        if replacing {
            fs::write(saves.path().join(&installed), b"saved by a structure block").expect("write");
            args.push("--replace");
        }
        let run = saves.install(&args);
        assert_eq!(run.code, Some(1), "{scratch}");
        assert_eq!(run.stdout, "", "{scratch}");
        assert_eq!(
            run.stderr,
            format!(
                "error[E_STRUCTURE_EXISTS]: `1.20.4/cottage.nbt` would install as \
                 `minecraft:cottage`, and the world has a file at `{}`, which the install writes \
                 through\n  note: the file is written beside its path with `.tmp` added, and one \
                 it replaces is moved aside with `.bak` added\n  fix: move that file aside, or \
                 install under another id\n",
                at.display(),
            ),
            "{scratch}"
        );
        assert_eq!(saves.read(&at).as_deref(), Some(&b"the player's own"[..]));
        fs::remove_file(saves.path().join(&at)).expect("rm");
        let _ = fs::remove_file(saves.path().join(&installed));
    }
}

#[test]
fn a_file_where_a_folder_goes_is_refused_by_a_dry_run_too() {
    let (saves, structure, world) = world_and_cottage();
    let generated = Path::new(&world).join("generated");
    fs::write(saves.path().join(&generated), b"").expect("write");
    for dry_run in [true, false] {
        let mut args = vec![structure.as_str(), "--world", &world];
        args.extend(dry_run.then_some("--dry-run"));
        let run = saves.install(&args);
        assert_eq!(run.code, Some(1), "{dry_run}");
        assert_eq!(run.stdout, "", "{dry_run}");
        assert_eq!(
            run.stderr,
            format!(
                "error[E_WORLD_UNWRITABLE]: `1.20.4/cottage.nbt` would install as \
                 `minecraft:cottage`, at `{}`, and `{}` is a file, not the folder that path goes \
                 through\n  fix: move that file aside, or install under another id\n",
                structure_at(&world, "structures", "minecraft", &["cottage.nbt"]).display(),
                generated.display(),
            ),
            "{dry_run}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_write_the_world_refuses_names_the_folder_and_its_fix() {
    let (saves, structure, world) = world_and_cottage();
    let generated = saves.path().join(&world).join("generated");
    std::os::unix::fs::symlink("/proc", &generated).expect("symlink");
    let reason = fs::create_dir(generated.join("minecraft")).expect_err("refused");
    let run = saves.install(&[&structure, "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        format!(
            "error[E_WORLD_UNWRITABLE]: creating `{}`: {reason}\n  fix: make `{world}` writable, \
             with room for the files, then install again\n",
            Path::new(&world)
                .join("generated")
                .join("minecraft")
                .display(),
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
            structure_at(&world, "structures", "minecraft", &["cottage.nbt"]).display()
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
    assert_eq!(run.stdout, "");
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
            structure_at(&world, "structures", "village", &["a.nbt"]).display(),
            structure_at(&world, "structures", "village", &["b.nbt"]).display(),
        )
    );
}

#[test]
fn every_refusal_of_a_run_is_reported_and_the_worst_sets_the_code() {
    let (saves, structure, world) = world_and_cottage();
    fs::copy(
        saves.path().join("cottage.crn"),
        saves.path().join("junk.nbt"),
    )
    .expect("copy");
    let not_found = std::io::Error::from(std::io::ErrorKind::NotFound);
    let reason = fs::read(saves.path().join("missing.nbt")).map_or_else(|err| err, |_| not_found);
    let missing = format!(
        "error[E_STRUCTURE_UNREADABLE]: `missing.nbt` cannot be read: {reason}\n  fix: name a \
         structure file `cairn compile --edition java` wrote\n"
    );
    let junk = format!(
        "error[E_STRUCTURE_UNREADABLE]: `junk.nbt` is not gzip-compressed\n{NOT_A_STRUCTURE}"
    );
    for files in [["missing.nbt", "junk.nbt"], ["junk.nbt", "missing.nbt"]] {
        let reports = files.map(|file| if file == "junk.nbt" { &junk } else { &missing });
        let run = saves.install(&[files[0], files[1], "--world", &world]);
        assert_eq!(run.code, Some(2), "{files:?}");
        assert_eq!(run.stdout, "", "{files:?}");
        assert_eq!(
            run.stderr,
            [reports[0].as_str(), reports[1]].concat(),
            "{files:?}"
        );
    }
    let newer = saves.java_world("Newer", 3953, "1.21");
    fs::copy(
        saves.path().join(&structure),
        saves.path().join("Cottage.nbt"),
    )
    .expect("copy");
    let run = saves.install(&["Cottage.nbt", "--world", &newer]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        "error[E_INVALID_STRUCTURE_ID]: `Cottage.nbt` would install as `minecraft:Cottage`, which \
         the game does not load: the name `Cottage` holds `C`\n  note: a name holds only `a`-`z`, \
         `0`-`9`, `_`, `-` and `.`, and `/` between its folders\n  fix: --name \
         cottage\nerror[E_CROSS_VERSION_UPGRADE]: `Cottage.nbt` is built for DataVersion 3700 \
         (1.20.4), older than the world's 3953 (1.21)\n  note: it keeps DataVersion 3700, so the \
         game upgrades it through DataFixerUpper as it loads, which can lose items, signs, \
         paintings and block entities\n  fix: recompile its source for the world, with \
         `--edition java --target 1.21`, or install this file as it is with \
         `--allow-cross-version`\n"
    );
    assert!(!saves.path().join(&newer).join("generated").exists());
}
