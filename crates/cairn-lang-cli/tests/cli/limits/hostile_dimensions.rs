//! Sources whose numbers are hostile are refused with a finding, not a panic, an abort or a hang.
//! Each run is a subprocess with a deadline, so a crash or a hang fails the suite.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

use crate::common::{assert_stream, cargo_bin};

/// Generous next to a legitimate source's sub-second, tight next to an unbounded shape's minutes.
const DEADLINE: Duration = Duration::from_secs(30);

/// The bound for a row that must be refused by arithmetic rather than by a search.
const ANSWERED_WITHOUT_SEARCHING: Duration = Duration::from_secs(5);

/// What a run did; "hung" and "crashed" call for different fixes.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Exited(i32),
    /// Killed at the deadline, or died from a signal, as a Unix abort or stack overflow does.
    NoExitCode,
    TimedOut,
}

/// Run `cairn` with a deadline, returning the outcome, stderr and elapsed time.
///
/// stderr goes to a file, since a full pipe nobody reads deadlocks the child like a hang.
fn run_bounded(dir: &Path, args: &[&str]) -> (Outcome, String, Duration) {
    run_bounded_to(dir, args, Stdio::null())
}

/// [`run_bounded`], keeping stdout too, in a file for the same reason.
fn run_bounded_with_stdout(dir: &Path, args: &[&str]) -> (Outcome, String, String, Duration) {
    let out_path = dir.join("stdout.txt");
    let out_file = File::create(&out_path).expect("create stdout sink");
    let (outcome, stderr, elapsed) = run_bounded_to(dir, args, Stdio::from(out_file));
    let stdout = fs::read_to_string(&out_path).unwrap_or_default();
    (outcome, stderr, stdout, elapsed)
}

fn run_bounded_to(dir: &Path, args: &[&str], stdout: Stdio) -> (Outcome, String, Duration) {
    let err_path = dir.join("stderr.txt");
    let err_file = File::create(&err_path).expect("create stderr sink");
    let mut child = Command::new(cargo_bin())
        .args(args)
        .stdout(stdout)
        .stderr(Stdio::from(err_file))
        .spawn()
        .expect("spawn cairn");

    let started = Instant::now();
    let outcome = loop {
        match child.try_wait().expect("poll child") {
            Some(status) => {
                break status.code().map_or(Outcome::NoExitCode, Outcome::Exited);
            }
            None if started.elapsed() >= DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                break Outcome::TimedOut;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let elapsed = started.elapsed();
    let stderr = fs::read_to_string(&err_path).unwrap_or_default();
    (outcome, stderr, elapsed)
}

const THEME: &str = "theme t:\n\
\x20\x20slot floor -> @oak_planks\n\
\x20\x20slot wall  -> @cobblestone\n\
\x20\x20slot roof  -> @spruce_stairs\n\
\x20\x20slot path  -> @gravel\n\n";

const HUT: &str = "def hut size=3x3:\n\
\x20\x20floor id=floor mat_slot=floor\n\
\x20\x20walls id=walls mat_slot=wall height=3\n\
\x20\x20door  id=entry side=front at=center\n\n";

fn source(body: &str) -> String {
    format!("@cairn 2026.06\n\n{THEME}{body}")
}

fn place(id: &str) -> String {
    source(&format!(
        "{HUT}site hamlet:\n  place id={id} use=hut theme=t at=origin\n"
    ))
}

fn connected(gap: &str) -> String {
    source(&format!(
        "{HUT}site duo:\n  place id=a use=hut theme=t at=origin\n\
         \x20\x20place id=b use=hut theme=t east_of=a gap={gap}\n\
         \x20\x20connect a.entry to b.entry path=@path\n"
    ))
}

/// Two huts with a door on their `+x` wall, `b` placed `east_of=a`.
fn connected_east_doors(gap: &str) -> String {
    source(&format!(
        "def east_door_hut size=3x3:\n\
         \x20\x20walls id=walls mat_slot=wall height=3\n\
         \x20\x20door  id=east side=right at=center\n\n\
         site duo:\n\
         \x20\x20place id=a use=east_door_hut theme=t at=origin\n\
         \x20\x20place id=b use=east_door_hut theme=t east_of=a gap={gap}\n\
         \x20\x20connect a.east to b.east path=@path\n"
    ))
}

/// Two huts whose doors face each other's backs along `z`, via `north_of=`.
fn connected_back_to_back(gap: &str) -> String {
    source(&format!(
        "def back_door_hut size=3x3:\n\
         \x20\x20walls id=walls mat_slot=wall height=3\n\
         \x20\x20door  id=back side=back at=center\n\n\
         site duo:\n\
         \x20\x20place id=a use=back_door_hut theme=t at=origin\n\
         \x20\x20place id=b use=back_door_hut theme=t north_of=a gap={gap}\n\
         \x20\x20connect a.back to b.back path=@path\n"
    ))
}

/// Two ports offset on both axes, which tells a length bound apart from an area one.
fn connected_diagonally(gap: &str) -> String {
    source(&format!(
        "{HUT}site duo:\n\
         \x20\x20place id=a use=hut theme=t at=origin\n\
         \x20\x20place id=b use=hut theme=t east_of=a gap={gap}\n\
         \x20\x20place id=c use=hut theme=t north_of=b gap={gap}\n\
         \x20\x20connect a.entry to c.entry path=@path\n"
    ))
}

/// The findings `lower` gives up on a hostile row with, in emission order.
type Findings = &'static [(u64, u64, &'static str, &'static str)];

/// Every shape that, unbounded, crashes, aborts or hangs, with the findings `lower` reports.
fn hostile_sources() -> Vec<(&'static str, String, Findings)> {
    const PLACE_ID: Findings = &[(15, 3, "error", "E_INVALID_PLACE_ID")];
    const TOO_LARGE: Findings = &[(9, 1, "warning", "W_STRUCTURE_TOO_LARGE")];
    const PORT_DEFERRED: Findings = &[(16, 3, "warning", "W_DEFERRED_MEMBER")];
    const PLACE_DEFERRED: Findings = &[
        (16, 3, "warning", "W_DEFERRED_MEMBER"),
        (17, 3, "warning", "W_DEFERRED_MEMBER"),
    ];
    vec![
        // `PlaceId::new` holds an invariant nothing upstream enforces.
        ("place-id-dot", place("\"home.1\""), PLACE_ID),
        ("place-id-space", place("\"home 1\""), PLACE_ID),
        ("place-id-colon", place("\"home:1\""), PLACE_ID),
        ("place-id-empty", place("\"\""), PLACE_ID),
        // Numbers past `u32`, which a saturating read would clamp instead of refusing.
        (
            "height-past-u32",
            source("struct tower size=3x3\n  walls mat_slot=wall height=5000000000\n"),
            &[(10, 3, "error", "E_INVALID_MEMBER")],
        ),
        (
            "overhang-past-u32",
            source("struct big size=3x3\n  roof kind=flat mat_slot=roof overhang=4294967295\n"),
            TOO_LARGE,
        ),
        // In-range numbers whose product is not.
        (
            "height-in-range",
            source("struct tower size=3x3\n  walls mat_slot=wall height=2147483647\n"),
            TOO_LARGE,
        ),
        (
            "overhang-in-range",
            source("struct big size=9x7\n  roof kind=flat mat_slot=roof overhang=100000\n"),
            TOO_LARGE,
        ),
        (
            "size-in-range",
            source("struct t size=100000x100000\n  floor mat_slot=floor\n"),
            TOO_LARGE,
        ),
        (
            "size-i32-max",
            source("struct t size=2147483647x2147483647\n  floor mat_slot=floor\n"),
            TOO_LARGE,
        ),
        (
            "size-u32-max",
            source("struct t size=4294967295x4294967295\n  floor mat_slot=floor\n"),
            TOO_LARGE,
        ),
        (
            "level-y-in-range",
            source(
                "struct t size=3x3\n  level id=l y=2147483647\n    walls mat_slot=wall height=2\n",
            ),
            TOO_LARGE,
        ),
        // `b` ends at `i32::MAX`, so only the step out of its `+x` door leaves the range.
        (
            "gap-port-past-i32",
            connected_east_doors("2147483642"),
            PORT_DEFERRED,
        ),
        // `b`'s origin lands at `i32::MIN`, so only the step out to its back door's port leaves it.
        (
            "gap-port-past-i32-north",
            connected_back_to_back("2147483645"),
            PORT_DEFERRED,
        ),
        // `b`'s last column lands one past `i32::MAX`, so the row is refused before any port.
        ("gap-body-past-i32", connected("2147483643"), PLACE_DEFERRED),
        // The origin itself leaves the range.
        (
            "gap-origin-past-i32",
            connected("2147483647"),
            PLACE_DEFERRED,
        ),
        (
            "gap-large",
            connected("100000000"),
            &[(17, 3, "warning", "W_WALKWAY_BLOCKED")],
        ),
        // Area, not length: a 60001-cell path in a 900-million-cell bounding box.
        (
            "gap-diagonal",
            connected_diagonally("30000"),
            &[(18, 3, "warning", "W_WALKWAY_BLOCKED")],
        ),
        (
            "gap-diagonal-large",
            connected_diagonally("1000000"),
            &[(18, 3, "warning", "W_WALKWAY_BLOCKED")],
        ),
    ]
}

/// A `circuit region=` inside a struct whose `size=` is hostile, so the driver segment is too.
fn circuit(size: &str) -> String {
    let body = [
        format!("struct big size={size}"),
        "  floor mat_slot=floor".to_owned(),
        "  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a".to_owned(),
        "  pressure_plate id=pb at=inside.front offset=1 y=0 -> sig.b".to_owned(),
        "  logic sig.o = sig.a and sig.b".to_owned(),
        "  door id=d side=front at=center mat_slot=wall opened_by=sig.o".to_owned(),
        "  circuit region=floor void=3".to_owned(),
    ]
    .join("\n");
    source(&format!("{body}\n"))
}

/// The same shapes put to place-and-route, which `--stage route` has to refuse by arithmetic.
fn hostile_circuits() -> Vec<(&'static str, String)> {
    vec![
        ("circuit-size-in-range", circuit("4000000x4000000")),
        ("circuit-size-i32-max", circuit("2147483647x2147483647")),
        ("circuit-size-u32-max", circuit("4294967295x4294967295")),
    ]
}

#[test]
fn routing_a_giant_reservation_answers_within_the_deadline() {
    let tmp = TempDir::new().expect("tempdir");
    for (name, body) in hostile_circuits() {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).expect("case dir");
        let path = write(&dir, name, &body);
        let file = path.to_str().unwrap();
        // Only `route`: the CLI stops at the first stage with an error.
        let (outcome, stderr, elapsed) = run_bounded(
            &dir,
            &[
                "synth",
                file,
                "--stage",
                "route",
                "--edition",
                "java",
                "--experimental-logic-synth",
            ],
        );
        // Not `Exited(0 | 1)`: a refusal demoted to a warning would put the same code on stderr.
        assert!(
            matches!(outcome, Outcome::Exited(1)),
            "{name}: `synth --stage route` ended as {outcome:?}; a reservation the attenuation \
             cap cannot span must be refused, not routed\nstderr={stderr}",
        );
        assert_stream(
            &stderr,
            &[
                (14, 43, "warning", "W_IGNORED_ARGUMENT"),
                (15, 3, "error", "E_ATTENUATION_LIMIT"),
            ],
        );
        assert!(
            elapsed < ANSWERED_WITHOUT_SEARCHING,
            "{name}: `synth --stage route` took {elapsed:?}; refusing on the straight line is \
             arithmetic, so anything near {ANSWERED_WITHOUT_SEARCHING:?} means a search ran",
        );
    }
}

fn write(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(format!("{name}.crn"));
    fs::write(&path, source).expect("write source");
    path
}

#[test]
fn no_command_crashes_or_hangs() {
    let tmp = TempDir::new().expect("tempdir");
    for (name, body, _) in hostile_sources() {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).expect("case dir");
        let path = write(&dir, name, &body);
        let file = path.to_str().unwrap();
        let out_dir = dir.join("out");

        for args in [
            vec!["parse", file],
            vec!["check", file],
            vec!["lower", file],
            vec!["info", file],
            vec![
                "compile",
                file,
                "--edition",
                "java",
                "--out",
                out_dir.to_str().unwrap(),
            ],
        ] {
            let (outcome, stderr, _) = run_bounded(&dir, &args);
            assert!(
                matches!(outcome, Outcome::Exited(0 | 1)),
                "{name}: `{}` ended as {outcome:?}; a hostile number must produce a \
                 diagnostic, not a crash or a hang\nstderr={stderr}",
                args[0],
            );
        }
    }
}

#[test]
fn lowering_says_which_member_it_gave_up_on() {
    // A source that silently built nothing would pass the test above.
    let tmp = TempDir::new().expect("tempdir");
    for (name, body, findings) in hostile_sources() {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).expect("case dir");
        let path = write(&dir, name, &body);
        let (outcome, stderr, _) = run_bounded(&dir, &["lower", path.to_str().unwrap()]);
        assert!(
            matches!(outcome, Outcome::Exited(0 | 1)),
            "{name}: ended as {outcome:?}",
        );
        assert_stream(&stderr, findings);
    }
}

#[test]
fn the_port_case_reaches_port_resolution() {
    // Each case defers the same member at the same position, so only the message tells them apart.
    let tmp = TempDir::new().expect("tempdir");
    let sources = hostile_sources();
    for (name, reached, not_reached) in [
        (
            "gap-port-past-i32",
            &["port `b.east` could not be placed"][..],
            &["origin works out to", "body reaches"][..],
        ),
        (
            "gap-port-past-i32-north",
            &["port `b.back` could not be placed"][..],
            &["origin works out to", "body reaches"][..],
        ),
        (
            "gap-body-past-i32",
            &[
                "body reaches x=2147483648",
                "the `b.entry` placement did not lower",
            ][..],
            &["could not be placed", "origin works out to"][..],
        ),
        (
            "gap-origin-past-i32",
            &[
                "origin works out to x=2147483650",
                "the `b.entry` placement did not lower",
            ][..],
            &["could not be placed", "body reaches"][..],
        ),
    ] {
        let (_, body, _) = sources
            .iter()
            .find(|(case, _, _)| *case == name)
            .unwrap_or_else(|| panic!("no hostile source named {name}"));
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).expect("case dir");
        let path = write(&dir, name, body);
        let (outcome, stderr, _) = run_bounded(&dir, &["lower", path.to_str().unwrap()]);
        assert!(
            matches!(outcome, Outcome::Exited(0 | 1)),
            "{name}: ended as {outcome:?}",
        );
        assert!(
            reached.iter().all(|text| stderr.contains(text))
                && !not_reached.iter().any(|text| stderr.contains(text)),
            "{name}: expected {reached:?} and none of {not_reached:?}; got {stderr}",
        );
    }
}

#[test]
fn compile_refuses_rather_than_certifying_the_wreckage() {
    let tmp = TempDir::new().expect("tempdir");
    for (name, body, _) in hostile_sources() {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).expect("case dir");
        let path = write(&dir, name, &body);
        let out_dir = dir.join("out");
        let (outcome, stderr, _) = run_bounded(
            &dir,
            &[
                "compile",
                path.to_str().unwrap(),
                "--edition",
                "java",
                "--out",
                out_dir.to_str().unwrap(),
            ],
        );
        let lock = path.with_extension("crn.lock");
        match outcome {
            Outcome::Exited(0) => assert!(
                lock.exists(),
                "{name}: a successful compile writes its lockfile",
            ),
            Outcome::Exited(1) => assert!(
                !lock.exists(),
                "{name}: a refused compile must not certify the source\nstderr={stderr}",
            ),
            other => panic!("{name}: ended as {other:?}\nstderr={stderr}"),
        }
    }
}

#[test]
fn a_body_past_i32_fails_the_build_it_is_part_of() {
    let sources = hostile_sources();
    let (name, body, _) = sources
        .iter()
        .find(|(case, _, _)| *case == "gap-body-past-i32")
        .expect("the hostile source with a body past `i32`");
    let tmp = TempDir::new().expect("tempdir");
    let path = write(tmp.path(), name, body);
    let file = path.to_str().unwrap();
    let out_dir = tmp.path().join("out");
    let out = out_dir.to_str().unwrap();
    for args in [
        vec!["check", file, "--edition", "java", "--target", "1.21.4"],
        vec!["compile", file, "--edition", "java", "--out", out],
    ] {
        let (outcome, stderr, _) = run_bounded(tmp.path(), &args);
        assert_eq!(
            outcome,
            Outcome::Exited(1),
            "`{}` must refuse a build missing a row\nstderr={stderr}",
            args[0],
        );
        for text in [
            "this placement's body reaches x=2147483648",
            "error[E_PARTIAL_BUILD]",
            "`site::duo::b` produced no voxels",
        ] {
            assert!(
                stderr.contains(text),
                "`{}` must say `{text}`; got {stderr}",
                args[0],
            );
        }
    }
}

/// With `gap=2147483640` the search rectangle's east margin is the `i32::MAX` column. The second
/// row's warning shows the straight L overlaps a floor, so the first row's silence means a detour.
#[test]
fn a_detour_searched_along_the_edge_of_i32_answers_rather_than_panics() {
    // `a` must have no floor, or its cells stretch the search past the router's area cap.
    let detour = |gap: &str| {
        source(&format!(
            "def shell size=3x3:\n\
             \x20\x20walls id=walls mat_slot=wall height=3\n\n\
             def floored_hut size=3x3:\n\
             \x20\x20floor id=floor mat_slot=floor\n\
             \x20\x20walls id=walls mat_slot=wall height=3\n\
             \x20\x20door  id=east side=right at=center\n\
             \x20\x20door  id=west side=left  at=center\n\n\
             site s:\n\
             \x20\x20place id=a use=shell       theme=t at=origin\n\
             \x20\x20place id=b use=floored_hut theme=t east_of=a gap={gap}\n\
             \x20\x20place id=c use=floored_hut theme=t north_of=b gap=2\n\
             \x20\x20connect b.east to c.west path=@gravel\n"
        ))
    };
    let tmp = TempDir::new().expect("tempdir");
    for (name, gap, warning) in [
        ("detour-at-the-edge", "2147483640", None),
        (
            "margin-past-the-edge",
            "2147483641",
            Some([
                "W_WALKWAY_BLOCKED",
                "the walkway endpoints sit at the edge of the representable coordinate space",
            ]),
        ),
    ] {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).expect("case dir");
        let path = write(&dir, name, &detour(gap));
        let file = path.to_str().unwrap();
        let out_dir = dir.join("out");
        let out = out_dir.to_str().unwrap();
        for args in [
            vec!["check", file, "--edition", "java", "--target", "1.21.4"],
            vec!["lower", file],
            vec!["compile", file, "--edition", "java", "--out", out],
        ] {
            let (outcome, stderr, _) = run_bounded(&dir, &args);
            assert_eq!(
                outcome,
                Outcome::Exited(0),
                "{name}: `{}` must answer, not crash\nstderr={stderr}",
                args[0],
            );
            match warning {
                None => assert!(
                    !stderr.contains("W_"),
                    "{name}: `{}` lays the detour, so it has nothing to warn about; got {stderr}",
                    args[0],
                ),
                Some(texts) => assert!(
                    texts.iter().all(|text| stderr.contains(text)),
                    "{name}: `{}` must say the router refused the edge; got {stderr}",
                    args[0],
                ),
            }
        }
        assert!(
            out_dir.join("s_walkway_b_east__c_west.nbt").exists(),
            "{name}: the walkway is laid and written either way",
        );
    }
}

/// A body past the range is refused before its floor is laid, so this one ends on the edge cell.
#[test]
fn a_blocked_port_on_a_body_at_the_i32_edge_answers_rather_than_panics() {
    let src = source(
        "def hut size=5x5:\n\
         \x20\x20floor id=f mat_slot=wall\n\
         \x20\x20walls id=w mat_slot=wall height=3\n\
         \x20\x20roof  kind=flat mat_slot=wall overhang=1\n\
         \x20\x20door  id=e  side=front at=center\n\
         \x20\x20door  id=bk side=back  at=center\n\
         \x20\x20pressure_plate id=pp at=front.outside offset=4 y=0\n\n\
         site s:\n\
         \x20\x20place id=a use=hut theme=t at=origin\n\
         \x20\x20place id=b use=hut theme=t east_of=a gap=2147483634\n\
         \x20\x20place id=c use=hut theme=t north_of=b gap=4\n\
         \x20\x20connect b.e to c.bk path=@gravel\n",
    );
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path().join("port-at-i32-edge");
    fs::create_dir_all(&dir).expect("case dir");
    let path = write(&dir, "port-at-i32-edge", &src);
    let file = path.to_str().unwrap();
    let out_dir = dir.join("out");
    let out = out_dir.to_str().unwrap();
    for args in [
        vec!["check", file, "--edition", "java", "--target", "1.21.4"],
        vec!["lower", file],
        vec!["compile", file, "--edition", "java", "--out", out],
    ] {
        let (outcome, stderr, _) = run_bounded(&dir, &args);
        assert_eq!(
            outcome,
            Outcome::Exited(0),
            "`{}` must answer, not crash\nstderr={stderr}",
            args[0],
        );
        assert_stream(&stderr, &[(21, 3, "warning", "W_WALKWAY_BLOCKED")]);
        assert!(
            stderr.contains("warning[W_WALKWAY_BLOCKED]: walkway `b.e ↔ c.bk`"),
            "`{}` must report the blocked walkway; got {stderr}",
            args[0],
        );
    }
}

/// A hut with a door front and back, so a `north_of=` pair's straight L crosses no floor.
const THROUGH_HUT: &str = "def hut size=3x3:\n\
\x20\x20floor id=floor mat_slot=floor\n\
\x20\x20walls id=walls mat_slot=wall height=3\n\
\x20\x20door  id=front side=front at=center\n\
\x20\x20door  id=back  side=back  at=center\n\n";

/// A million-cell z strip is one cell wide, under the router's area cap, so it is laid with no
/// finding and only its time shows a quadratic lowering.
#[test]
fn a_long_north_south_walkway_lowers_within_the_deadline() {
    let body = source(&format!(
        "{THROUGH_HUT}site duo:\n\
         \x20\x20place id=a use=hut theme=t at=origin\n\
         \x20\x20place id=b use=hut theme=t north_of=a gap=1000000\n\
         \x20\x20connect a.back to b.front path=@gravel\n"
    ));
    let tmp = TempDir::new().expect("tempdir");
    let path = write(tmp.path(), "north-south", &body);
    let (outcome, stderr, stdout, elapsed) = run_bounded_with_stdout(
        tmp.path(),
        &["lower", path.to_str().unwrap(), "--format", "json"],
    );
    assert_eq!(
        outcome,
        Outcome::Exited(0),
        "`lower` ended as {outcome:?} after {elapsed:?}; a z strip must be laid in time linear \
         in its length\nstderr={stderr}",
    );
    assert!(
        !stderr.contains("W_") && !stderr.contains("E_"),
        "the walkway must be laid, not refused; got {stderr}",
    );
    // A row dropped without a finding is silent too, so read the strip back.
    let ir: serde_json::Value = serde_json::from_str(&stdout).expect("`lower` prints JSON");
    let key = "walkway::duo::a.back__b.front";
    let walkways = ir["walkways"].as_object().expect("a `walkways` map");
    assert_eq!(
        walkways.keys().collect::<Vec<_>>(),
        [key],
        "one walkway, for the one row"
    );
    assert_eq!(
        walkways[key]["footprint"],
        serde_json::json!({ "x": 1, "z": 1_000_000 }),
    );
    let strip = &ir["structures"][key];
    assert_eq!(
        strip["palette"],
        serde_json::json!([{ "id": "minecraft:air" }, { "id": "minecraft:gravel" }]),
    );
    let voxels = strip["voxels"].as_array().expect("a voxel array");
    assert_eq!(voxels.len(), 1_000_000);
    assert!(
        voxels.iter().all(|index| index == 1),
        "every cell of the strip is gravel"
    );
}

/// A sink walled in two blocks from its driver, in a reservation as large as `size=` and `void=`
/// make it.
fn walled_in_near_sink() -> String {
    let body = [
        "theme t:",
        "  slot wall -> @oak_planks",
        "  slot door -> @oak_door",
        "",
        "struct s size=31x2000",
        "  floor mat_slot=wall",
        "  door id=d0 side=front at=center mat_slot=door",
        "  door id=d1 side=back at=center mat_slot=door",
        "  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a",
        "  logic sig.g0 = (sig.a or sig.a) or (sig.a and sig.a)",
        "  logic sig.g1 = sig.g0 and sig.a",
        "  door[id=d0] opened_by=sig.g0",
        "  door[id=d1] opened_by=sig.g1",
        "  circuit region=floor void=200",
    ];
    format!("{}\n", body.join("\n"))
}

#[test]
fn a_walled_in_sink_is_refused_without_searching_the_reservation() {
    let tmp = TempDir::new().expect("tempdir");
    let path = write(tmp.path(), "walled", &walled_in_near_sink());
    let (outcome, stderr, elapsed) = run_bounded(
        tmp.path(),
        &[
            "synth",
            path.to_str().unwrap(),
            "--stage",
            "route",
            "--edition",
            "java",
            "--experimental-logic-synth",
        ],
    );
    assert!(
        matches!(outcome, Outcome::Exited(1)),
        "`synth --stage route` ended as {outcome:?}\nstderr={stderr}",
    );
    // Congestion, not distance: the sink is walled in whatever the cap allows.
    assert_stream(
        &stderr,
        &[
            (7, 44, "warning", "W_IGNORED_ARGUMENT"),
            (8, 43, "warning", "W_IGNORED_ARGUMENT"),
            (14, 3, "error", "E_ROUTE_CONGESTION"),
        ],
    );
    assert!(stderr.contains("cannot reach (5,0,1)"), "got {stderr:?}");
    // 12.4 million coords of reservation: searched, it runs past `DEADLINE`.
    assert!(
        elapsed < ANSWERED_WITHOUT_SEARCHING,
        "`synth --stage route` took {elapsed:?} to refuse a sink two blocks from its driver",
    );
}
