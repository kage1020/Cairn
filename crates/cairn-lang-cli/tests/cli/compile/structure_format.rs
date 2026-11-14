//! `cairn compile --structure-format`: which files each Java format writes, and what stays the
//! same whichever is picked.

use std::fs;
use std::process::Output;

use cairn_lang_formats::litematic::read_litematic;

use crate::common::{Fixture, cairn};

/// A struct, and a site of two placements joined by a walkway.
const YARD: &str = "theme t:\n  slot wall -> @cobblestone\n\n\
                    def hut size=5x5:\n  walls id=w mat_slot=wall height=3\n  \
                    door  id=p side=front at=center\n\n\
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
fn the_lockfile_and_the_bytes_do_not_vary_from_one_litematic_build_to_the_next() {
    let nbt = Fixture::new(YARD);
    compile_with(&nbt, &[]);
    let first = Fixture::new(YARD);
    compile_with(&first, &["--structure-format", "litematic"]);
    let second = Fixture::new(YARD);
    compile_with(&second, &["--structure-format", "litematic"]);

    let lock = |fixture: &Fixture| fs::read_to_string(fixture.lock()).expect("read lock");
    assert_eq!(
        lock(&first),
        lock(&nbt),
        "the format leaves the lockfile alone"
    );
    for stem in ["shed", "yard"] {
        let file = format!("{stem}.litematic");
        assert_eq!(
            fs::read(first.out().join(&file)).expect("read"),
            fs::read(second.out().join(&file)).expect("read"),
            "{file}"
        );
    }
}

#[test]
fn a_site_named_after_a_struct_collides_only_as_one_litematic() {
    let source = YARD.replace("struct shed", "struct yard");
    let nbt = Fixture::new(&source);
    compile_with(&nbt, &[]);

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
             `site::yard`\n",
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
