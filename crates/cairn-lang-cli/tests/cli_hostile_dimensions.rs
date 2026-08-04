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

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

/// Generous next to the sub-second every legitimate source takes, tight
/// next to the minutes the unbounded shapes ran for.
const DEADLINE: Duration = Duration::from_secs(30);

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

/// Run `cairn` with a deadline, returning what happened and its stderr.
///
/// stderr goes to a file rather than a pipe: a pipe that fills while nobody
/// reads it deadlocks the child, which would look exactly like the hang
/// being tested for.
fn run_bounded(dir: &Path, args: &[&str]) -> (Outcome, String) {
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
    let stderr = fs::read_to_string(&err_path).unwrap_or_default();
    (outcome, stderr)
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
        // path was materialised before anything looked at its length.
        ("gap-i32-max", connected("2147483647")),
        ("gap-large", connected("100000000")),
    ]
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
            let (outcome, stderr) = run_bounded(&dir, &args);
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
        let (outcome, stderr) = run_bounded(&dir, &["lower", path.to_str().unwrap()]);
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
        let (outcome, stderr) = run_bounded(
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
