use std::path::Path;

use cairn_lang_core::{Edition, UnknownEdition};
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

/// [`HUT`] with walls and a window whose `shape=` no pass reads, so its one finding is a warning.
const WARNED: &str = "theme t:\n  slot a -> @cobblestone\n\nstruct hut size=3x3\n  floor \
                      mat_slot=a\n  walls mat_slot=a height=3\n  window side=front offset=1 y=1 \
                      size=1x1 mat_slot=a shape=round\n";

const LOCK: &str = "no/such/dir/probe.crn.lock";

/// `info` on `source` at `probe.crn` with the lock `--lock` names, with what it sent its sink.
fn run_with(
    source: &str,
    lock: Option<&Path>,
    format: ReportFormat,
) -> (Result<InfoDocument, Refusal>, Vec<String>) {
    let editions = RequestedEditions::default();
    let request = InfoRequest {
        file: Path::new("probe.crn"),
        source,
        editions: &editions,
        lock,
        format,
    };
    let mut sink = Recorded::default();
    let result = info(&request, &mut sink);
    (result, sink.0)
}

/// `info` on `source` at `probe.crn` with `--lock` [`LOCK`], with what it sent its sink.
fn run(source: &str, format: ReportFormat) -> (Result<InfoDocument, Refusal>, Vec<String>) {
    run_with(source, Some(Path::new(LOCK)), format)
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
        RequestedEditions::default().as_slice(),
        [Edition::Java, Edition::Bedrock],
    );
}

#[test]
fn an_editions_list_naming_nothing_to_report_on_is_refused() {
    assert_eq!(requested_editions(&[]), Err(EditionsRefused::Empty));
    assert_eq!(
        requested_editions(&["java".to_owned(), " ".to_owned()]),
        Err(EditionsRefused::EmptyEntry),
    );
    assert_eq!(
        requested_editions(&["java".to_owned(), "pocket".to_owned()]),
        Err(EditionsRefused::Unknown(UnknownEdition {
            input: "pocket".to_owned()
        })),
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
    let (result, sent) = run("struct\n", ReportFormat::Json);
    let refused = result.expect_err("an unparsable source has no report");
    assert_eq!(codes(&refused.findings), ["E_PARSE"]);
    assert_eq!(refused.palettes, [] as [String; 0]);
    assert_eq!(sent, [] as [String; 0]);
}

#[test]
fn a_report_carries_the_lock_it_read_and_sends_the_note_explaining_it() {
    let (result, sent) = run(HUT, ReportFormat::Json);
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
            "note: no lockfile at {LOCK}, so no target is verified; no/such/dir is not a \
             directory, so `cairn compile --lock {LOCK}` cannot write one there"
        )],
    );
}

#[test]
fn a_lock_named_at_the_default_path_is_still_the_one_compile_is_told_to_write() {
    let lock = default_lock_path(Path::new("probe.crn"));
    let note = |compile: &str| {
        format!(
            "note: no lockfile at {}, so no target is verified; {compile} writes one",
            lock.display()
        )
    };
    let (beside, sent) = run_with(HUT, None, ReportFormat::Json);
    assert_eq!(
        beside
            .expect("a clean source is reported on")
            .verified_target
            .path,
        lock.display().to_string(),
    );
    assert_eq!(sent, [note("`cairn compile`")]);
    let (_, sent) = run_with(HUT, Some(&lock), ReportFormat::Json);
    assert_eq!(
        sent,
        [note(&format!("`cairn compile --lock {}`", lock.display()))]
    );
}

#[test]
fn a_warning_is_sent_to_the_sink_and_the_source_is_still_reported_on() {
    for format in [ReportFormat::Text, ReportFormat::Json] {
        let (result, sent) = run(WARNED, format);
        result.expect("a warning does not refuse the run");
        assert_eq!(
            sent.first().map(String::as_str),
            Some("finding W_IGNORED_ARGUMENT"),
            "{format:?}: {sent:?}",
        );
    }
}

#[test]
fn an_edition_error_is_held_for_the_document_or_sent_under_its_edition() {
    let (held, sent) = run(EDITION_SPECIFIC, ReportFormat::Json);
    assert_eq!(
        codes(
            &held
                .expect_err("the bedrock lowering refuses the run")
                .findings
        ),
        ["E_UNRESOLVED_SLOT"],
    );
    assert!(
        !sent.iter().any(|line| line.contains("--editions bedrock")),
        "no header over an edition whose only finding went to the document: {sent:?}",
    );

    let (reported, sent) = run(EDITION_SPECIFIC, ReportFormat::Text);
    assert_eq!(
        codes(
            &reported
                .expect_err("the bedrock lowering refuses the run")
                .findings
        ),
        [] as [&str; 0],
    );
    let header = sent
        .iter()
        .position(|line| line == "note: reported for --editions bedrock")
        .unwrap_or_else(|| panic!("the header naming the edition: {sent:?}"));
    assert_eq!(sent[header + 1], "finding E_UNRESOLVED_SLOT");
}

#[test]
fn a_refused_palette_refuses_the_run_with_the_lines_it_sent_and_no_finding() {
    let source = "theme t:\n  slot step -> @oak_stairs[facing=up]\n\nstruct s size=3x3\n  floor \
                  mat_slot=step\n";
    let (result, sent) = run(source, ReportFormat::Json);
    let refused = result.expect_err("a palette the pack refuses has no report");
    assert_eq!(codes(&refused.findings), [] as [&str; 0]);
    assert_eq!(
        refused.palettes.first().map(String::as_str),
        Some(
            "error: the bedrock palette carries blockstates a registry pack is expected to \
             refuse, so this edition gets no portability figure:"
        ),
    );
    let at = sent
        .iter()
        .position(|line| *line == refused.palettes[0])
        .unwrap_or_else(|| panic!("the sink got the palette lines: {sent:?}"));
    assert_eq!(sent[at..at + refused.palettes.len()], refused.palettes[..]);
}

#[test]
fn a_failure_document_renders_each_finding_it_refuses() {
    let (result, _) = run("struct\n", ReportFormat::Json);
    let refused = result.expect_err("an unparsable source has no report");
    let document = refused.document("struct\n");
    let value = serde_json::to_value(&document).expect("the document serialises");
    let rendered: Vec<&str> = value["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert_eq!(rendered, ["E_PARSE"]);
}
