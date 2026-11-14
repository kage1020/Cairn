//! The exports as a JavaScript caller meets them, run in Node by `wasm-pack test --node`.

#![cfg(target_arch = "wasm32")]

use cairn_lang_core::{CAIRN_VERSION, Edition};
use cairn_lang_formats::build::gate::requested_editions;
use cairn_lang_wasm::{cairn_version_js, compile, info, report, targets};
use js_sys::{Array, Error, JSON, Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

/// A struct with one painted floor, so a refusal cannot be mistaken for an empty build.
const BUILD: &str =
    "theme t:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=floor\n";

fn get(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &key.into()).expect("an export answers a plain object")
}

fn as_json(value: &JsValue) -> String {
    JSON::stringify(value)
        .expect("an export answers JSON-able data")
        .into()
}

fn thrown_message(thrown: JsValue) -> String {
    thrown
        .dyn_into::<Error>()
        .expect("an export throws an `Error`")
        .message()
        .into()
}

#[wasm_bindgen_test]
fn cairn_version_is_the_release_version() {
    assert_eq!(cairn_version_js(), CAIRN_VERSION);
}

#[wasm_bindgen_test]
fn compile_answers_the_report_with_each_structure_file_as_a_named_byte_array() {
    for (source, edition, name) in [
        (BUILD, "java", "s.nbt"),
        (BUILD, "bedrock", "s.mcstructure"),
    ] {
        let parsed: Edition = edition.parse().expect("an edition");
        let expected = report::compile(source, parsed, "latest");
        let exported = compile(source, edition, "latest").expect("an edition");
        let artifacts = Array::from(&get(&exported, "artifacts"));
        assert_eq!(artifacts.length(), 1);
        let artifact = artifacts.get(0);
        assert_eq!(get(&artifact, "name").as_string().as_deref(), Some(name));
        let bytes: Uint8Array = get(&artifact, "bytes")
            .dyn_into()
            .expect("an artifact's bytes are a `Uint8Array`");
        assert_eq!(bytes.to_vec(), expected.artifacts[0].bytes);
        let object: &Object = exported.unchecked_ref();
        Reflect::delete_property(object, &"artifacts".into()).expect("a plain object");
        assert_eq!(
            as_json(&exported),
            serde_json::to_string(&expected).expect("serialize"),
        );
    }
}

#[wasm_bindgen_test]
fn a_refused_build_answers_its_findings_and_no_artifact() {
    let exported = compile("struct s size=\n", "java", "latest").expect("an edition");
    assert_eq!(get(&exported, "ok"), JsValue::FALSE);
    assert_eq!(Array::from(&get(&exported, "artifacts")).length(), 0);
    let diagnostics = Array::from(&get(&exported, "diagnostics"));
    assert_eq!(diagnostics.length(), 1);
    assert_eq!(
        get(&diagnostics.get(0), "code").as_string().as_deref(),
        Some("E_PARSE"),
    );
    assert_eq!(get(&exported, "ir"), JsValue::NULL);
}

#[wasm_bindgen_test]
fn a_name_that_is_not_an_edition_throws_naming_the_valid_ones() {
    let message = "unknown edition `Java`. Valid: java, bedrock. Fix: pass one of the supported \
                   edition names";
    let thrown = compile(BUILD, "Java", "latest").expect_err("`Java` is not an edition");
    assert_eq!(thrown_message(thrown), message);
    let thrown = targets("Java").expect_err("`Java` is not an edition");
    assert_eq!(thrown_message(thrown), message);
    let thrown = info(BUILD, vec!["java".to_owned(), "Java".to_owned()])
        .expect_err("`Java` is not an edition");
    assert_eq!(thrown_message(thrown), message);
}

#[wasm_bindgen_test]
fn info_answers_the_report_and_throws_on_an_empty_editions_list() {
    let asked = vec!["java".to_owned(), "bedrock".to_owned()];
    let expected = report::info(BUILD, &requested_editions(&asked).expect("editions"));
    let exported = info(BUILD, asked).expect("editions");
    assert_eq!(
        as_json(&exported),
        serde_json::to_string(&expected).expect("serialize"),
    );
    let thrown = info(BUILD, Vec::new()).expect_err("no edition is asked about");
    assert_eq!(
        thrown_message(thrown),
        "`editions` names no edition. Valid: java, bedrock. Fix: pass at least one edition name",
    );
}

#[wasm_bindgen_test]
fn targets_answers_the_report() {
    let exported = targets("bedrock").expect("an edition");
    assert_eq!(
        as_json(&exported),
        serde_json::to_string(&report::targets(Edition::Bedrock)).expect("serialize"),
    );
}
