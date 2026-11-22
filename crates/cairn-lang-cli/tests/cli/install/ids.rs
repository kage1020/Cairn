//! The structure id each file installs under: its name and namespace, the ids the game does not
//! load, and the fix each refusal names.

use std::fs;
use std::path::Path;

use crate::install::{Saves, structure_at};

/// A world at 1.20.4 and the cottage built for it.
fn world_and_cottage() -> (Saves, String, String) {
    let saves = Saves::new();
    let structure = saves.compiled("java", "1.20.4");
    let world = saves.java_world("New World", 3700, "1.20.4");
    (saves, structure, world)
}

#[test]
fn a_name_holding_folders_installs_nested() {
    let (saves, structure, world) = world_and_cottage();
    let run = saves.install(&[&structure, "--world", &world, "--name", "houses/v1.2"]);
    let installed = structure_at(&world, "structures", "minecraft", &["houses", "v1.2.nbt"]);
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
fn a_file_name_loses_only_a_trailing_nbt() {
    let (saves, structure, world) = world_and_cottage();
    for copy in ["a.old", "a.nbt"] {
        fs::copy(saves.path().join(&structure), saves.path().join(copy)).expect("copy");
    }
    let run = saves.install(&["a.old", "a.nbt", "--world", &world, "--dry-run"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(run.stderr, "");
    assert_eq!(
        run.stdout,
        format!(
            "would install minecraft:a.old at {}\nwould install minecraft:a at {}\n",
            structure_at(&world, "structures", "minecraft", &["a.old.nbt"]).display(),
            structure_at(&world, "structures", "minecraft", &["a.nbt"]).display(),
        )
    );
}

#[test]
fn a_name_ending_in_nbt_is_refused() {
    let (saves, structure, world) = world_and_cottage();
    let run = saves.install(&[
        &structure,
        "--world",
        &world,
        "--name",
        "houses/cottage.nbt",
    ]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        "error[E_INVALID_STRUCTURE_ID]: `1.20.4/cottage.nbt` would install as \
         `minecraft:houses/cottage.nbt`, whose name ends in `.nbt`\n  note: the game adds `.nbt` \
         to a name itself, so the file would be `houses/cottage.nbt.nbt` and `/place template` \
         would name it with the `.nbt` kept\n  fix: --name houses/cottage\n"
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn an_id_the_game_does_not_load_is_refused_with_one_it_does() {
    let (saves, structure, world) = world_and_cottage();
    let character = "  note: a name holds only `a`-`z`, `0`-`9`, `_`, `-` and `.`, and `/` between \
                     its folders\n";
    let device = "  note: the game refuses a folder or file name Windows reserves for a device, \
                  such as `con` or `com1`, with or without an extension\n";
    for (name, refusal, note, fix) in [
        (
            "Oak Cottage",
            "the name `Oak Cottage` holds `O`",
            character,
            "--name oak_cottage",
        ),
        (
            "houses/con",
            "`con` in the name names a device Windows reserves",
            device,
            "--name houses/con_",
        ),
        (
            "aux.v2",
            "`aux.v2` in the name names a device Windows reserves",
            device,
            "--name aux_.v2",
        ),
        ("CON", "the name `CON` holds `C`", character, "--name con_"),
        (
            "houses/",
            "the name `houses/` has an empty folder or file name",
            "  note: each folder and the file name in a name holds at least one character\n",
            "--name houses",
        ),
        (
            "../../x",
            "the folder `..` in the name ends in `.`",
            "  note: the game refuses a folder name that ends in `.`, as `.` and `..` do\n",
            "--name x",
        ),
        (
            "",
            "the name is empty",
            "  note: a structure id is `namespace:name`, neither part empty\n",
            "install it under another name with `--name`",
        ),
    ] {
        let run = saves.install(&[&structure, "--world", &world, "--name", name]);
        assert_eq!(run.code, Some(1), "{name}");
        assert_eq!(run.stdout, "", "{name}");
        assert_eq!(
            run.stderr,
            format!(
                "error[E_INVALID_STRUCTURE_ID]: `1.20.4/cottage.nbt` would install as \
                 `minecraft:{name}`, which the game does not load: {refusal}\n{note}  fix: {fix}\n"
            ),
            "{name}"
        );
    }
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn each_file_whose_name_the_game_does_not_load_is_reported() {
    let (saves, structure, world) = world_and_cottage();
    for copy in ["Cottage.nbt", "House.nbt"] {
        fs::copy(saves.path().join(&structure), saves.path().join(copy)).expect("copy");
    }
    let run = saves.install(&["Cottage.nbt", "House.nbt", "--world", &world]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        "error[E_INVALID_STRUCTURE_ID]: `Cottage.nbt` would install as `minecraft:Cottage`, which \
         the game does not load: the name `Cottage` holds `C`\n  note: a name holds only `a`-`z`, \
         `0`-`9`, `_`, `-` and `.`, and `/` between its folders\n  fix: install it in a run of \
         its own with `--name cottage`, or rename the file\nerror[E_INVALID_STRUCTURE_ID]: \
         `House.nbt` would install as `minecraft:House`, which the game does not load: the name \
         `House` holds `H`\n  note: a name holds only `a`-`z`, `0`-`9`, `_`, `-` and `.`, and `/` \
         between its folders\n  fix: install it in a run of its own with `--name house`, or \
         rename the file\n"
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn a_namespace_the_game_does_not_load_is_reported_once_beside_each_name() {
    let (saves, structure, world) = world_and_cottage();
    fs::copy(saves.path().join(&structure), saves.path().join("Barn.nbt")).expect("copy");
    let run = saves.install(&[
        &structure,
        "Barn.nbt",
        "--world",
        &world,
        "--namespace",
        "My Pack",
    ]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        "error[E_INVALID_STRUCTURE_ID]: `--namespace My Pack` names no namespace the game loads: \
         the namespace `My Pack` holds `M`\n  note: a namespace holds only `a`-`z`, `0`-`9`, `_`, \
         `-` and `.`\n  fix: --namespace my_pack\nerror[E_INVALID_STRUCTURE_ID]: `Barn.nbt` would \
         install as `My Pack:Barn`, which the game does not load: the name `Barn` holds `B`\n  \
         note: a name holds only `a`-`z`, `0`-`9`, `_`, `-` and `.`, and `/` between its \
         folders\n  fix: install it in a run of its own with `--name barn`, or rename the file\n"
    );
    let run = saves.install(&[&structure, "--world", &world, "--namespace", ""]);
    assert_eq!(run.code, Some(1));
    assert_eq!(
        run.stderr,
        "error[E_INVALID_STRUCTURE_ID]: `--namespace ` names no namespace the game loads: the \
         namespace is empty\n  note: a structure id is `namespace:name`, neither part empty\n  \
         fix: --namespace minecraft\n"
    );
    assert!(!saves.path().join(&world).join("generated").exists());
}

#[test]
fn name_takes_one_file() {
    let (saves, structure, world) = world_and_cottage();
    let run = saves.install(&[&structure, "cottage.crn", "--world", &world, "--name", "x"]);
    assert_eq!(run.code, Some(2));
    assert_eq!(run.stdout, "");
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
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        "error: `1.20.4/cottage.nbt` and `1.21.4/cottage.nbt` would both install as \
         `minecraft:cottage`\n  fix: install one of them in a run of its own, with `--name`\n"
    );
}
