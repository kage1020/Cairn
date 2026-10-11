//! `cairn synth`: the opt-in gate, the `--stage` / `--edition` usage
//! errors and the order they are decided in, each stage's JSON dump, and
//! the exit code a refused circuit leaves.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use cairn_lang_core::{Edition, IntentModule, lower, parse};
use cairn_lang_redstone::{
    Diagnostic, compile_crossing, compile_delay, compile_edition_netlist, compile_netlist,
    compile_placement, compile_routing, synthesize,
};
use serde_json::Value;

use crate::common::{cairn, examples_dir, stderr_stream, write_source};

/// The edition-tagged stage the usage tests run: which stages need
/// `--edition` is pinned inside the binary, and the gate in front of
/// them is one gate.
const TAGGED_STAGE: &str = "placement";

/// The edition-neutral stage the usage tests run, on the same terms as
/// [`TAGGED_STAGE`].
const NEUTRAL_STAGE: &str = "netlist";

#[test]
fn cli_synth_requires_experimental_flag() {
    // The subcommand is internal-tier; a caller invoking it without the
    // opt-in flag must exit 2 with a usage hint on stderr so the gate
    // cannot be missed silently.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn("synth", &[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("--experimental-logic-synth"),
        "gate diagnostic should name the required flag, got: {stderr}",
    );
}

#[test]
fn the_opt_in_gate_is_decided_ahead_of_the_edition_gate() {
    // With no `--stage` the default `logic` is edition-neutral, so leaving
    // `--edition` out is right there and neither `--edition` refusal has
    // anything to say. Here each side of the partition is named with the
    // `--edition` it refuses, so an `--edition` refusal would apply too;
    // the opt-in gate stands first, and its line is the only one.
    let path = examples_dir().join("redstone-door.crn");
    for args in [
        vec!["--stage", TAGGED_STAGE],
        vec!["--stage", NEUTRAL_STAGE, "--edition", "java"],
    ] {
        let mut args = args;
        args.push(path.to_str().unwrap());
        let out = cairn("synth", &args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(2),
            "synth {args:?} without the opt-in flag should exit 2, got stderr: {stderr}",
        );
        assert!(
            out.stdout.is_empty(),
            "synth {args:?} without the opt-in flag must print no IR, got: {}",
            String::from_utf8_lossy(&out.stdout),
        );
        let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(
            lines.len(),
            1,
            "synth {args:?}: stderr should be the opt-in refusal alone, got: {stderr}",
        );
        assert!(
            lines[0].contains("--experimental-logic-synth"),
            "synth {args:?}: the one line should be the opt-in refusal, not an --edition one, \
             got: {stderr}",
        );
    }
}

#[test]
fn an_edition_tagged_stage_without_edition_is_a_usage_error() {
    // The stage reads the target edition's cell library, so running it
    // without one would silently pick a default the caller did not choose.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            TAGGED_STAGE,
            path.to_str().unwrap(),
        ],
    );
    assert_missing_edition_usage_error(&out, TAGGED_STAGE, "on redstone-door.crn");
}

#[test]
fn the_missing_edition_is_reported_ahead_of_the_sources_findings() {
    // Each source has a finding (see `FINDING_SOURCES`), and the usage
    // error still has to be the one line, with the usage exit code. That
    // holds for the warning-only source too, which does not stop a run:
    // its warning is not printed ahead of the usage error.
    //
    // Each source is also run with `--edition java`, the CONTROL: without
    // the usage error in the way, its finding is what surfaces, with the
    // exit code its severity gives. A source that stopped carrying its
    // finding would still pass the usage-error assertions.
    let dir = tempfile::tempdir().expect("temp dir");
    for (name, path, code) in &write_finding_sources(dir.path()) {
        let out = cairn(
            "synth",
            &[
                "--experimental-logic-synth",
                "--stage",
                TAGGED_STAGE,
                path.to_str().unwrap(),
            ],
        );
        assert_missing_edition_usage_error(&out, TAGGED_STAGE, &format!("on {name}"));

        let out = cairn(
            "synth",
            &[
                "--experimental-logic-synth",
                "--stage",
                TAGGED_STAGE,
                "--edition",
                "java",
                path.to_str().unwrap(),
            ],
        );
        assert_reports_finding(
            &out,
            code,
            &format!("--stage {TAGGED_STAGE} --edition java on {name}"),
        );
    }
}

#[test]
fn the_missing_edition_is_reported_ahead_of_a_missing_file() {
    // A path that names nothing exits 2, as the usage error does, so the
    // exit code cannot tell the two apart and only stderr can. The usage
    // error is decided from argv alone, before the file is read, so it is
    // the one a caller who got both wrong hears first, and the read is
    // never attempted.
    //
    // The CONTROL runs the same path with `--edition java`, where the read
    // is what fails. Without it, the first half would pass just as well on
    // a path that exists.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("missing.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            TAGGED_STAGE,
            path.to_str().unwrap(),
        ],
    );
    assert_missing_edition_usage_error(&out, TAGGED_STAGE, "on a missing file");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("cannot read"),
        "a missing file without --edition was read before the flag was refused, got: {stderr}",
    );

    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            TAGGED_STAGE,
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "--edition java on a missing file: {stderr}",
    );
    assert!(
        stderr.contains("cannot read"),
        "--edition java on a missing file should report the read, got: {stderr}",
    );
}

#[test]
fn a_stray_edition_on_an_edition_neutral_stage_is_a_usage_error() {
    // The refusing half of `--edition`'s contract; why the flag is refused
    // rather than ignored is `synth_edition`'s to say. The refusal has to
    // be the one stderr line whatever the file holds, so it is also run
    // over `FINDING_SOURCES`; the CONTROL runs each source on that stage
    // without `--edition`, where its finding is what surfaces.
    let path = examples_dir().join("redstone-door.crn");
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sources: Vec<(&str, PathBuf, Option<&str>)> = vec![("redstone-door.crn", path, None)];
    sources.extend(
        write_finding_sources(dir.path())
            .into_iter()
            .map(|(name, path, code)| (name, path, Some(code))),
    );
    for (name, source, code) in &sources {
        for edition in ["java", "bedrock"] {
            let out = cairn(
                "synth",
                &[
                    "--experimental-logic-synth",
                    "--stage",
                    NEUTRAL_STAGE,
                    "--edition",
                    edition,
                    source.to_str().unwrap(),
                ],
            );
            assert_stray_edition_usage_error(&out, NEUTRAL_STAGE, edition, &format!("on {name}"));
        }
        if let Some(code) = code {
            let out = cairn(
                "synth",
                &[
                    "--experimental-logic-synth",
                    "--stage",
                    NEUTRAL_STAGE,
                    source.to_str().unwrap(),
                ],
            );
            assert_reports_finding(&out, code, &format!("--stage {NEUTRAL_STAGE} on {name}"));
        }
    }
}

/// Assert that `out` is the missing-`--edition` usage error for
/// `--stage <stage>` and nothing else: exit 2, no stdout (no partial IR
/// dump escaped), and one stderr line, naming `--stage <stage>` and
/// `--edition`. `context` says which run it was, after the stage.
fn assert_missing_edition_usage_error(out: &Output, stage: &str, context: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "--stage {stage} {context} without --edition should exit 2, got stderr: {stderr}",
    );
    assert!(
        out.stdout.is_empty(),
        "--stage {stage} {context} must print no IR before the usage gate, got: {}",
        String::from_utf8_lossy(&out.stdout),
    );
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "--stage {stage} {context}: stderr should be the usage error alone, got: {stderr}",
    );
    assert!(
        lines[0].contains(&format!("--stage {stage}")),
        "--stage {stage} {context}: the usage error should name the stage as typed, got: {stderr}",
    );
    assert!(
        lines[0].contains("--edition"),
        "--stage {stage} {context}: the usage error should name --edition, got: {stderr}",
    );
}

/// Sources that each carry a finding, for the tests that pin a usage
/// refusal ahead of whatever the file has to say: a file name, the text,
/// and the code of the finding the text carries.
///
/// The errors come from the three passes `run_synth` reports from before
/// it dispatches the stage: a parse error, a `check` error in a file that
/// parses (`bogus` is no keyword), and a synthesis error in a file `check`
/// passes (nothing in the scope emits `sig.nope`). The last source carries
/// a warning alone, which does not stop a run.
const FINDING_SOURCES: [(&str, &str, &str); 4] = [
    (
        "parse.crn",
        "@cairn 2026.06\nstruct s size=3x3 size=\n",
        "E_PARSE",
    ),
    (
        "check.crn",
        "@cairn 2026.06\nstruct s size=3x3\n  bogus a=1\n",
        "E_UNKNOWN_KEYWORD",
    ),
    (
        "synth.crn",
        "@cairn 2026.06\nstruct s size=3x3\n  \
         door id=d side=front at=center opened_by=sig.nope\n",
        "E_LOGIC_UNBOUND_SIGNAL",
    ),
    (
        "warning.crn",
        "@cairn 2026.06\nstruct s size=3x3\n  \
         pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "W_LOGIC_UNUSED_SIGNAL",
    ),
];

/// Write each of [`FINDING_SOURCES`] into `dir`, as its name, the path it
/// was written to, and its code.
fn write_finding_sources(dir: &Path) -> Vec<(&'static str, PathBuf, &'static str)> {
    FINDING_SOURCES
        .into_iter()
        .map(|(name, text, code)| (name, write_source(dir, name, text), code))
        .collect()
}

/// Assert that `out` reported the finding `code` with the exit code its
/// severity gives: `error[E_…]` and exit 1, or `warning[W_…]` and exit 0,
/// since a warning leaves the run its product.
fn assert_reports_finding(out: &Output, code: &str, context: &str) {
    let (severity, exit) = match code.split_once('_') {
        Some(("E", _)) => ("error", 1),
        Some(("W", _)) => ("warning", 0),
        _ => panic!("`{code}` is neither an E_ nor a W_ code"),
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(exit),
        "{context}: a source carrying {code} should exit {exit}, got stderr: {stderr}",
    );
    assert!(
        stderr.contains(&format!("{severity}[{code}]")),
        "{context}: stderr should carry {severity}[{code}], got: {stderr}",
    );
}

/// Assert that `out` is the stray-`--edition` usage error for
/// `--stage <stage> --edition <edition>` and nothing else: exit 2, no
/// stdout, and one stderr line, naming the flag and `<stage>` among the
/// edition-neutral stages. `context` says which run it was, after the
/// flags.
fn assert_stray_edition_usage_error(out: &Output, stage: &str, edition: &str, context: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "edition-neutral --stage {stage} should refuse --edition {edition} {context}, \
         got stderr: {stderr}",
    );
    assert!(
        out.stdout.is_empty(),
        "--stage {stage} --edition {edition} {context} must print no IR, got: {}",
        String::from_utf8_lossy(&out.stdout),
    );
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "--stage {stage} --edition {edition} {context}: stderr should be the usage error \
         alone, got: {stderr}",
    );
    assert!(
        lines[0].contains("`--edition`") && lines[0].contains("edition-neutral"),
        "--stage {stage} --edition {edition} {context} should be refused as a stray flag, \
         got: {stderr}",
    );
    // The message lists the edition-neutral stages by bare name; the stage
    // just refused has to be among them, or the caller is told a set the
    // gate does not enforce.
    assert!(
        lines[0].contains(&format!("`{stage}`")),
        "--stage {stage} --edition {edition} {context} should name `{stage}` among the \
         edition-neutral stages, got: {stderr}",
    );
}

/// One `--stage` dump computed in-process: the stage, whether it is run
/// for an edition, the IR as `synth` prints it, and the `(severity, code)`
/// of every finding `synth` reports on the way to it.
struct Dump {
    stage: &'static str,
    tagged: bool,
    ir: Value,
    findings: Vec<(String, String)>,
}

/// Every stage's dump of `ir` for `edition`, running the redstone passes
/// in the order `synth` chains them.
fn dumps(ir: &IntentModule, edition: Edition) -> Vec<Dump> {
    fn findings(so_far: &mut Vec<(String, String)>, found: &[Diagnostic]) -> Vec<(String, String)> {
        so_far.extend(
            found
                .iter()
                .map(|d| (d.severity().as_str().to_owned(), d.code.as_str().to_owned())),
        );
        so_far.clone()
    }
    fn json(ir: &impl serde::Serialize) -> Value {
        serde_json::to_value(ir).expect("the IR serialises")
    }
    let mut so_far = Vec::new();
    let synth = synthesize(ir);
    let logic = findings(&mut so_far, &synth.diagnostics);
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    let placement = compile_placement(&edition_netlist, ir);
    let placed = findings(&mut so_far, &placement.diagnostics);
    let routing = compile_routing(&placement.scoped);
    let routed = findings(&mut so_far, &routing.diagnostics);
    let delay = compile_delay(&routing.scoped);
    let delayed = findings(&mut so_far, &delay.diagnostics);
    let crossing = compile_crossing(&delay.scoped);
    let crossed = findings(&mut so_far, &crossing.diagnostics);
    vec![
        Dump {
            stage: "logic",
            tagged: false,
            ir: json(&synth.scoped),
            findings: logic.clone(),
        },
        Dump {
            stage: "netlist",
            tagged: false,
            ir: json(&netlist),
            findings: logic.clone(),
        },
        Dump {
            stage: "edition",
            tagged: true,
            ir: json(&edition_netlist),
            findings: logic,
        },
        Dump {
            stage: "placement",
            tagged: true,
            ir: json(&placement.scoped),
            findings: placed,
        },
        Dump {
            stage: "route",
            tagged: true,
            ir: json(&routing.scoped),
            findings: routed,
        },
        Dump {
            stage: "delay",
            tagged: true,
            ir: json(&delay.scoped),
            findings: delayed,
        },
        Dump {
            stage: "crossing",
            tagged: true,
            ir: json(&crossing.scoped),
            findings: crossed,
        },
    ]
}

/// What `synth` prints for each stage is the redstone crate's own IR for
/// that stage, serialised, with the findings reported on the way to it on
/// stderr and exit 0. The values themselves are the redstone crate's to
/// pin; what is pinned here is that the command runs the passes in order,
/// stops at the stage asked for, prints that stage and not a neighbour,
/// and neither drops a finding nor promotes a warning to a refusal.
#[test]
fn every_stage_prints_the_ir_the_redstone_passes_build() {
    for example in ["redstone-door.crn", "crossbar.crn"] {
        let path = examples_dir().join(example);
        let source = fs::read_to_string(&path).expect("read example");
        let ir = lower(&parse(&source).expect("the example parses"));
        for edition in [Edition::Java, Edition::Bedrock] {
            let edition_flag = edition.as_str();
            for dump in dumps(&ir, edition) {
                // An edition-neutral stage dumps the same IR for either
                // edition, and refuses `--edition`; it is run once.
                if !dump.tagged && edition != Edition::Java {
                    continue;
                }
                let mut args = vec!["--experimental-logic-synth", "--stage", dump.stage];
                if dump.tagged {
                    args.extend(["--edition", edition_flag]);
                }
                args.push(path.to_str().unwrap());
                let out = cairn("synth", &args);
                let context = format!("{example} {args:?}");
                let stderr = String::from_utf8(out.stderr).expect("utf-8");
                assert_eq!(out.status.code(), Some(0), "{context}: {stderr}");
                let reported: Vec<(String, String)> = stderr_stream(&stderr)
                    .into_iter()
                    .map(|(_, _, severity, code)| (severity, code))
                    .collect();
                assert_eq!(reported, dump.findings, "{context}: {stderr}");
                if dump.findings.is_empty() {
                    assert!(stderr.is_empty(), "{context}: nothing to report: {stderr}");
                }
                let stdout = String::from_utf8(out.stdout).expect("utf-8");
                let printed: Value = serde_json::from_str(&stdout)
                    .unwrap_or_else(|err| panic!("{context}: stdout is not JSON: {err}\n{stdout}"));
                assert_eq!(printed, dump.ir, "{context}");
            }
        }
    }
}

#[test]
fn cli_synth_stage_placement_missing_region_exits_one() {
    // A scope with cells but no `circuit region=` line surfaces
    // `E_NO_CIRCUIT_REGION` on stderr and exits 1 — the symmetric E2E
    // gate to the congestion case below, so both fail-loud paths are
    // wired through the CLI in the same shape.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("noregion.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct noregion size=7x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.open = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.open\n";
    std::fs::write(&path, source).expect("write missing-region fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "placement",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_NO_CIRCUIT_REGION"),
        "expected E_NO_CIRCUIT_REGION on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_placement_congestion_exits_one() {
    // A scope whose synthesised netlist overflows its `circuit
    // region=... void=N` reservation should fail loud with
    // `E_ROUTE_CONGESTION` on stderr and exit 1 — the same convention
    // the synth pass's earlier fail-loud diagnostics follow.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("tiny.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct tiny size=3x3\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.and_ab   = sig.a and sig.b\n  \
        logic sig.or_ab    = sig.a or sig.b\n  \
        logic sig.combined = sig.and_ab and sig.or_ab\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.combined\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "placement",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_route_congestion_exits_one() {
    // A scope that passes placement at the cell-only budget boundary
    // but overflows once routing lays wires must fail loud with
    // `E_ROUTE_CONGESTION` on stderr and exit 1 — the same convention
    // the earlier stages follow.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("pack.crn");
    // Two cells in the shortest row that holds them, in the shallowest
    // region the row fits in: seven coords are left over once the cells
    // are counted, and the wire spends more than seven. One sensor
    // rather than two so the chain routes — a stranded sink would
    // refuse this a stage earlier, for a different reason.
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct pack size=5x3\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        logic sig.c0 = not sig.a\n  \
        logic sig.c1 = sig.c0 and sig.a\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.c1\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "route",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION on stderr, got: {stderr}",
    );
    assert!(
        stderr.contains("routed netlist for struct `pack`"),
        "primary should name the routing origin and failed scope, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_delay_inherits_upstream_congestion_failure() {
    // A scope that fails at the routing stage (E_ROUTE_CONGESTION) is
    // elided from routing's output. The delay stage runs on the elided
    // set, so its own diagnostics list is empty; but the routing
    // failure was already reported and the process exits 1. Pins the
    // "delay stage inherits upstream fail-loud" contract so an author
    // running `--stage delay` sees the same congestion errors they
    // would have seen from `--stage route`.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("pack.crn");
    // Two cells in the shortest row that holds them, in the shallowest
    // region the row fits in: seven coords are left over once the cells
    // are counted, and the wire spends more than seven. One sensor
    // rather than two so the chain routes — a stranded sink would
    // refuse this a stage earlier, for a different reason.
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct pack size=5x3\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        logic sig.c0 = not sig.a\n  \
        logic sig.c1 = sig.c0 and sig.a\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.c1\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "delay",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION inherited from routing on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_delay_attenuation_limit_exits_one() {
    // A scope whose output-pad segment exceeds the v1 attenuation cap
    // must fail loud with `E_ATTENUATION_LIMIT` on stderr and exit 1.
    // Uses a 300-block-wide region so the `sig.out` output driver spans
    // the full x-axis to the right-edge output pad — that segment
    // (~300 blocks) sits well past the 256-block cap.
    //
    // `--stage delay` is asked for, but the refusal now comes from the
    // routing pass: the straight line alone is already over the cap, so
    // stage 2's gate answers first and the CLI stops there. Hence the
    // `placed` noun below — the cells are still `Unrouted` when it
    // fires. The delay pass's own routed-length check is exercised by
    // the detour fixtures in `cairn-lang-redstone`, where the straight
    // line fits and only the route does not.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("wide.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct wide_pack size=300x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    std::fs::write(&path, source).expect("write attenuation fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "delay",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ATTENUATION_LIMIT"),
        "expected E_ATTENUATION_LIMIT on stderr, got: {stderr}",
    );
    assert!(
        stderr.contains("placed netlist for struct `wide_pack`"),
        "primary should name the stage that refused and the failed scope, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_route_refuses_a_detour_past_the_attenuation_cap() {
    // `sig.s1` feeds the gate from its pad at `(0,0,2)` through `(1,0,2)`,
    // so `sig.s2`'s wire, from its pad at `(0,0,3)`, cannot start along
    // its own row: `(1,0,3)` is one step from that dust. It steps out a
    // row, runs the length of the region and steps back to the far
    // door's pad at `(width - 1, 0, 2)`: its straight line is `width`
    // and its route `width + 2`. At `width = cap - 2` the route is the
    // cap and the scope routes; one wider it is one block over, and the
    // router's search, bounded by the cap, refuses it at stage 2 — still
    // within the cap in a straight line, so the straight-line gate lets
    // it through to the router.
    use cairn_lang_redstone::MAX_ATTENUATION_SEGMENT as CAP;
    let dir = tempfile::tempdir().expect("temp dir");
    for (width, refuses) in [(CAP - 2, false), (CAP - 1, true)] {
        let path = dir.path().join(format!("detour{width}.crn"));
        let source = format!(
            "@cairn 2026.06\n@requires version>=1.20\n\n\
             theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n\
             struct s size={width}x6\n  \
             floor mat_slot=wall\n  \
             door id=d0 side=front at=center mat_slot=door\n  \
             door id=d1 side=back at=center mat_slot=door\n  \
             pressure_plate id=p0 at=front.outside offset=0 y=0 -> sig.s0\n  \
             pressure_plate id=p1 at=back.outside offset=0 y=0 -> sig.s1\n  \
             pressure_plate id=p2 at=front.outside offset=1 y=0 -> sig.s2\n  \
             logic sig.g0 = sig.s0 and sig.s1\n  \
             door[id=d0] opened_by=sig.g0\n  \
             door[id=d1] opened_by=sig.s2\n  \
             circuit region=floor void=1\n"
        );
        std::fs::write(&path, source).expect("write detour fixture");
        let out = cairn(
            "synth",
            &[
                "--experimental-logic-synth",
                "--stage",
                "route",
                "--edition",
                "java",
                path.to_str().unwrap(),
            ],
        );
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        if !refuses {
            assert_eq!(out.status.code(), Some(0), "width {width}: {stderr}");
            let stdout = String::from_utf8(out.stdout).expect("utf-8");
            let value: serde_json::Value = serde_json::from_str(&stdout)
                .unwrap_or_else(|err| panic!("stdout should parse as JSON: {err}\n{stdout}"));
            let lengths: Vec<u64> = value[0]["ir"]["outputs"]
                .as_array()
                .expect("outputs array")
                .iter()
                .map(|output| output["wire_length"].as_u64().expect("routed"))
                .collect();
            assert_eq!(
                lengths,
                [u64::from(CAP - 3), u64::from(CAP)],
                "the far door's wire is exactly the cap",
            );
            continue;
        }
        assert_eq!(out.status.code(), Some(1), "width {width}: {stderr}");
        let expected = format!(
            "error[E_ATTENUATION_LIMIT]: placed netlist for struct `s` has no route from the \
             driver at (0,0,3) to ({},0,2) within the v1 attenuation limit of {CAP} steps; the \
             faces it could arrive through are taken by cell #0\n  note: Fix: give the wire a \
             shorter way round",
            width - 1,
        );
        assert!(stderr.contains(&expected), "width {width}: got {stderr}");
    }
}

#[test]
fn cli_synth_unparseable_source_exits_one() {
    // Parse-level failure follows the same exit-code convention as
    // `cairn parse` / `check`: exit 1 (build problem), position-anchored
    // error on stderr. Using a scratch file rather than a fixture so the
    // test does not add a permanent broken example under `tests/`.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("broken.crn");
    std::fs::write(&path, "@cairn 2026.06\n\nstruct 3xnot-a-size\n").expect("write scratch file");
    let out = cairn(
        "synth",
        &["--experimental-logic-synth", path.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("error"),
        "expected an error line on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_crossing_inherits_upstream_attenuation_failure() {
    // `--stage crossing` runs stages 1-4 in sequence, so an
    // Error-severity diagnostic from any prior stage (here, the
    // routing pass's `E_ATTENUATION_LIMIT` on a 300-block-wide region)
    // short-circuits with exit 1 before stage 4 runs. The stderr
    // still names the origin stage so a downstream reader can tell
    // which pass tripped.
    //
    // The origin used to be the delay pass; stage 2 now answers first
    // for a segment whose straight line alone is over the cap, and
    // every `.crn` that reaches this code does so by that route. A
    // scope that passes the straight-line gate and fails only the
    // routed-length one can no longer be written in `.crn` at all, so
    // "crossing inherits a *delay* failure" is exercised by a
    // hand-built two-scope fixture in `cairn-lang-redstone`'s
    // `tests/delay.rs` rather than here.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("wide.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct wide_pack size=300x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    std::fs::write(&path, source).expect("write attenuation fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "crossing",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ATTENUATION_LIMIT"),
        "expected upstream E_ATTENUATION_LIMIT on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_crossing_two_nets_over_one_coord_exits_one() {
    // Two nets that want one coord, with no layer above the plane to
    // escape onto. The compiler refuses the scope rather than emitting
    // a layout in which those two signals are one strand of dust, and
    // the refusal has to reach the caller through `--stage crossing`
    // the way every earlier stage's does: exit 1, code on stderr,
    // nothing on stdout. Without this case a dropped diagnostic report
    // between the pipeline and the JSON dump would let a refused scope
    // be printed as a legalized IR.
    //
    // One cell, two sensors, two actuators, one service layer. `sig.a`
    // drives both the cell and the back door, so it is laid first, and
    // its run out to that door takes the coords beside the front door's
    // pad; the cell's own output has none left to arrive through and no
    // layer to climb onto.
    let source = "\
theme cross:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct crossbar size=4x4
  floor mat_slot=wall
  door  id=front side=front at=center mat_slot=door
  door  id=back  side=back  at=center mat_slot=door

  pressure_plate id=plate1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=plate2 at=inside.front  offset=1 y=0 -> sig.b

  logic sig.f = sig.a and sig.b

  door[id=front] opened_by=sig.f
  door[id=back]  opened_by=sig.a

  circuit region=floor void=1
";
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("crossbar.crn");
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "crossing",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION on stderr, got: {stderr}",
    );
    // `placed`, not `routed`: `--stage crossing` runs the whole pipeline
    // and the CLI stops at the first Error-severity stage, so this scope
    // never reaches the crossing pass — the routing pass refuses it while
    // laying nets over a netlist whose cells are still unrouted. The noun
    // names the netlist the refusing pass read, which is the only way to
    // tell from the message where in the pipeline the scope died.
    assert!(
        stderr.contains("placed netlist for struct `crossbar`")
            && stderr.contains("another net's dust, on the coord or one step from it")
            && stderr.contains("the faces it could arrive through are taken by sig.a"),
        "the refusal names the scope, which of the three kinds of obstacle it \
         means and how far it reaches, and the net standing in the way, got: \
         {stderr}",
    );
    // Which other code leaked would say the fixture drifted into an
    // over-long segment rather than into the congestion this is about.
    assert!(
        !stderr.contains("E_ATTENUATION_LIMIT"),
        "the fixture must refuse for want of room, not for segment length, \
         got: {stderr}",
    );
    assert!(
        out.stdout.is_empty(),
        "a refused scope must not reach stdout as a legalized IR dump, got: {}",
        String::from_utf8_lossy(&out.stdout),
    );
}
