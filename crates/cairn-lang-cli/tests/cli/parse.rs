//! `cairn parse <file>` end to end, against every shipped example.

use tempfile::TempDir;

use crate::common::{cairn, crn_examples, examples_dir, write_source};

#[test]
fn every_example_parses_to_a_json_module() {
    for path in crn_examples() {
        let out = cairn("parse", &[path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{}: stderr={}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).expect("stdout utf-8");
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|err| panic!("{}: stdout is not JSON: {err}", path.display()));
        assert!(
            parsed.get("items").is_some(),
            "{}: a parsed module carries its items, got {stdout}",
            path.display(),
        );
    }
}

#[test]
fn debug_format_runs_successfully() {
    // The debug shape is `{:?}` of internal types, not a contract.
    let path = examples_dir().join("cottage.crn");
    let out = cairn("parse", &[path.to_str().unwrap(), "--format", "debug"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        !stdout.trim().is_empty(),
        "debug stdout should not be empty"
    );
}

#[test]
fn each_member_line_of_a_struct_or_def_body_carries_its_address() {
    let tmp = TempDir::new().expect("tempdir");
    let path = write_source(
        tmp.path(),
        "addresses.crn",
        "def porch size=3x2\n  floor\n\n\
         struct s size=9x7\n  \
         window side=front offset=6\n  \
         door[id=main] opened_by=sig.a\n  \
         level y=4\n    \
         window side=front offset=2\n  \
         @edition java\n    \
         raw_block mat=minecraft:stone at=1,1,1\n  \
         door id=main side=front at=center\n\n\
         site v:\n  place id=a use=s at=origin\n",
    );
    let out = cairn("parse", &[path.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let mut found = Vec::new();
    for item in parsed["items"].as_array().expect("items") {
        collect_addresses(&item["body"], &mut found);
    }
    let expected = [
        ("floor", Some("floor[0]")),
        ("window", Some("window@front[0]")),
        ("door", None),
        ("level", Some("level[0]")),
        ("window", Some("level[0].window@front[0]")),
        ("", None),
        ("raw_block", Some("raw_block[0]")),
        ("door", Some("door@front[0]")),
        ("place", None),
    ]
    .map(|(keyword, address)| (keyword.to_owned(), address.map(str::to_owned)));
    assert_eq!(found, expected);
}

#[test]
fn each_edit_line_lists_its_target_and_the_keys_it_sets() {
    let tmp = TempDir::new().expect("tempdir");
    let path = write_source(
        tmp.path(),
        "edits.crn",
        "edit window[class=vent][level=floor2] set shape=arch\n\n\
         struct s size=5x5\n  window side=front offset=1\n\n\
         edit level[1].window@front[0] set mat_slot=accent y=2\n",
    );
    let out = cairn("parse", &[path.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let ident = |key: &str, value: &str| serde_json::json!({"key": key, "value": {"kind": "Ident", "value": value}});
    assert_eq!(
        parsed["edits"],
        serde_json::json!([
            {
                "target": {
                    "kind": "Selector",
                    "keyword": "window",
                    "attrs": [ident("class", "vent"), ident("level", "floor2")],
                },
                "set": [ident("shape", "arch")],
            },
            {
                "target": {"kind": "Address", "address": "level[1].window@front[0]"},
                "set": [
                    ident("mat_slot", "accent"),
                    {"key": "y", "value": {"kind": "Int", "value": 2}},
                ],
            },
        ]),
    );
}

/// Each line of `body` and of the bodies nested in it, as `(keyword, address)`.
fn collect_addresses(body: &serde_json::Value, found: &mut Vec<(String, Option<String>)>) {
    for line in body.as_array().into_iter().flatten() {
        let keyword = line["keyword"].as_str().unwrap_or_default().to_owned();
        found.push((keyword, line["address"].as_str().map(str::to_owned)));
        collect_addresses(&line["children"], found);
    }
}

#[test]
fn the_json_keeps_each_object_in_the_order_its_fields_are_declared() {
    let tmp = TempDir::new().expect("tempdir");
    let path = write_source(
        tmp.path(),
        "order.crn",
        "struct s size=3x3\n  walls height=2\n",
    );
    let out = cairn("parse", &[path.to_str().unwrap()]);
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let compact: String = stdout.split_whitespace().collect();
    assert_eq!(
        compact,
        "{\"headers\":[],\"items\":[{\"kind\":\"Struct\",\"name\":\"s\",\"args\":[{\"key\":\"size\",\
         \"value\":{\"kind\":\"Size\",\"value\":{\"w\":3,\"h\":3}}}],\"body\":[{\"kind\":\"Generic\",\
         \"keyword\":\"walls\",\"args\":[{\"key\":\"height\",\"value\":{\"kind\":\"Int\",\"value\":2}}],\
         \"address\":\"walls[0]\"}]}]}",
    );
}
