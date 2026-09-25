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
/// being tested for.
fn run_bounded(dir: &Path, args: &[&str]) -> (Outcome, String, Duration) {
    let err_path = dir.join("stderr.txt");
    let err_file = File::create(&err_path).expect("create stderr sink");
    let mut child = Command::new(cargo_bin())
        .args(args)
        .stdout(Stdio::null())
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
        // strip was materialised before anything measured it.
        ("gap-i32-max", connected("2147483647")),
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
        "  pressure_plate id=pb at=inside.front offset=0 y=0 -> sig.b".to_owned(),
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
