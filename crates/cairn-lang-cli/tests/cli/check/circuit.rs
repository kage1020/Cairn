//! `cairn check` builds each struct's circuit as `cairn compile` does and reports its findings,
//! unpinned only where every edition raises them at the same place or one edition's guards hold
//! them.

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

/// The raw block only Bedrock builds stands beside the lamp, so the block that lights the lamp
/// would power it too.
const GUARDED_NEIGHBOUR: &str = concat!(
    "theme t:\n",
    "  slot wall -> @oak_planks\n",
    "\n",
    "struct room size=15x11\n",
    "  floor mat_slot=wall\n",
    "  walls mat_slot=wall height=3\n",
    "  lever id=l at=inside.back offset=4 y=0 -> sig.a\n",
    "  lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a\n",
    "  circuit region=floor void=3\n",
    "  @edition bedrock\n",
    "    raw_block mat=minecraft:stone at=2,0,4\n",
);

#[test]
fn a_circuit_finding_a_member_under_one_editions_guard_causes_is_that_editions_unpinned() {
    let refused = (8, 3, "error", "E_ROUTE_UNREACHABLE_COMPONENT");
    for (args, code, expected) in [
        (&[][..], 1, &[refused][..]),
        (&["--edition", "bedrock"], 1, &[refused]),
        (&["--edition", "java"], 0, &[]),
    ] {
        let run = check(GUARDED_NEIGHBOUR, args);
        assert_eq!(run.code, Some(code), "{args:?}: stderr={}", run.stderr);
        assert_stream(&run.stderr, expected);
    }
}
