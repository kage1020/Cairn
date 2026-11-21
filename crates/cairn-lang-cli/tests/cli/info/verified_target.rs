//! The `verified target` row: what the source's lockfile records, per
//! `spec/versioning-editions` "The `verified target` row".

use std::fs;
use std::path::{Path, PathBuf};

use cairn_lang_core::{CAIRN_VERSION, Edition};
use cairn_lang_formats::registry::builtin_pack;

use crate::common::{Fixture, Run, cairn_argv, compile, compile_as};

const HUT: &str = "theme t:\n  slot a -> @cobblestone\n\nstruct hut size=3x3\n  floor mat_slot=a\n";

/// `cairn info` of the fixture's source for Java alone, with `extra` arguments.
fn info(fixture: &Fixture, extra: &[&str]) -> Run {
    let source = fixture.source();
    let mut argv = vec![
        "info",
        source.to_str().expect("utf-8 path"),
        "--editions",
        "java",
    ];
    argv.extend_from_slice(extra);
    Run::from(cairn_argv(&argv))
}

/// The text after the `verified target:` label.
fn verified_row(run: &Run) -> &str {
    let rows: Vec<&str> = run
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("verified target:         "))
        .collect();
    let [row] = rows.as_slice() else {
        panic!("one `verified target` row expected, got: {}", run.stdout);
    };
    row
}

fn compiled(edition: &str, target: &str) -> Fixture {
    let fixture = Fixture::new(HUT);
    let run = Run::from(compile_as(&fixture, edition, target));
    assert_eq!(
        run.code,
        Some(0),
        "premise: the compile writes a lock: {}",
        run.stderr
    );
    fixture
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn a_compiled_source_reports_the_java_target_its_lock_verified() {
    let fixture = compiled("java", "1.21.4");
    let run = info(&fixture, &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        verified_row(&run),
        format!("java 1.21.4/DataVersion 4189, source unchanged, written by Cairn {CAIRN_VERSION}"),
    );
    assert_eq!(run.stderr, "", "an unchanged source needs no note");
}

#[test]
fn a_bedrock_target_names_its_block_version() {
    let fixture = compiled("bedrock", "1.21.60");
    let run = info(&fixture, &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        verified_row(&run),
        format!(
            "bedrock 1.21.60/block version 18168865, source unchanged, written by Cairn \
             {CAIRN_VERSION}"
        ),
    );
    assert_eq!(run.stderr, "", "an unchanged source needs no note");
}

#[test]
fn a_source_edited_since_its_compile_reports_the_change() {
    let fixture = compiled("java", "1.21.4");
    fs::write(
        fixture.source(),
        format!("{HUT}  walls mat_slot=a height=2\n"),
    )
    .expect("edit");
    let run = info(&fixture, &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        verified_row(&run),
        format!("java 1.21.4/DataVersion 4189, source changed, written by Cairn {CAIRN_VERSION}"),
    );
    assert_eq!(
        run.stderr,
        format!(
            "note: {} has changed since {} was written, so its target was verified for another \
             revision of the file; the next `cairn compile` rewrites the lock\n",
            display(&fixture.source()),
            display(&fixture.lock()),
        ),
    );
}

#[test]
fn a_source_never_compiled_has_no_lock() {
    let fixture = Fixture::new(HUT);
    let run = info(&fixture, &[]);
    assert_eq!(
        run.code,
        Some(0),
        "a missing lock does not refuse: {}",
        run.stderr
    );
    assert_eq!(verified_row(&run), "(no lock)");
    assert_eq!(
        run.stderr,
        format!(
            "note: no lockfile at {}, so no target is verified; `cairn compile` writes one\n",
            display(&fixture.lock()),
        ),
    );
}

#[test]
fn the_lock_flag_names_the_lock_read() {
    let fixture = Fixture::new(HUT);
    let elsewhere = fixture.out().with_extension("lock");
    let run = Run::from(compile(
        &fixture.source(),
        "java",
        "1.20.4",
        &fixture.out(),
        Some(&elsewhere),
    ));
    assert_eq!(
        run.code,
        Some(0),
        "premise: the compile writes a lock: {}",
        run.stderr
    );

    let named = info(
        &fixture,
        &["--lock", elsewhere.to_str().expect("utf-8 path")],
    );
    assert_eq!(
        verified_row(&named),
        format!("java 1.20.4/DataVersion 3700, source unchanged, written by Cairn {CAIRN_VERSION}"),
    );
    assert_eq!(named.stderr, "", "an unchanged source needs no note");
    let default = info(&fixture, &[]);
    assert_eq!(
        verified_row(&default),
        "(no lock)",
        "the default is `<source>.lock`"
    );
}

#[test]
fn a_lock_of_a_newer_schema_is_named_and_not_read() {
    let fixture = compiled("java", "1.21.4");
    let body = fs::read_to_string(fixture.lock()).expect("lock");
    let newer = body.replace("lock_schema_version: 1\n", "lock_schema_version: 2\n");
    assert_ne!(newer, body, "premise: the lock declares its schema");
    fs::write(fixture.lock(), newer).expect("write lock");
    let run = info(&fixture, &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        verified_row(&run),
        "(lock schema 2, newer than this Cairn reads)"
    );
    assert_eq!(
        run.stderr,
        format!(
            "note: {} declares lock schema version 2, and this Cairn reads 1; the newer Cairn \
             that wrote it can report it\n",
            display(&fixture.lock()),
        ),
    );
}

#[test]
fn a_lock_the_schema_refuses_is_unreadable() {
    let fixture = Fixture::new(HUT);
    fs::write(fixture.lock(), "verified: maybe\n").expect("write lock");
    let run = info(&fixture, &[]);
    assert_eq!(
        run.code,
        Some(0),
        "an unreadable lock does not refuse: {}",
        run.stderr
    );
    assert_eq!(verified_row(&run), "(unreadable lock)");
    assert_eq!(
        run.stderr,
        format!(
            "note: {}: the lockfile could not be read (lockfile YAML: verified: invalid type: \
             string \"maybe\", expected a boolean at line 1 column 11); the next `cairn \
             compile` replaces it\n",
            display(&fixture.lock()),
        ),
    );
}

#[test]
fn a_lock_refused_only_for_an_identifier_still_reports_its_target() {
    let fixture = compiled("java", "1.21.4");
    let mut body = fs::read_to_string(fixture.lock()).expect("lock");
    body.push_str(
        "placements:\n- site: a/b\n  id: home\n  def: hut\n  theme: t\n  origin: [0, 0, 0]\n  \
         dims: [3, 1, 3]\n",
    );
    fs::write(fixture.lock(), body).expect("write lock");
    let run = info(&fixture, &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        verified_row(&run),
        format!("java 1.21.4/DataVersion 4189, source unchanged, written by Cairn {CAIRN_VERSION}"),
    );
    assert_eq!(
        run.stderr,
        format!(
            "note: {} records an identifier this Cairn refuses (identifier `a/b` contains \
             forbidden character `/`); the next `cairn compile` replaces it\n",
            display(&fixture.lock()),
        ),
    );
}

#[test]
fn a_lock_not_verified_says_so() {
    let fixture = compiled("java", "1.21.4");
    let body = fs::read_to_string(fixture.lock()).expect("lock");
    let unverified = body.replace("verified: true\n", "verified: false\n");
    assert_ne!(unverified, body, "premise: the lock records `verified`");
    fs::write(fixture.lock(), unverified).expect("write lock");
    let run = info(&fixture, &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        verified_row(&run),
        format!(
            "java 1.21.4/DataVersion 4189 (not verified), source unchanged, written by Cairn \
             {CAIRN_VERSION}"
        ),
    );
    assert_eq!(run.stderr, "", "the row says it is not verified");
}

#[test]
fn json_carries_the_row_as_verified_target_with_its_path() {
    let fixture = compiled("java", "1.21.4");
    let run = info(&fixture, &["--format", "json"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    assert_eq!(
        report["verified_target"],
        serde_json::json!({
            "status": "read",
            "path": display(&fixture.lock()),
            "target": { "edition": "java", "mc_version": "1.21.4", "data_version": 4189 },
            "verified": true,
            "source_unchanged": true,
            "cairn_version": CAIRN_VERSION,
        }),
    );

    let never = Fixture::new(HUT);
    let run = info(&never, &["--format", "json"]);
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    assert_eq!(
        report["verified_target"],
        serde_json::json!({ "status": "missing", "path": display(&never.lock()) }),
    );
}

/// `body` with a placement whose `site` the identifier rule refuses.
fn with_refused_placement(body: &str) -> String {
    format!(
        "{body}placements:\n- site: a/b\n  id: home\n  def: hut\n  theme: t\n  origin: [0, 0, 0]\n  \
         dims: [3, 1, 3]\n"
    )
}

#[test]
fn a_refused_identifier_is_noted_before_an_edited_source() {
    let fixture = compiled("java", "1.21.4");
    let body = fs::read_to_string(fixture.lock()).expect("lock");
    fs::write(fixture.lock(), with_refused_placement(&body)).expect("write lock");
    fs::write(
        fixture.source(),
        format!("{HUT}  walls mat_slot=a height=2\n"),
    )
    .expect("edit");
    let run = info(&fixture, &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        verified_row(&run),
        format!("java 1.21.4/DataVersion 4189, source changed, written by Cairn {CAIRN_VERSION}"),
    );
    assert_eq!(
        run.stderr,
        format!(
            "note: {lock} records an identifier this Cairn refuses (identifier `a/b` contains \
             forbidden character `/`); the next `cairn compile` replaces it\n\
             note: {source} has changed since {lock} was written, so its target was verified for \
             another revision of the file; the next `cairn compile` rewrites the lock\n",
            lock = display(&fixture.lock()),
            source = display(&fixture.source()),
        ),
    );
}

/// The fixture compiled for java 1.21.4 with its lock at `out.lock`, not `<source>.lock`.
fn compiled_elsewhere() -> (Fixture, PathBuf) {
    let fixture = Fixture::new(HUT);
    let elsewhere = fixture.out().with_extension("lock");
    let run = Run::from(compile(
        &fixture.source(),
        "java",
        "1.21.4",
        &fixture.out(),
        Some(&elsewhere),
    ));
    assert_eq!(
        run.code,
        Some(0),
        "premise: the compile writes a lock: {}",
        run.stderr
    );
    (fixture, elsewhere)
}

#[test]
fn notes_on_a_lock_the_flag_names_say_to_compile_with_the_flag() {
    let (fixture, elsewhere) = compiled_elsewhere();
    let lock = display(&elsewhere);
    let body = fs::read_to_string(&elsewhere).expect("lock");
    fs::write(&elsewhere, with_refused_placement(&body)).expect("write lock");
    fs::write(
        fixture.source(),
        format!("{HUT}  walls mat_slot=a height=2\n"),
    )
    .expect("edit");
    let run = info(&fixture, &["--lock", &lock]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        run.stderr,
        format!(
            "note: {lock} records an identifier this Cairn refuses (identifier `a/b` contains \
             forbidden character `/`); the next `cairn compile --lock {lock}` replaces it\n\
             note: {source} has changed since {lock} was written, so its target was verified for \
             another revision of the file; the next `cairn compile --lock {lock}` rewrites the \
             lock\n",
            source = display(&fixture.source()),
        ),
    );

    fs::write(&elsewhere, "verified: maybe\n").expect("write lock");
    let run = info(&fixture, &["--lock", &lock]);
    assert_eq!(verified_row(&run), "(unreadable lock)");
    assert_eq!(
        run.stderr,
        format!(
            "note: {lock}: the lockfile could not be read (lockfile YAML: verified: invalid type: \
             string \"maybe\", expected a boolean at line 1 column 11); the next `cairn compile \
             --lock {lock}` replaces it\n"
        ),
    );
}

#[test]
fn a_lock_flag_naming_no_file_reports_no_lock() {
    let fixture = Fixture::new(HUT);
    let lock = display(&fixture.out().with_extension("lock"));
    let run = info(&fixture, &["--lock", &lock]);
    assert_eq!(
        run.code,
        Some(0),
        "a missing lock does not refuse: {}",
        run.stderr
    );
    assert_eq!(verified_row(&run), "(no lock)");
    assert_eq!(
        run.stderr,
        format!(
            "note: no lockfile at {lock}, so no target is verified; `cairn compile --lock {lock}` \
             writes one\n"
        ),
    );
}

#[test]
fn a_lock_flag_naming_the_default_path_still_says_to_compile_with_the_flag() {
    let fixture = Fixture::new(HUT);
    let lock = display(&fixture.lock());
    let run = info(&fixture, &["--lock", &lock]);
    assert_eq!(verified_row(&run), "(no lock)");
    assert_eq!(
        run.stderr,
        format!(
            "note: no lockfile at {lock}, so no target is verified; `cairn compile --lock {lock}` \
             writes one\n"
        ),
    );
}

#[test]
fn a_lock_flag_through_no_directory_says_no_compile_can_write_there() {
    let fixture = Fixture::new(HUT);
    for (dir, label) in [
        (fixture.out(), "a directory that does not exist"),
        (fixture.source(), "a file in place of a directory"),
    ] {
        let lock = display(&dir.join("s.crn.lock"));
        let run = info(&fixture, &["--lock", &lock]);
        assert_eq!(run.code, Some(0), "{label}: {}", run.stderr);
        assert_eq!(verified_row(&run), "(no lock)", "{label}");
        assert_eq!(
            run.stderr,
            format!(
                "note: no lockfile at {lock}, so no target is verified; {} is not a directory, so \
                 `cairn compile --lock {lock}` cannot write one there\n",
                display(&dir),
            ),
            "{label}",
        );
    }
}

#[test]
fn a_directory_at_the_lock_path_is_unreadable_with_no_replacement_promised() {
    let fixture = Fixture::new(HUT);
    fs::create_dir(fixture.out()).expect("create directory");
    let lock = display(&fixture.out());
    let run = info(&fixture, &["--lock", &lock]);
    assert_eq!(
        run.code,
        Some(0),
        "an unreadable lock does not refuse: {}",
        run.stderr
    );
    assert_eq!(verified_row(&run), "(unreadable lock)");
    assert_eq!(
        run.stderr,
        format!(
            "note: {lock}: the lockfile could not be read (lockfile I/O: a directory, not a file)\n"
        ),
    );
    let run = info(&fixture, &["--lock", &lock, "--format", "json"]);
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    assert_eq!(
        report["verified_target"],
        serde_json::json!({
            "status": "unreadable",
            "path": lock,
            "kind": "io",
            "error": "lockfile I/O: a directory, not a file",
        }),
    );
}

#[test]
fn a_lock_value_that_would_break_its_row_adds_no_row_to_the_report() {
    let fixture = compiled("java", "1.21.4");
    let body = fs::read_to_string(fixture.lock()).expect("lock");
    let recorded = format!("cairn_version: {CAIRN_VERSION}\n");
    let forged = body.replace(
        &recorded,
        "cairn_version: \"x\\nsemantic-sensitive:      fake\"\n",
    );
    assert_ne!(forged, body, "premise: the lock records `{recorded}`");
    fs::write(fixture.lock(), forged).expect("write lock");
    let run = info(&fixture, &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "registry compatibility:  0.0 .. latest\n\
         inferred range:          Java: 0.0 .. latest\n\
         edition portability:     Java: portable: 1  degraded: 0  unsupported: 0\n\
         respelled:               Java: none\n\
         buildable targets:       Java: 1.20.4, 1.21, 1.21.4, 26.3\n\
         intended targets:        (none declared)\n\
         semantic-sensitive:      Java: (none)\n\
         verified target:         (unreadable lock)\n",
    );
    assert_eq!(
        run.stderr,
        format!(
            "note: {}: the lockfile could not be read (lockfile YAML: invalid value: string \
             \"x\\nsemantic-sensitive:      fake\", expected a `cairn_version` naming a Cairn \
             release, `YYYY.M` or `YYYY.M.PATCH`); the next `cairn compile` replaces it\n",
            display(&fixture.lock()),
        ),
    );

    let forged = body.replace(
        "mc_version: 1.21.4\n",
        "mc_version: \"1.21.4\\nfake: row\"\n",
    );
    assert_ne!(
        forged, body,
        "premise: the lock records `mc_version: 1.21.4`"
    );
    fs::write(fixture.lock(), forged).expect("write lock");
    let run = info(&fixture, &["--format", "json"]);
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    assert_eq!(
        report["verified_target"],
        serde_json::json!({
            "status": "unreadable",
            "path": display(&fixture.lock()),
            "kind": "invalid",
            "error": "lockfile YAML: target: invalid value: string \"1.21.4\\nfake: row\", \
                      expected an `mc_version` without control characters or line breaks at line \
                      5 column 3",
        }),
    );
}

#[test]
fn every_target_a_compile_writes_reads_back_from_its_lock() {
    for edition in [Edition::Java, Edition::Bedrock] {
        let field = match edition {
            Edition::Java => "DataVersion",
            Edition::Bedrock => "block version",
        };
        for row in builtin_pack(edition).targetable() {
            let version = row.mc_version.as_str();
            let fixture = compiled(edition.as_str(), version);
            let source = fixture.source();
            let source = source.to_str().expect("utf-8 path");
            let run = Run::from(cairn_argv(&[
                "info",
                source,
                "--editions",
                edition.as_str(),
            ]));
            assert_eq!(run.code, Some(0), "{edition} {version}: {}", run.stderr);
            assert_eq!(
                verified_row(&run),
                format!(
                    "{edition} {version}/{field} {}, source unchanged, written by Cairn \
                     {CAIRN_VERSION}",
                    row.block_version.unwrap_or(row.data_version),
                ),
            );
            assert_eq!(run.stderr, "", "{edition} {version}");
        }
    }
}
