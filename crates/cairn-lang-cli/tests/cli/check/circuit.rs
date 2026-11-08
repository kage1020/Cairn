//! `cairn check` builds each struct's circuit as `cairn compile` does and reports its findings,
//! unpinned only where every edition raises them at the same place.

use tempfile::TempDir;

use crate::common::{
    QUASI_CONNECTED_DISPENSER, Run, WALL_LEVER, assert_stream, cairn, json_stream, reported,
    write_source,
};

/// `cairn check` of `source` with `args` after the file.
fn check(source: &str, args: &[&str]) -> Run {
    let tmp = TempDir::new().expect("tempdir");
    let path = write_source(tmp.path(), "s.crn", source);
    let mut command_line = vec![path.to_str().unwrap()];
    command_line.extend_from_slice(args);
    Run::from(cairn("check", &command_line))
}

#[test]
fn a_circuit_finding_every_edition_raises_is_reported_unpinned_and_pinned() {
    let expected = [(8, 3, "error", "E_ROUTE_UNREACHABLE_COMPONENT")];
    for args in [&[][..], &["--edition", "java"], &["--edition", "bedrock"]] {
        let run = check(WALL_LEVER, args);
        assert_eq!(run.code, Some(1), "{args:?}: stderr={}", run.stderr);
        assert_stream(&run.stderr, &expected);
    }
    let run = check(WALL_LEVER, &["--format", "json"]);
    assert_eq!(run.code, Some(1), "stderr={}", run.stderr);
    assert_eq!(json_stream(&run.stdout), reported(&expected));
}

#[test]
fn a_circuit_finding_one_edition_alone_raises_waits_for_that_edition_pinned() {
    let unpinned = check(QUASI_CONNECTED_DISPENSER, &[]);
    assert_eq!(unpinned.code, Some(0), "stderr={}", unpinned.stderr);
    assert_stream(&unpinned.stderr, &[]);
    for (edition, expected) in [
        ("java", (8, 3, "error", "E_NO_PORTABLE_IMPL")),
        ("bedrock", (9, 3, "error", "E_ROUTE_UNREACHABLE_COMPONENT")),
    ] {
        let run = check(QUASI_CONNECTED_DISPENSER, &["--edition", edition]);
        assert_eq!(run.code, Some(1), "{edition}: stderr={}", run.stderr);
        assert_stream(&run.stderr, &[expected]);
    }
}
