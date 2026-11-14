//! The `verified target` row: what the source's lockfile says it verified, per
//! `spec/versioning-editions` "The `verified target` row".

use std::fs;
use std::path::Path;

use cairn_lang_core::CAIRN_VERSION;

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
    assert_eq!(
        verified_row(&run),
        format!(
            "java 1.21.4/DataVersion 4189 (not verified), source unchanged, written by Cairn \
             {CAIRN_VERSION}"
        ),
    );
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
