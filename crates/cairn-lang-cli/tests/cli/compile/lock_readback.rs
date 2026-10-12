//! `cairn compile` reads back the lockfile it is about to replace, per `spec/versioning-editions`
//! "Provenance and lock".

use std::fs;

use tempfile::TempDir;

use crate::common::{Run, compile, example_in_tempdir};

fn warning_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("W_") || line.starts_with("warning:"))
        .collect()
}

#[test]
fn a_recompile_for_another_version_prints_the_shape_the_spec_prints() {
    // The exact line `spec/versioning-editions` "Provenance and lock" shows.
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    let first = Run::from(compile(&src, "java", "1.20.4", out_dir.path(), Some(&lock)));
    assert_eq!(first.code, Some(0), "stderr={}", first.stderr);
    assert!(
        warning_lines(&first.stderr).is_empty(),
        "the first compile has nothing to compare against: {:?}",
        warning_lines(&first.stderr),
    );

    let second = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    assert_eq!(second.code, Some(0), "stderr={}", second.stderr);
    assert_eq!(
        warning_lines(&second.stderr),
        ["W_PREVIOUSLY_VERIFIED_TARGET: verified for 1.20.4/DataVersion 3700, now 1.21.4/4189."],
        "stderr does not carry the spec's line: {}",
        second.stderr,
    );
    assert!(
        !second.stdout.contains("W_PREVIOUSLY_VERIFIED_TARGET"),
        "the warning must not reach stdout: {}",
        second.stdout,
    );
}

#[test]
fn a_recompile_for_the_same_target_says_nothing() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock));
    let again = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    assert_eq!(again.code, Some(0), "stderr={}", again.stderr);
    assert!(
        warning_lines(&again.stderr).is_empty(),
        "an unchanged target should warn about nothing: {:?}",
        warning_lines(&again.stderr),
    );
}

#[test]
fn a_change_of_edition_names_both_editions() {
    // `1.21.4` and `1.21.60` are not comparable without their editions.
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock));
    let switched = Run::from(compile(
        &src,
        "bedrock",
        "1.21.60",
        out_dir.path(),
        Some(&lock),
    ));
    assert_eq!(switched.code, Some(0), "stderr={}", switched.stderr);
    let line = switched
        .stderr
        .lines()
        .find(|l| l.starts_with("W_PREVIOUSLY_VERIFIED_TARGET:"))
        .unwrap_or_else(|| panic!("no warning in: {}", switched.stderr));
    assert!(
        line.contains("java 1.21.4") && line.contains("bedrock 1.21.60"),
        "the line should name both editions: {line}",
    );
}

#[test]
fn the_verified_side_names_the_field_the_recorded_edition_actually_has() {
    // Only the left half names the field, so the Bedrock lock has to be the recorded one.
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, "bedrock", "1.21.60", out_dir.path(), Some(&lock));
    let switched = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    let line = switched
        .stderr
        .lines()
        .find(|l| l.starts_with("W_PREVIOUSLY_VERIFIED_TARGET:"))
        .unwrap_or_else(|| panic!("no warning in: {}", switched.stderr));
    assert!(
        line.contains("verified for bedrock 1.21.60/block version 18168865,"),
        "the verified side should name Bedrock's own version field: {line}",
    );
    assert!(
        line.ends_with("now java 1.21.4/4189."),
        "the new side prints the integer bare: {line}",
    );
}

#[test]
fn the_sensitivity_list_reported_is_the_one_the_lockfile_recorded() {
    // The compiler records no sensitivity, so the entries come from a doctored lock.
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, "java", "1.20.4", out_dir.path(), Some(&lock));
    let recorded = fs::read_to_string(&lock).expect("read lock");
    let doctored = recorded.replace(
        "member_version_sensitivity: []\n",
        "member_version_sensitivity:\n\
         - id: yard_water\n  \
           reason: cauldron split at 1.17\n\
         - id: fence\n  \
           reason: connects differently since 1.16\n",
    );
    assert_ne!(doctored, recorded, "the fixture did not add any entries");
    fs::write(&lock, &doctored).expect("write lock");

    let second = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    assert_eq!(
        warning_lines(&second.stderr),
        [
            PREVIOUS_TARGET,
            "W_SEMANTIC_SENSITIVITY: 2 members may resolve differently: yard_water, fence",
        ],
        "stderr does not carry the recorded members: {}",
        second.stderr,
    );
    assert!(
        !second.stdout.contains("W_SEMANTIC_SENSITIVITY"),
        "the warning must not reach stdout: {}",
        second.stdout,
    );
}

#[test]
fn one_recorded_member_reads_as_one_member() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, "java", "1.20.4", out_dir.path(), Some(&lock));
    let recorded = fs::read_to_string(&lock).expect("read lock");
    let doctored = recorded.replace(
        "member_version_sensitivity: []\n",
        "member_version_sensitivity:\n\
         - id: yard_water\n  \
           reason: cauldron split at 1.17\n",
    );
    assert_ne!(doctored, recorded, "the fixture did not add any entries");
    fs::write(&lock, &doctored).expect("write lock");

    let second = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    assert_eq!(
        warning_lines(&second.stderr),
        [
            PREVIOUS_TARGET,
            "W_SEMANTIC_SENSITIVITY: 1 member may resolve differently: yard_water",
        ],
        "stderr: {}",
        second.stderr,
    );
}

#[test]
fn nothing_is_said_about_sensitivity_when_the_lockfile_recorded_none() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, "java", "1.20.4", out_dir.path(), Some(&lock));
    let second = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    assert_eq!(
        warning_lines(&second.stderr),
        [PREVIOUS_TARGET],
        "stderr: {}",
        second.stderr,
    );
}

#[test]
fn a_lockfile_that_does_not_parse_is_reported_rather_than_discarded() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");
    fs::write(&lock, "this: [is not, a lockfile\n").expect("write junk");

    let run = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert!(
        run.stderr.contains("existing lockfile"),
        "stderr should say the old lockfile could not be read: {}",
        run.stderr,
    );
    let replaced = fs::read_to_string(&lock).expect("read lock");
    assert!(
        replaced.starts_with("lock_schema_version:"),
        "the lockfile should still have been replaced: {replaced}",
    );
}

#[test]
fn a_lockfile_from_a_newer_schema_is_reported_rather_than_read() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, "java", "1.20.4", out_dir.path(), Some(&lock));
    let recorded = fs::read_to_string(&lock).expect("read lock");
    let doctored = recorded.replace("lock_schema_version: 1\n", "lock_schema_version: 99\n");
    assert_ne!(doctored, recorded, "the fixture did not change the version");
    fs::write(&lock, doctored).expect("write lock");

    let run = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert!(
        run.stderr
            .contains("the existing lockfile is schema version 99 and this build reads 1"),
        "stderr should name the schema it refused, in its own words: {}",
        run.stderr,
    );
    assert!(
        !run.stderr.contains("could not be read"),
        "a newer document is not a corrupt one: {}",
        run.stderr,
    );
    assert!(
        !run.stderr.contains("W_PREVIOUSLY_VERIFIED_TARGET"),
        "a document that was not read cannot be compared: {}",
        run.stderr,
    );
    let replaced = fs::read_to_string(&lock).expect("read lock");
    assert!(
        replaced.starts_with("lock_schema_version: 1\n"),
        "the lockfile should have been replaced with this build's schema: {replaced}",
    );
}

#[test]
fn the_default_lock_path_is_read_back_too() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out");

    let first = Run::from(compile(&src, "java", "1.20.4", out_dir.path(), None));
    assert_eq!(first.code, Some(0), "stderr={}", first.stderr);
    let default_lock = src.with_extension("crn.lock");
    assert!(
        default_lock.exists(),
        "expected the default lockfile at {}",
        default_lock.display(),
    );

    let second = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), None));
    assert_eq!(
        warning_lines(&second.stderr),
        [PREVIOUS_TARGET],
        "the default path was not read back: {}",
        second.stderr,
    );
}

/// `village.crn` compiled for 1.20.4, its lock rewritten by `doctor`, then recompiled for 1.21.4.
fn recompile_village_over(doctor: impl FnOnce(&str) -> String) -> (Run, String) {
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("village.lock");
    let first = Run::from(compile(&src, "java", "1.20.4", out_dir.path(), Some(&lock)));
    assert_eq!(first.code, Some(0), "stderr={}", first.stderr);
    let written = fs::read_to_string(&lock).expect("read lock");
    let doctored = doctor(&written);
    assert_ne!(doctored, written, "the fixture changed nothing");
    fs::write(&lock, doctored).expect("write lock");
    let run = Run::from(compile(&src, "java", "1.21.4", out_dir.path(), Some(&lock)));
    let replaced = fs::read_to_string(&lock).expect("read lock");
    (run, replaced)
}

/// `written` with the first `recorded` line replaced by `refused`.
fn with_line(written: &str, recorded: &str, refused: &str) -> String {
    let doctored = written.replacen(recorded, refused, 1);
    assert_ne!(doctored, written, "`{recorded}` is not in the lock");
    doctored
}

const PREVIOUS_TARGET: &str =
    "W_PREVIOUSLY_VERIFIED_TARGET: verified for 1.20.4/DataVersion 3700, now 1.21.4/4189.";

/// Assert a lock refused only for `id`'s `ch` was reported, still compared and replaced.
fn assert_refused_but_read(run: &Run, replaced: &str, id: &str, ch: &str) {
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert!(
        run.stderr.contains(&format!(
            "the existing lockfile records an identifier this build refuses \
             (identifier `{id}` contains forbidden character `{ch}`) but is otherwise valid"
        )),
        "stderr should name the refused id in its own words: {}",
        run.stderr,
    );
    assert!(
        !run.stderr.contains("could not be read"),
        "a lock with a refused id is not a corrupt one: {}",
        run.stderr,
    );
    assert!(
        run.stderr.contains(PREVIOUS_TARGET),
        "the recorded target should still be compared: {}",
        run.stderr,
    );
    assert!(
        !replaced.contains(id),
        "the lockfile should have been replaced: {replaced}",
    );
}

// An earlier Cairn's lock can record an id this build refuses; its target is still compared.

#[test]
fn a_lock_recording_a_placement_id_the_rule_refuses_is_still_read() {
    let id = "sub/home1";
    let (run, replaced) = recompile_village_over(|written| {
        with_line(written, "  id: home1\n", &format!("  id: {id}\n"))
    });
    assert_refused_but_read(&run, &replaced, id, "/");
}

#[test]
fn a_lock_recording_a_walkway_port_the_rule_refuses_is_still_read() {
    let id = "a*b";
    let (run, replaced) = recompile_village_over(|written| {
        with_line(written, "    port: entry\n", &format!("    port: {id}\n"))
    });
    assert_refused_but_read(&run, &replaced, id, "*");
}

#[test]
fn a_lock_recording_a_no_break_space_quotes_it_as_its_escape() {
    // It would print as a plain space, on an id that reads `a b`.
    let id = "a\u{a0}b";
    let (run, replaced) = recompile_village_over(|written| {
        with_line(written, "    port: entry\n", &format!("    port: {id}\n"))
    });
    assert_refused_but_read(&run, &replaced, id, "\\u{a0}");
}

#[test]
fn a_lock_with_a_refused_id_still_reports_the_sensitive_members_it_recorded() {
    let id = "sub/home1";
    let (run, replaced) = recompile_village_over(|written| {
        let sensitive = with_line(
            written,
            "member_version_sensitivity: []\n",
            "member_version_sensitivity:\n- id: yard_water\n  reason: cauldron split at 1.17\n",
        );
        with_line(&sensitive, "  id: home1\n", &format!("  id: {id}\n"))
    });
    assert_refused_but_read(&run, &replaced, id, "/");
    assert!(
        run.stderr
            .contains("W_SEMANTIC_SENSITIVITY: 1 member may resolve differently: yard_water"),
        "the recorded member should be reported: {}",
        run.stderr,
    );
}

/// Assert a lock broken besides its refused id is reported unreadable, with `cause`, uncompared.
fn assert_unreadable_despite_refused_id(run: &Run, cause: &str) {
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert!(
        run.stderr
            .contains("the existing lockfile could not be read (lockfile YAML: ")
            && run.stderr.contains(cause),
        "stderr should report the lock unreadable because of {cause:?}: {}",
        run.stderr,
    );
    assert!(
        !run.stderr.contains("records an identifier"),
        "a broken lock is not one an earlier Cairn wrote: {}",
        run.stderr,
    );
    assert!(
        !run.stderr.contains("W_PREVIOUSLY_VERIFIED_TARGET"),
        "the target of an unreadable lock is not compared: {}",
        run.stderr,
    );
}

fn with_refused_id(written: &str) -> String {
    with_line(written, "  id: home1\n", "  id: sub/home1\n")
}

#[test]
fn a_lock_missing_a_required_field_is_unreadable_despite_a_refused_id() {
    let (run, _) = recompile_village_over(|written| {
        with_refused_id(&with_line(written, "member_version_sensitivity: []\n", ""))
    });
    assert_unreadable_despite_refused_id(&run, "missing field `member_version_sensitivity`");
}

#[test]
fn a_lock_with_a_malformed_hash_is_unreadable_despite_a_refused_id() {
    let (run, _) = recompile_village_over(|written| {
        let source_hash = written
            .lines()
            .find(|line| line.starts_with("source_hash: "))
            .expect("source_hash");
        with_refused_id(&with_line(
            written,
            source_hash,
            "source_hash: \"NOTAHASH:\"",
        ))
    });
    assert_unreadable_despite_refused_id(&run, "hash is missing the `sha256:` prefix");
}

#[test]
fn a_lock_whose_schema_version_does_not_parse_is_unreadable_despite_a_refused_id() {
    // An unparsed schema version vouches for nothing, so the target beside it is not printed.
    let (run, _) = recompile_village_over(|written| {
        let unversioned = with_line(
            written,
            "lock_schema_version: 1\n",
            "lock_schema_version: \"1\"\n",
        );
        let unknown_target = with_line(
            &unversioned,
            "data_version: 3700\n",
            "data_version: 31337\n",
        );
        with_refused_id(&unknown_target)
    });
    assert_unreadable_despite_refused_id(&run, "lock_schema_version: invalid type: string");
}

#[test]
fn a_lock_carrying_an_unknown_key_is_unreadable_despite_a_refused_id() {
    let (run, _) = recompile_village_over(|written| {
        with_refused_id(&with_line(
            written,
            "  def: cottage\n",
            "  def: cottage\n  tampered: true\n",
        ))
    });
    assert_unreadable_despite_refused_id(&run, "unknown field `tampered`");
}
