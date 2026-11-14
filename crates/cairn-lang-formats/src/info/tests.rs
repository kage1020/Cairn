use std::path::Path;

use cairn_lang_core::Edition;
use serde_json::json;

use super::*;

/// A source two editions lower apart: Bedrock's theme variant leaves a slot unbound.
const EDITION_SPECIFIC: &str = "@cairn 2026.06\n\n\
     theme shop_java:\n\
     \x20\x20slot wall           -> @wall.stone.cobble\n\
     \x20\x20slot floating_text  -> @sign.oak\n\n\
     theme shop_bedrock:\n\
     \x20\x20slot wall           -> @wall.stone.cobble\n\n\
     struct shop size=5x5\n\
     \x20\x20walls  class=outer mat_slot=wall height=3\n\
     \x20\x20window class=display side=front offset=2 y=2 size=1x1 mat_slot=floating_text\n";

const HUT: &str = "theme t:\n  slot a -> @cobblestone\n\nstruct hut size=3x3\n  floor mat_slot=a\n";

const LOCK: &str = "no/such/dir/probe.crn.lock";

fn both_editions() -> RequestedEditions {
    requested_editions(&["java".to_owned(), "bedrock".to_owned()]).expect("both editions parse")
}

/// `info` on `source`, with what it sent its sink.
fn run(source: &str, errors_in_document: bool) -> (Result<InfoDocument, Vec<Found>>, Vec<String>) {
    let editions = both_editions();
    let request = InfoRequest {
        file: Path::new("probe.crn"),
        source,
        editions: &editions,
        lock: Path::new(LOCK),
        errors_in_document,
    };
    let mut sink = Recorded::default();
    let result = info(&request, &mut sink);
    (result, sink.0)
}

fn codes(found: &[Found]) -> Vec<&'static str> {
    found
        .iter()
        .map(|found| match found {
            Found::Core(d) => d.code.as_str(),
            Found::Circuit(d) => d.code.as_str(),
        })
        .collect()
}

#[test]
fn an_editions_list_keeps_each_edition_once_in_the_order_it_first_names_it() {
    let names = ["bedrock", "java", "bedrock"].map(str::to_owned);
    assert_eq!(
        requested_editions(&names).map(|asked| asked.as_slice().to_vec()),
        Ok(vec![Edition::Bedrock, Edition::Java]),
    );
    assert_eq!(
        requested_editions(&["java".to_owned(), " ".to_owned()]),
        Err("--editions value must not contain empty entries".to_owned()),
    );
}

#[test]
fn the_default_lock_keeps_the_source_extension() {
    assert_eq!(
        default_lock_path(Path::new("site/shop.crn")),
        Path::new("site/shop.crn.lock"),
    );
}

#[test]
fn a_source_that_does_not_parse_is_refused_with_the_parse_finding_alone() {
    let (result, sent) = run("struct\n", true);
    let refused = result.expect_err("an unparsable source has no report");
    assert_eq!(codes(&refused), ["E_PARSE"]);
    assert_eq!(sent, [] as [String; 0]);
}

#[test]
fn a_report_carries_the_lock_it_read_and_sends_the_note_explaining_it() {
    let (result, sent) = run(HUT, true);
    let document = result.expect("a clean source is reported on");
    let value = serde_json::to_value(&document).expect("the document serialises");
    assert_eq!(
        value["verified_target"],
        json!({ "path": LOCK, "status": "missing" }),
    );
    assert!(
        value.get("registry_compat").is_some(),
        "the axes sit beside the row, not under a key: {value}",
    );
    assert_eq!(
        sent,
        [format!(
            "note: no lockfile at {LOCK}, so no target is verified; `cairn compile` writes one"
        )],
    );
}

#[test]
fn an_edition_error_is_held_for_the_document_or_sent_under_its_edition() {
    let (held, sent) = run(EDITION_SPECIFIC, true);
    assert_eq!(
        codes(&held.expect_err("the bedrock lowering refuses the run")),
        ["E_UNRESOLVED_SLOT"],
    );
    assert!(
        !sent.iter().any(|line| line.contains("--editions bedrock")),
        "no header over an edition whose only finding went to the document: {sent:?}",
    );

    let (reported, sent) = run(EDITION_SPECIFIC, false);
    assert_eq!(
        codes(&reported.expect_err("the bedrock lowering refuses the run")),
        [] as [&str; 0],
    );
    let header = sent
        .iter()
        .position(|line| line == "note: reported for --editions bedrock")
        .unwrap_or_else(|| panic!("the header naming the edition: {sent:?}"));
    assert_eq!(sent[header + 1], "finding E_UNRESOLVED_SLOT");
}

#[test]
fn a_failure_document_renders_each_finding_it_refuses() {
    let (result, _) = run("struct\n", true);
    let refused = result.expect_err("an unparsable source has no report");
    let document = FailureDocument::new("struct\n", &refused);
    let value = serde_json::to_value(&document).expect("the document serialises");
    let rendered: Vec<&str> = value["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert_eq!(rendered, ["E_PARSE"]);
}
