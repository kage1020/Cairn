use std::path::{Path, PathBuf};

use cairn_lang_core::lock::{
    HashHex, LOCK_SCHEMA_VERSION, LockInputs, LockTarget, Lockfile, hash_source,
};
use cairn_lang_core::{CAIRN_VERSION, Edition, UnknownEdition};
use serde_json::json;

use super::*;
use crate::write_path::WritePath;

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

/// [`HUT`] with walls and a window whose `shape=` is no identifier, so its one finding is a warning.
const WARNED: &str = "theme t:\n  slot a -> @cobblestone\n\nstruct hut size=3x3\n  floor \
                      mat_slot=a\n  walls mat_slot=a height=3\n  window side=front offset=1 y=1 \
                      size=1x1 mat_slot=a shape=1\n";

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

/// A directory holding [`HUT`] as `hut.crn`, removed when the test ends.
fn hut_on_disk() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("hut.crn");
    std::fs::write(&file, HUT).expect("write source");
    (dir, file)
}

/// `info` on [`HUT`] at `file` with `--lock lock`, with what it sent its sink.
fn run_at(file: &Path, lock: &Path) -> (InfoDocument, Vec<String>) {
    run_on_disk(file, Some(lock))
}

/// `info` on [`HUT`] at `file`, with `--lock` when `lock` is given, and what it sent its sink.
fn run_on_disk(file: &Path, lock: Option<&Path>) -> (InfoDocument, Vec<String>) {
    let editions = RequestedEditions::default();
    let request = InfoRequest {
        file,
        source: HUT,
        editions: &editions,
        lock,
        format: ReportFormat::Json,
    };
    let mut sink = Recorded::default();
    let document = info(&request, &mut sink).expect("a clean source is reported on");
    (document, sink.0)
}

#[test]
fn a_lock_path_reaching_the_source_is_refused_however_it_is_spelled() {
    let (dir, file) = hut_on_disk();
    std::fs::create_dir(dir.path().join("sub")).expect("make sub");
    let refused = |lock: &Path| lock_path_refusal(&file, lock);

    for lock in [
        file.clone(),
        dir.path().join(".").join("hut.crn"),
        dir.path().join("sub").join("..").join("hut.crn"),
    ] {
        assert_eq!(
            refused(&lock),
            Some(LockPathRefusal::Source(WritePath::Final)),
            "{lock:?}",
        );
    }
    assert_eq!(refused(&dir.path().join("hut.crn.lock")), None);
    std::fs::write(dir.path().join("hut.crn.lock"), "").expect("write lock");
    assert_eq!(refused(&dir.path().join("hut.crn.lock")), None);
}

#[test]
fn a_lock_path_hard_linked_to_the_source_is_refused() {
    let (dir, file) = hut_on_disk();
    std::fs::hard_link(&file, dir.path().join("hard.lock")).expect("hard link");

    assert_eq!(
        lock_path_refusal(&file, &dir.path().join("hard.lock")),
        Some(LockPathRefusal::Source(WritePath::Final)),
    );
}

#[cfg(unix)]
#[test]
fn a_lock_path_symlinked_to_the_source_is_refused() {
    let (dir, file) = hut_on_disk();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("link")).expect("symlink dir");
    std::os::unix::fs::symlink(&file, dir.path().join("soft.lock")).expect("symlink");

    for lock in ["link/hut.crn", "soft.lock"] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)),
            Some(LockPathRefusal::Source(WritePath::Final)),
            "{lock}",
        );
    }
}

#[test]
fn a_lock_path_whose_scratch_file_reaches_the_source_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let lock = dir.path().join("notes");
    let refused = |source: &str| {
        let file = dir.path().join(source);
        std::fs::write(&file, HUT).expect("write source");
        lock_path_refusal(&file, &lock)
    };

    assert_eq!(
        refused("notes.tmp"),
        Some(LockPathRefusal::Source(WritePath::Staged)),
    );
    assert_eq!(
        refused("notes.bak"),
        Some(LockPathRefusal::Source(WritePath::Backup)),
    );
    assert_eq!(refused("notes.crn"), None);
}

#[test]
fn a_lock_flag_whose_scratch_file_is_the_source_gives_the_fix_rather_than_a_compile() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("notes.tmp");
    std::fs::write(&file, HUT).expect("write source");
    let lock = dir.path().join("notes");

    let (document, sent) = run_at(&file, &lock);

    assert_eq!(document.verified_target.reading, LockReading::Missing);
    let (file, lock) = (file.display(), lock.display());
    assert_eq!(
        sent,
        [
            format!("note: no lockfile at {lock}, so no target is verified"),
            format!(
                "note: `cairn compile` refuses `--lock {lock}`, whose scratch file {file} names \
                 {file}, the source; give `--lock` a `.lock` path, or leave it out to read \
                 {file}.lock"
            ),
        ],
    );
}

#[test]
fn a_default_lock_linked_to_the_source_gives_the_fix_rather_than_a_compile() {
    let (dir, file) = hut_on_disk();
    let scratch = dir.path().join("hut.crn.lock.tmp");
    std::fs::hard_link(&file, &scratch).expect("hard link");

    let (document, sent) = run_on_disk(&file, None);

    assert_eq!(document.verified_target.reading, LockReading::Missing);
    let (file, scratch) = (file.display(), scratch.display());
    assert_eq!(
        sent,
        [
            format!("note: no lockfile at {file}.lock, so no target is verified"),
            format!(
                "note: `cairn compile` refuses to write {file}.lock, whose scratch file {scratch} \
                 names {file}, the source; a link makes them one file, so give `--lock` a `.lock` \
                 path elsewhere"
            ),
        ],
    );
}

#[test]
fn a_lock_path_ending_in_crn_is_refused_whatever_it_names() {
    let (dir, file) = hut_on_disk();
    std::fs::write(dir.path().join("other.crn"), HUT).expect("write other source");

    for lock in [
        "other.crn",
        "missing.crn",
        "OTHER.CRN",
        ".crn",
        "no/such/dir/x.Crn",
    ] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)),
            Some(LockPathRefusal::SourceName),
            "{lock}",
        );
    }
    for lock in [
        "other.crn.lock",
        "crn",
        "other.crnx",
        "other.crn.d/hut.lock",
    ] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)),
            None,
            "{lock}"
        );
    }
}

#[test]
fn a_lock_flag_naming_the_source_gives_the_fix_rather_than_a_compile() {
    let (_dir, file) = hut_on_disk();

    let (document, sent) = run_at(&file, &file);

    let LockReading::Unreadable {
        kind: UnreadableKind::Invalid,
        error,
    } = &document.verified_target.reading
    else {
        panic!("the source is no lock: {:?}", document.verified_target);
    };
    let file = file.display();
    assert_eq!(
        sent,
        [
            format!("note: {file}: the lockfile could not be read ({error})"),
            format!(
                "note: `cairn compile` refuses `--lock {file}`, which names {file}, the source; \
                 give `--lock` a `.lock` path, or leave it out to read {file}.lock"
            ),
        ],
    );
}

#[test]
fn a_lock_flag_ending_in_crn_gives_the_fix_rather_than_a_compile() {
    let (dir, file) = hut_on_disk();
    let lock = dir.path().join("other.crn");
    let fix = format!(
        "note: `cairn compile` refuses `--lock {lock}`, which ends in `.crn` as a source does; \
         give `--lock` a `.lock` path, such as {lock}.lock, or leave it out to read {file}.lock",
        lock = lock.display(),
        file = file.display(),
    );

    let (_, sent) = run_at(&file, &lock);
    assert_eq!(
        sent,
        [
            format!(
                "note: no lockfile at {}, so no target is verified",
                lock.display()
            ),
            fix.clone(),
        ],
    );

    Lockfile {
        lock_schema_version: LOCK_SCHEMA_VERSION,
        source_hash: hash_source("an earlier revision"),
        cairn_version: CAIRN_VERSION.to_owned(),
        target: LockTarget {
            edition: Edition::Java,
            mc_version: "1.21.4".to_owned(),
            data_version: 4189,
        },
        inputs: LockInputs::zero(),
        resolved_ir_hash: HashHex::zero(),
        verified: true,
        member_version_sensitivity: Vec::new(),
        placements: Vec::new(),
        walkways: Vec::new(),
    }
    .write_to_path(&lock)
    .expect("the lock is written");
    let (_, sent) = run_at(&file, &lock);
    assert_eq!(
        sent,
        [
            format!(
                "note: {} has changed since {} was written, so its target was verified for \
                 another revision of the file",
                file.display(),
                lock.display(),
            ),
            fix,
        ],
    );
}
