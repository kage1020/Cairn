//! Sources whose numbers are hostile must be refused, not crash or hang.
//!
//! `cairn check` accepts every input below, so the only thing standing
//! between an author's typo and the failure is the lowering pass. The
//! failures it used to produce were all of the kind a caller cannot recover
//! from:
//!
//! | symptom | how it showed up |
//! | --- | --- |
//! | panic | exit 101 — `expect` on a value read from the source, or overflowing arithmetic |
//! | allocation failure | the process aborted asking the allocator for tens of gigabytes |
//! | effectively hung | minutes of work for a one-line source |
//!
//! Each needs a subprocess to observe: two of them take the process down,
//! and the third never returns. Every run below therefore carries a
//! deadline, so a regression fails the suite instead of hanging CI.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

mod common;
use common::cargo_bin;

/// Generous next to the sub-second every legitimate source takes, tight
/// next to the minutes the unbounded shapes ran for.
const DEADLINE: Duration = Duration::from_secs(30);

/// The bound for a row that must be refused by arithmetic rather than by a
/// search. Far above the few milliseconds it costs, far below [`DEADLINE`].
const ANSWERED_WITHOUT_SEARCHING: Duration = Duration::from_secs(5);

/// What a run did. `TimedOut` is a distinct outcome rather than an error so
/// the assertion can name it: "hung" and "crashed" call for different fixes.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Exited(i32),
    /// Killed at the deadline, or died from a signal (which is how a Unix
    /// abort or stack overflow surfaces through `ExitStatus`).
    NoExitCode,
    TimedOut,
}

/// Run `cairn` with a deadline, returning what happened, its stderr, and
/// how long it took.
///
/// The elapsed time is returned rather than only compared against
/// [`DEADLINE`], because a row that must be *fast* and a row that must
/// merely *finish* want different bounds and the deadline is shared.
///
/// stderr goes to a file rather than a pipe: a pipe that fills while nobody
/// reads it deadlocks the child, which would look exactly like the hang
/// being tested for. stdout is discarded; [`run_bounded_with_stdout`]
/// keeps it.
fn run_bounded(dir: &Path, args: &[&str]) -> (Outcome, String, Duration) {
    run_bounded_to(dir, args, Stdio::null())
}

/// [`run_bounded`], keeping stdout as well, for a run whose output is what
/// the test reads.
///
/// stdout goes to a file for the same reason stderr does.
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
///
/// `HUT`'s front door sits inside its body's `x` span, and a body may not
/// reach past `i32`, so only a door on the `+x` wall can put a port one
/// step past the range on that axis.
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
///
/// `HUT`'s only door is on its front, so [`connected`] cannot put a port on
/// the `-z` side; this pair needs a door of its own on the back.
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

/// Two ports offset on *both* axes, via an `east_of=` / `north_of=` chain.
///
/// The single-axis pair above only ever spans a line, so a pair like this is
/// what tells a length bound apart from an area one.
fn connected_diagonally(gap: &str) -> String {
    source(&format!(
        "{HUT}site duo:\n\
         \x20\x20place id=a use=hut theme=t at=origin\n\
         \x20\x20place id=b use=hut theme=t east_of=a gap={gap}\n\
         \x20\x20place id=c use=hut theme=t north_of=b gap={gap}\n\
         \x20\x20connect a.entry to c.entry path=@path\n"
    ))
}

/// Every shape measured to crash, abort, or hang before the bounds existed,
/// with what it did.
///
/// The four groups are separate defects that happened to share a symptom:
/// an `expect` on source text, a saturating read, an unbounded derived
/// volume, and unchecked coordinate arithmetic. Keeping them in one table
/// is what makes it obvious when a fix closes one and leaves its neighbours
/// open — which is how the first three of them survived the initial audit.
fn hostile_sources() -> Vec<(&'static str, String)> {
    vec![
        // `PlaceId::new(..).expect(..)` — the invariant it names is not
        // enforced anywhere upstream.
        ("place-id-dot", place("\"home.1\"")),
        ("place-id-space", place("\"home 1\"")),
        ("place-id-colon", place("\"home:1\"")),
        ("place-id-empty", place("\"\"")),
        // Reads that saturated to `u32::MAX` instead of deferring.
        (
            "height-past-u32",
            source("struct tower size=3x3\n  walls mat_slot=wall height=5000000000\n"),
        ),
        (
            "overhang-past-u32",
            source("struct big size=3x3\n  roof kind=flat mat_slot=roof overhang=4294967295\n"),
        ),
        // In-range numbers whose product is not: the derived volume was
        // never bounded, so each of these asked the allocator for more
        // memory than the machine has.
        (
            "height-in-range",
            source("struct tower size=3x3\n  walls mat_slot=wall height=2147483647\n"),
        ),
        (
            "overhang-in-range",
            source("struct big size=9x7\n  roof kind=flat mat_slot=roof overhang=100000\n"),
        ),
        (
            "size-in-range",
            source("struct t size=100000x100000\n  floor mat_slot=floor\n"),
        ),
        (
            "size-i32-max",
            source("struct t size=2147483647x2147483647\n  floor mat_slot=floor\n"),
        ),
        (
            "size-u32-max",
            source("struct t size=4294967295x4294967295\n  floor mat_slot=floor\n"),
        ),
        (
            "level-y-in-range",
            source(
                "struct t size=3x3\n  level id=l y=2147483647\n    walls mat_slot=wall height=2\n",
            ),
        ),
        // Walkway port resolution added `i32`s without checking, and the
        // strip was materialised before anything measured it. The hut is 3
        // wide, so this puts `b`'s last column at exactly `i32::MAX` and
        // only the step out of its `+x` door to the port leaves the range.
        // `the_port_case_reaches_port_resolution` holds it there.
        ("gap-port-past-i32", connected_east_doors("2147483642")),
        // The same on the `-z` side: `b`'s origin lands at `z = 0 - 3 -
        // 2147483645 = i32::MIN` exactly, and only the step out to its back
        // door's port leaves the range.
        (
            "gap-port-past-i32-north",
            connected_back_to_back("2147483645"),
        ),
        // Here the origin itself leaves the range. That saturated
        // onto `i32::MAX` once; the row is refused now, before any port is
        // resolved against it.
        ("gap-origin-past-i32", connected("2147483647")),
        ("gap-large", connected("100000000")),
        // Area, not length. A single-axis pair only ever spans a line, so a
        // bound on path length looks sufficient — which is exactly the shape
        // that hid the leak. `gap=30000` on both axes is a 60001-cell path
        // and a 900-million-cell bounding box, and the bounding box is what
        // gets allocated: 32 seconds, roughly 1.8 GB.
        ("gap-diagonal", connected_diagonally("30000")),
        ("gap-diagonal-large", connected_diagonally("1000000")),
    ]
}

/// A `circuit region=` inside a struct whose `size=` is hostile.
///
/// The reservation takes the floor's extent, so `region=` is as wide as
/// `size=` and the actuator pad lands at `x = width - 1`: the driver
/// segment out to it is the author's `size=`. The rest of the file is
/// the smallest circuit that reaches the place-and-route passes at all
/// — two sensors, one gate, one actuator.
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

/// The same shapes put to the place-and-route passes, which the
/// commands above never reach.
///
/// `hostile_sources` runs `parse`, `check`, `lower`, `info` and
/// `compile`; none of them routes, so a reservation as wide as a
/// hostile `size=` was measured by nothing. Stage 2 laid a wire one
/// coord at a time out to the actuator pad — millions of them — and
/// nothing at that stage measured the result, so `--stage route` spent
/// minutes on the widest shape here and then exited 0 carrying a
/// netlist stage 3 would have refused. Only a run that went on to
/// stage 3 got the refusal, and only after laying the same wire again.
///
/// So this asks `--stage route` for the refusal: the stage that used to
/// answer slowly and wrongly is the one worth pinning.
fn hostile_circuits() -> Vec<(&'static str, String)> {
    vec![
        ("circuit-size-in-range", circuit("4000000x4000000")),
        ("circuit-size-i32-max", circuit("2147483647x2147483647")),
        ("circuit-size-u32-max", circuit("4294967295x4294967295")),
    ]
}

#[test]
fn hostile_4_routing_a_giant_reservation_answers_within_the_deadline() {
    let tmp = TempDir::new().expect("tempdir");
    for (name, body) in hostile_circuits() {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).expect("case dir");
        let path = write(&dir, name, &body);
        let file = path.to_str().unwrap();
        // Only `route` is asked. `delay` and `crossing` would exercise
        // nothing further: routing elides the scope and the CLI stops at
        // the first Error-severity stage, so all three spell the same
        // routing-side refusal. That every pass asks the same question
        // of the same shape is what `pass::tests::stages()` covers, from
        // an IR the CLI cannot hand them.
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
        // `Exited(1)` rather than `Exited(0 | 1)`: these widths must
        // always be refused, so accepting 0 would let a regression that
        // demotes the refusal to a warning pass unnoticed — the code
        // string below reaches stderr either way.
        assert!(
            matches!(outcome, Outcome::Exited(1)),
            "{name}: `synth --stage route` ended as {outcome:?}; a reservation the attenuation \
             cap cannot span must be refused, not routed\nstderr={stderr}",
        );
        // Exiting cleanly is not enough: a run that answered by dropping
        // the circuit would satisfy the line above while leaving the
        // author with nothing to act on.
        assert!(
            stderr.contains("E_ATTENUATION_LIMIT"),
            "{name}: `synth --stage route` must name the cap it could not meet; got {stderr:?}",
        );
        // The refusal is arithmetic on the reservation, so it is answered
        // before a single coordinate is searched — milliseconds, against
        // the minutes `canary` spends on the widest row. `DEADLINE` alone
        // does not say that: it is shared with the rows that only have to
        // finish, and 30 s cannot tell 4 ms from 29 s. The bound here is
        // three orders of magnitude above what the run costs, so it fails
        // on a regression that starts searching rather than on a slow
        // runner.
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
fn hostile_1_no_command_crashes_or_hangs() {
    let tmp = TempDir::new().expect("tempdir");
    for (name, body) in hostile_sources() {
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
fn hostile_2_lowering_says_which_member_it_gave_up_on() {
    // Exiting cleanly is not enough on its own: a source that silently
    // produced an empty build would satisfy the test above while leaving the
    // author with no idea why their walls are missing.
    let tmp = TempDir::new().expect("tempdir");
    for (name, body) in hostile_sources() {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).expect("case dir");
        let path = write(&dir, name, &body);
        let (outcome, stderr, _) = run_bounded(&dir, &["lower", path.to_str().unwrap()]);
        assert!(
            matches!(outcome, Outcome::Exited(0 | 1)),
            "{name}: ended as {outcome:?}",
        );
        assert!(
            stderr.contains("W_") || stderr.contains("E_"),
            "{name}: lowering must name a diagnostic code for what it dropped; got {stderr:?}",
        );
    }
}

#[test]
fn the_port_case_reaches_port_resolution() {
    // `hostile_2` only asks for some diagnostic code, which a row refused
    // for its origin satisfies without ever resolving a port. Each case is
    // held to the stage it is named for, so a change to either value that
    // moves it to the other stage fails here rather than going inert.
    let tmp = TempDir::new().expect("tempdir");
    let sources = hostile_sources();
    for (name, reached, not_reached) in [
        (
            "gap-port-past-i32",
            &["port `b.east` could not be placed"][..],
            "origin works out to",
        ),
        (
            "gap-port-past-i32-north",
            &["port `b.back` could not be placed"][..],
            "origin works out to",
        ),
        (
            "gap-origin-past-i32",
            &[
                "origin works out to x=2147483650",
                "the `b.entry` placement did not lower",
            ][..],
            "could not be placed",
        ),
    ] {
        let (_, body) = sources
            .iter()
            .find(|(case, _)| *case == name)
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
            reached.iter().all(|text| stderr.contains(text)) && !stderr.contains(not_reached),
            "{name}: expected {reached:?} and not `{not_reached}`; got {stderr}",
        );
    }
}

#[test]
fn hostile_3_compile_refuses_rather_than_certifying_the_wreckage() {
    // Whatever the lowering decides to drop, the lockfile must not end up
    // claiming the build succeeded. Sources that lose their only structure
    // are refused outright; the rest still have to exit cleanly.
    let tmp = TempDir::new().expect("tempdir");
    for (name, body) in hostile_sources() {
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

/// A walkway whose router search reaches the `i32::MAX` column.
///
/// The straight L from `b.east` to `c.west` crosses a floor, so the
/// router runs. With `gap=2147483640`, `b.east` sits at `x = i32::MAX - 1`,
/// the search rectangle's east margin is the `i32::MAX` column, and the
/// router expands a cell there. One block further east the margin itself
/// would leave `i32`, so the router refuses the rectangle, the lowering
/// falls back to the straight L and skips the cells it overlaps.
///
/// Both rows are kept so the first stays on the edge: one block west and
/// the router never reaches `i32::MAX`, so the case goes inert, and the
/// second row's warning would go with it; one block east and the first row
/// starts warning. The second row is also what shows the straight L
/// overlaps a floor, so the first row's lack of a warning means the router
/// laid a detour.
///
/// Standalone rather than `hostile_sources()` rows: the first row lowers
/// with no diagnostic at all, which `hostile_2` would reject.
#[test]
fn a_detour_searched_along_the_edge_of_i32_answers_rather_than_panics() {
    // `a` must have no floor: with one, its cells stretch the search
    // rectangle 2.1e9 blocks west, past the router's area cap, and both
    // rows fall back to the straight L.
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

/// A hut with a door on its front and on its back, so a `north_of=` pair
/// faces door to door along one column and the straight L between them
/// is a z leg alone, crossing no floor.
const THROUGH_HUT: &str = "def hut size=3x3:\n\
\x20\x20floor id=floor mat_slot=floor\n\
\x20\x20walls id=walls mat_slot=wall height=3\n\
\x20\x20door  id=front side=front at=center\n\
\x20\x20door  id=back  side=back  at=center\n\n";

/// A north–south walkway long enough that laying it in time quadratic
/// in its length would not finish.
///
/// Every walkway row in [`hostile_sources`] is refused before a strip is
/// laid: at the `i32` edge, or by the router's 4,000,000-cell area cap.
/// The cap measures the straight L's bounding box (`l_path_area`), so the
/// single-axis `gap-large`, a strip of about 100,000,000 cells, is past it
/// as surely as the rows that span both axes. None of them reaches the z
/// leg of the straight L. This one does: a million-cell z strip is one
/// cell wide, so its bounding box sits under the cap, and that margin is
/// the premise of the test. It is laid rather than refused, which is why
/// it stands alone:
/// `hostile_2` requires every row to name a diagnostic, and this row earns
/// none.
///
/// `gap=1000000` is a million-cell strip. Laid in linear time it lowers
/// far inside [`DEADLINE`]; a lookup over the laid cells per step would
/// make that half a trillion comparisons, and the run would reach the
/// deadline instead.
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
    // Silence alone does not say the strip was laid: a row dropped without
    // a finding would leave it silent too. Read the walkway back, so the
    // run above is known to have timed the z leg rather than a refusal.
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

/// A sink walled in two blocks from its driver, in a reservation as large
/// as the `size=` and `void=` make it.
///
/// The sink that strands is a cell two blocks from its driver, every face
/// of which is taken by a block, another net's dust, or the coords beside
/// that dust. Nothing bounded the search, so the router searched until
/// it had visited every free coord the net could reach: seconds and
/// hundreds of megabytes per million coords of reservation, set by
/// `width × depth × void` and not by the two blocks between the ends.
/// Raising `void`, which the refusal's own fix line suggests, made the
/// next run slower and the answer the same.
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
fn hostile_5_a_walled_in_sink_is_refused_without_searching_the_reservation() {
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
    // Still the wall, not the distance: every face of the sink is taken,
    // so it is walled in whatever the cap allows.
    assert!(
        stderr.contains("E_ROUTE_CONGESTION") && stderr.contains("cannot reach (5,0,1)"),
        "got {stderr:?}",
    );
    // 12.4 million coords of reservation. Searched, it ran past
    // `DEADLINE`; answered from the sink's faces, or from a search the
    // cap bounds, it is far inside this.
    assert!(
        elapsed < ANSWERED_WITHOUT_SEARCHING,
        "`synth --stage route` took {elapsed:?} to refuse a sink two blocks from its driver",
    );
}
