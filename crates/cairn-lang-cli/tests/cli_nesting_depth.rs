//! Deeply nested sources must be refused, not abort the process.
//!
//! Every recursive descent here costs native stack, and a Rust stack
//! overflow cannot be caught: it kills the process. That is why these
//! assertions run the binary as a subprocess and look at the exit code — a
//! library-level test could not observe the failure it is guarding against
//! without taking the test runner down with it.
//!
//! The exit codes worth telling apart:
//!
//! | code | meaning |
//! | --- | --- |
//! | 1 | refused with a diagnostic — what we want |
//! | 101 | Rust panic |
//! | 127 | stack overflow (`0xC00000FD` on Windows) |

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

fn write(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, source).expect("write source");
    path
}

/// `mat=[[[ … x … ]]]`, nested `depth` deep.
fn nested_list(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  window mat={}x{}\n",
        "[".repeat(depth),
        "]".repeat(depth),
    )
}

/// `logic sig.out = ((( … sig.a … )))`, nested `depth` deep.
fn nested_parens(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a{}\n",
        "(".repeat(depth),
        ")".repeat(depth),
    )
}

/// `logic sig.out = not not … sig.a`, chained `depth` deep.
fn chained_not(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a\n",
        "not ".repeat(depth),
    )
}

/// Deep enough to have aborted before the limit existed. Measured on a
/// debug build, the tightest shape (`mat=[[[…`) overflowed at 287 levels.
const PAST_THE_LIMIT: usize = 400;

/// Every shape, at a depth that used to take the process down.
fn over_limit_sources() -> Vec<(&'static str, String)> {
    vec![
        ("list", nested_list(PAST_THE_LIMIT)),
        ("parens", nested_parens(PAST_THE_LIMIT)),
        ("not", chained_not(PAST_THE_LIMIT)),
    ]
}

#[test]
fn depth_1_every_command_refuses_deep_nesting_instead_of_aborting() {
    let tmp = TempDir::new().expect("tempdir");
    for (shape, source) in over_limit_sources() {
        let path = write(tmp.path(), &format!("{shape}.crn"), &source);
        let file = path.to_str().unwrap();
        let out_dir = tmp.path().join(format!("out-{shape}"));

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
            vec!["synth", file, "--experimental-logic-synth"],
        ] {
            let out = run(&args);
            let code = out.status.code();
            assert_eq!(
                code,
                Some(1),
                "{shape}: `{}` should refuse with a diagnostic; got {code:?} \
                 (101 = panic, 127 = stack overflow)\nstderr={}",
                args[0],
                String::from_utf8_lossy(&out.stderr),
            );
        }
    }
}

#[test]
fn depth_2_the_refusal_says_what_the_limit_is() {
    let tmp = TempDir::new().expect("tempdir");
    for (shape, source) in over_limit_sources() {
        let path = write(tmp.path(), &format!("{shape}.crn"), &source);
        let out = run(&["parse", path.to_str().unwrap()]);
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(
            stderr.contains("nesting")
                && stderr.contains("64")
                && stderr.contains(&format!("{}:", path.display())),
            "{shape}: the message must name the limit and the position so the \
             author can act on it; got {stderr}",
        );
    }
}

#[test]
fn depth_3_nesting_at_the_limit_still_parses() {
    // The limit is a wall, not a cliff edge: a source sitting exactly on it
    // is legal, so the boundary is pinned from both sides.
    let tmp = TempDir::new().expect("tempdir");
    for (shape, source) in [
        ("list", nested_list(64)),
        ("parens", nested_parens(64)),
        ("not", chained_not(64)),
    ] {
        let path = write(tmp.path(), &format!("ok-{shape}.crn"), &source);
        let out = run(&["parse", path.to_str().unwrap()]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{shape}: depth 64 is within the limit; stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn depth_4_a_reverse_declared_logic_chain_is_refused_not_aborted() {
    // `synth` lowers a binding by descending into whatever it references, so
    // a chain declared in the reverse of topological order recursed once per
    // binding. Around 410 lines took the process down with no diagnostic —
    // while the same graph written in dependency order lowered fine at
    // several thousand, so a semantically identical program crashed on
    // statement order alone.
    let tmp = TempDir::new().expect("tempdir");
    let stages = 600;
    let mut source = String::from(
        "@cairn 2026.06\n\n\
         theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n\
         struct chain size=8x6\n\
         \x20\x20floor mat_slot=wall\n\
         \x20\x20walls class=outer mat_slot=wall height=3\n\
         \x20\x20door  id=front side=front at=center mat_slot=door\n\
         \x20\x20pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n\
         \x20\x20pressure_plate id=p2 at=inside.front  offset=0 y=0 -> sig.b\n",
    );
    let _ = writeln!(source, "  door[id=front] opened_by=sig.s{stages}");
    for i in (1..=stages).rev() {
        let _ = writeln!(source, "  logic sig.s{i} = sig.s{} and sig.a", i - 1);
    }
    source.push_str("  logic sig.s0 = sig.a and sig.b\n");
    source.push_str("  circuit region=floor void=2\n");

    let path = write(tmp.path(), "reverse_chain.crn", &source);
    let out = run(&[
        "synth",
        path.to_str().unwrap(),
        "--experimental-logic-synth",
    ]);
    let code = out.status.code();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        code,
        Some(1),
        "a chain past the limit must be refused, not abort; got {code:?}\nstderr={stderr}",
    );
    assert!(
        stderr.contains("E_LOGIC_NESTING_TOO_DEEP"),
        "the refusal needs a code a tool can match on; got {stderr}",
    );
}

#[test]
fn depth_5_a_forward_declared_chain_of_the_same_size_still_lowers() {
    // The mirror of the case above: the limit must bite on nesting depth,
    // not on how many bindings a scope has.
    let tmp = TempDir::new().expect("tempdir");
    let stages = 600;
    let mut source = String::from(
        "@cairn 2026.06\n\n\
         theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n\
         struct chain size=8x6\n\
         \x20\x20floor mat_slot=wall\n\
         \x20\x20walls class=outer mat_slot=wall height=3\n\
         \x20\x20door  id=front side=front at=center mat_slot=door\n\
         \x20\x20pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n\
         \x20\x20pressure_plate id=p2 at=inside.front  offset=0 y=0 -> sig.b\n\
         \x20\x20logic sig.s0 = sig.a and sig.b\n",
    );
    for i in 1..=stages {
        let _ = writeln!(source, "  logic sig.s{i} = sig.s{} and sig.a", i - 1);
    }
    let _ = writeln!(source, "  door[id=front] opened_by=sig.s{stages}");
    source.push_str("  circuit region=floor void=2\n");

    let path = write(tmp.path(), "forward_chain.crn", &source);
    let out = run(&[
        "synth",
        path.to_str().unwrap(),
        "--experimental-logic-synth",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a dependency-ordered chain is not deeply nested; stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}
