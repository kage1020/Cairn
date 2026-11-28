//! `cairn compile --structure-format`: which files each Java format writes, and what stays the
//! same whichever is picked.

use std::fs;
use std::process::Output;

use cairn_lang_formats::litematic::read_litematic;
use cairn_lang_formats::schem::read_schem;

use crate::common::{Fixture, cairn};

/// A struct, and a site of two placements joined by a walkway.
const YARD: &str = "theme t:\n  slot wall -> @cobblestone\n\n\
                    def hut size=5x5:\n  floor mat_slot=wall\n  \
                    walls id=w mat_slot=wall height=3\n  door  id=p side=front at=center\n\n\
                    struct shed size=3x3\n  walls mat_slot=wall height=2\n\n\
                    site yard:\n  place id=a use=hut theme=t at=origin\n  \
                    place id=b use=hut theme=t east_of=a gap=4\n  \
                    connect a.p to b.p path=@gravel\n";

/// `cairn compile` of the fixture's source for Java 1.21.4, with `extra` arguments.
fn compile_with(fixture: &Fixture, extra: &[&str]) -> Output {
    let source = fixture.source();
    let out = fixture.out();
    let lock = fixture.lock();
    let mut args = vec![
        source.to_str().expect("utf-8 path"),
        "--edition",
        "java",
        "--target",
        "1.21.4",
        "--out",
        out.to_str().expect("utf-8 path"),
        "--lock",
        lock.to_str().expect("utf-8 path"),
    ];
    args.extend(extra);
    let result = cairn("compile", &args);
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}

#[test]
fn litematic_writes_a_file_per_struct_and_one_per_site() {
    let fixture = Fixture::new(YARD);
    compile_with(&fixture, &["--structure-format", "litematic"]);
    assert_eq!(
        fixture.artifacts(),
        ["s.crn.lock", "shed.litematic", "yard.litematic"]
    );
    let regions = |stem: &str| -> Vec<String> {
        let bytes = fs::read(fixture.out().join(format!("{stem}.litematic"))).expect("read");
        read_litematic(bytes.as_slice())
            .expect("the artifact reads as a litematic")
            .regions
            .into_iter()
            .map(|region| region.name)
            .collect()
    };
    assert_eq!(regions("shed"), ["shed"]);
    assert_eq!(regions("yard"), ["a", "b", "yard_walkway_a_p__b_p"]);
}

#[test]
fn schem_writes_a_file_per_struct_placement_and_walkway_at_its_offset() {
    let fixture = Fixture::new(YARD);
    compile_with(&fixture, &["--structure-format", "schem"]);
    let artifacts = fixture.artifacts();
    assert_eq!(
        artifacts,
        [
            "a.schem",
            "b.schem",
            "s.crn.lock",
            "shed.schem",
            "yard_walkway_a_p__b_p.schem"
        ]
    );
    let offsets: Vec<(&str, (i32, i32, i32))> = artifacts
        .iter()
        .filter(|name| *name != "s.crn.lock")
        .map(|name| {
            let bytes = fs::read(fixture.out().join(name)).expect("read");
            let schematic = read_schem(bytes.as_slice()).expect("the artifact reads as a schem");
            (name.as_str(), schematic.offset)
        })
        .collect();
    assert_eq!(
        offsets,
        [
            ("a.schem", (0, 0, 0)),
            ("b.schem", (9, 0, 0)),
            ("shed.schem", (0, 0, 0)),
            ("yard_walkway_a_p__b_p.schem", (2, 0, 5)),
        ]
    );
}

#[test]
fn nbt_is_the_default_and_naming_it_writes_the_same_files() {
    let unnamed = Fixture::new(YARD);
    compile_with(&unnamed, &[]);
    let named = Fixture::new(YARD);
    compile_with(&named, &["--structure-format", "nbt"]);
    assert_eq!(
        unnamed.artifacts(),
        [
            "a.nbt",
            "b.nbt",
            "s.crn.lock",
            "shed.nbt",
            "yard_walkway_a_p__b_p.nbt"
        ]
    );
    assert_eq!(named.artifacts(), unnamed.artifacts());
    for name in unnamed.artifacts() {
        let (left, right) = if name == "s.crn.lock" {
            (unnamed.lock(), named.lock())
        } else {
            (unnamed.out().join(&name), named.out().join(&name))
        };
        assert_eq!(fs::read(left).ok(), fs::read(right).ok(), "{name}");
    }
}

#[test]
fn neither_format_changes_the_lockfile_or_varies_from_one_build_to_the_next() {
    let nbt = Fixture::new(YARD);
    compile_with(&nbt, &[]);
    let lock = |fixture: &Fixture| fs::read_to_string(fixture.lock()).expect("read lock");
    for (format, stems) in [
        ("litematic", &["shed", "yard"][..]),
        ("schem", &["a", "b", "shed", "yard_walkway_a_p__b_p"][..]),
    ] {
        let first = Fixture::new(YARD);
        compile_with(&first, &["--structure-format", format]);
        let second = Fixture::new(YARD);
        compile_with(&second, &["--structure-format", format]);
        assert_eq!(
            lock(&first),
            lock(&nbt),
            "{format} leaves the lockfile alone"
        );
        for stem in stems {
            let file = format!("{stem}.{format}");
            assert_eq!(
                fs::read(first.out().join(&file)).expect("read"),
                fs::read(second.out().join(&file)).expect("read"),
                "{file}"
            );
        }
    }
}

#[test]
fn a_site_named_after_a_struct_collides_only_as_one_litematic() {
    let source = YARD.replace("struct shed", "struct yard");
    for format in ["nbt", "schem"] {
        compile_with(&Fixture::new(&source), &["--structure-format", format]);
    }

    let litematic = Fixture::new(&source);
    let out = litematic.out();
    let result = cairn(
        "compile",
        &[
            litematic.source().to_str().expect("utf-8 path"),
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--out",
            out.to_str().expect("utf-8 path"),
            "--structure-format",
            "litematic",
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&result.stderr),
        format!(
            "error: output filename `{}` collides between scopes `struct::yard` and \
             `site::yard`\n  note: rename the site or the struct, or compile with \
             `--structure-format nbt`, which writes the site as a file per placement\n",
            out.join("yard.litematic").display(),
        ),
    );
    assert!(
        litematic.artifacts().is_empty(),
        "{:?}",
        litematic.artifacts()
    );
}

#[test]
fn a_site_named_like_another_site_or_a_struct_but_for_case_is_refused_as_litematic_on_every_host() {
    let hut =
        "theme t:\n  slot floor -> @oak_planks\n\ndef hut size=3x3:\n  floor mat_slot=floor\n\n";
    let site =
        |name: &str, id: &str| format!("site {name}:\n  place id={id} use=hut theme=t at=origin\n");
    for (second, scope, fix) in [
        (
            site("S", "b"),
            "site::S",
            "rename one of the sites, or compile with `--structure-format nbt`, which writes each \
             site as a file per placement",
        ),
        (
            "struct S size=3x3\n  floor mat_slot=floor\n".to_owned(),
            "struct::S",
            "rename the site or the struct, or compile with `--structure-format nbt`, which \
             writes the site as a file per placement",
        ),
    ] {
        let source = format!("{hut}{}{second}", site("s", "a"));
        compile_with(&Fixture::new(&source), &[]);
        let fixture = Fixture::new(&source);
        let out = fixture.out();
        let result = cairn(
            "compile",
            &[
                fixture.source().to_str().expect("utf-8 path"),
                "--edition",
                "java",
                "--target",
                "1.21.4",
                "--out",
                out.to_str().expect("utf-8 path"),
                "--structure-format",
                "litematic",
            ],
        );
        let (first_path, path) = if scope == "site::S" {
            (out.join("s.litematic"), out.join("S.litematic"))
        } else {
            (out.join("S.litematic"), out.join("s.litematic"))
        };
        let (first, second) = if scope == "site::S" {
            ("site::s", scope)
        } else {
            (scope, "site::s")
        };
        assert_eq!(
            (
                result.status.code(),
                String::from_utf8_lossy(&result.stderr)
            ),
            (
                Some(1),
                format!(
                    "error: output filename `{}` collides between scopes `{first}` and \
                     `{second}`\n  note: `{}` and `{}` differ only in case, which makes them one \
                     file on the case-insensitive file systems macOS and Windows use by default; \
                     rename one of the scopes so their names differ by more than case\n  note: \
                     {fix}\n",
                    path.display(),
                    first_path.display(),
                    path.display(),
                )
                .into()
            ),
            "{scope}"
        );
        assert!(fixture.artifacts().is_empty(), "{:?}", fixture.artifacts());
    }
}

#[test]
fn a_struct_wider_than_a_schem_extent_is_refused_with_a_format_that_holds_it() {
    let source = "theme t:\n  slot floor -> @stone\n\n\
                  struct strip size=65536x1\n  floor mat_slot=floor\n";
    let fixture = Fixture::new(source);
    let out = fixture.out();
    let result = cairn(
        "compile",
        &[
            fixture.source().to_str().expect("utf-8 path"),
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--out",
            out.to_str().expect("utf-8 path"),
            "--structure-format",
            "schem",
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&result.stderr),
        "error: checking `struct::strip`: the array is 65536 blocks along x, past the 65535 \
         `Width` holds\n  note: split the structure along that axis, or compile with \
         `--structure-format nbt`, which stores each extent as an int\n",
    );
    assert!(fixture.artifacts().is_empty(), "{:?}", fixture.artifacts());
    compile_with(&fixture, &["--structure-format", "nbt"]);
}

#[test]
fn a_raw_block_compound_without_a_string_id_is_refused_as_a_schem_and_written_as_an_nbt() {
    let source = "struct store size=3x3\n  @edition java\n    \
                  raw_block mat=minecraft:chest at=1,0,1 nbt={Items:[]}\n";
    let fixture = Fixture::new(source);
    let out = fixture.out();
    let result = cairn(
        "compile",
        &[
            fixture.source().to_str().expect("utf-8 path"),
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--out",
            out.to_str().expect("utf-8 path"),
            "--structure-format",
            "schem",
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&result.stderr),
        "error: checking `struct::store`: the block entity at (1, 0, 1) has no string `id`, which \
         a `.schem` file names it by\n  note: name the block entity in its `raw_block`'s `nbt=` \
         with a string `id`, such as `id:\"minecraft:chest\"`\n",
    );
    assert!(fixture.artifacts().is_empty(), "{:?}", fixture.artifacts());
    compile_with(&fixture, &["--structure-format", "nbt"]);
}

#[test]
fn litematic_and_schem_note_the_entities_they_leave_out_and_nbt_writes_them() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=5x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                  armor_stand at=2,1,2\n  villager at=3,1,2\n";
    for (format, stderr) in [
        (
            "litematic",
            "note: a `.litematic` file writes no entities, so the 2 entities the build places are \
             left out; `--structure-format nbt` writes them\n",
        ),
        (
            "schem",
            "note: a `.schem` file writes no entities, so the 2 entities the build places are left \
             out; `--structure-format nbt` writes them\n",
        ),
        ("nbt", ""),
    ] {
        let fixture = Fixture::new(source);
        let run = compile_with(&fixture, &["--structure-format", format]);
        assert_eq!(String::from_utf8_lossy(&run.stderr), stderr, "{format}");
    }
}

#[test]
fn a_structure_format_for_bedrock_is_refused_before_the_file_is_read() {
    let result = cairn(
        "compile",
        &[
            "absent.crn",
            "--edition",
            "bedrock",
            "--structure-format",
            "nbt",
        ],
    );
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&result.stderr),
        "error: `--structure-format` picks a Java format; `--edition bedrock` writes only \
         `.mcstructure`\n  note: drop `--structure-format`, or compile with `--edition java`\n",
    );
}
