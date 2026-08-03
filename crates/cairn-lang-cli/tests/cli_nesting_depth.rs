//! Deeply nested sources must be refused, not abort the process.
//!
//! Every recursive descent here costs native stack, and a Rust stack
//! overflow cannot be caught: it kills the process. That is why these
//! assertions run the binary as a subprocess and look at the exit status — a
//! library-level test could not observe the failure it is guarding against
//! without taking the test runner down with it.
//!
//! What an overflow looks like through `Output::status.code()`:
//!
//! | platform | value |
//! | --- | --- |
//! | Windows | `Some(-1073741571)` — `0xC00000FD`, `STATUS_STACK_OVERFLOW` |
//! | Unix | `None` — the child died on `SIGSEGV`/`SIGABRT`, so there is no code |
//!
//! (`127` is what a POSIX *shell* synthesises for a signal death; it never
//! reaches `std::process`. Asserting `Some(1)` rejects every one of these.)

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cairn_lang_core::{MAX_EXPR_DEPTH, MAX_NESTING_DEPTH};
use cairn_lang_redstone::synth::MAX_LOWERING_DEPTH;
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

fn nested_list(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  window mat={}x{}\n",
        "[".repeat(depth),
        "]".repeat(depth),
    )
}

fn nested_parens(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a{}\n",
        "(".repeat(depth),
        ")".repeat(depth),
    )
}

fn chained_not(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a\n",
        "not ".repeat(depth),
    )
}

fn nested_blocks(depth: usize) -> String {
    let mut source = String::from("struct a size=1x1\n");
    for level in 1..=depth {
        source.push_str(&"  ".repeat(level));
        source.push_str("level y=0\n");
    }
    source
}

fn flat_or_chain(terms: usize) -> String {
    let chain = std::iter::repeat_n("sig.a", terms)
        .collect::<Vec<_>>()
        .join(" or ");
    format!("struct a size=1x1\n  logic sig.out = {chain}\n")
}

/// Every shape, deep enough that the unguarded build died on it.
///
/// The depths differ because the shapes did: measured on a debug build the
/// parser overflowed at roughly 287 levels of `[[[…`, 380 of parentheses,
/// 785 of `not`, and 400 of indented blocks, while a flat `or` chain parsed
/// fine and took `cairn parse` down at about 570 terms when it serialised
/// the tree. Each entry sits past its own shape's measurement, not past the
/// smallest of them.
fn over_limit_sources() -> Vec<(&'static str, String)> {
    vec![
        ("list", nested_list(400)),
        ("parens", nested_parens(500)),
        ("not", chained_not(900)),
        ("blocks", nested_blocks(500)),
        ("flat-or", flat_or_chain(700)),
    ]
}

/// Every shape at exactly the bound that governs it.
fn at_limit_sources() -> Vec<(&'static str, String)> {
    vec![
        ("list", nested_list(MAX_NESTING_DEPTH)),
        ("parens", nested_parens(MAX_NESTING_DEPTH)),
        ("not", chained_not(MAX_NESTING_DEPTH)),
        ("blocks", nested_blocks(MAX_NESTING_DEPTH)),
        ("flat-or", flat_or_chain(MAX_EXPR_DEPTH)),
    ]
}

const CHAIN_HEADER: &str = "@cairn 2026.06\n\n\
theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n\
struct chain size=8x6\n\
\x20\x20floor mat_slot=wall\n\
\x20\x20walls class=outer mat_slot=wall height=3\n\
\x20\x20door  id=front side=front at=center mat_slot=door\n\
\x20\x20pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n\
\x20\x20pressure_plate id=p2 at=inside.front  offset=0 y=0 -> sig.b\n";

/// A `logic` chain of `stages` bindings under `prefix`, declared so that
/// each binding references the next one to be written.
fn reverse_chain(prefix: &str, stages: usize) -> String {
    let mut body = String::new();
    for i in (1..=stages).rev() {
        let _ = writeln!(
            body,
            "  logic sig.{prefix}{i} = sig.{prefix}{} and sig.a",
            i - 1
        );
    }
    let _ = writeln!(body, "  logic sig.{prefix}0 = sig.a and sig.b");
    body
}

/// The same chain with every binding declared after what it references.
fn forward_chain(prefix: &str, stages: usize) -> String {
    let mut body = String::new();
    let _ = writeln!(body, "  logic sig.{prefix}0 = sig.a and sig.b");
    for i in 1..=stages {
        let _ = writeln!(
            body,
            "  logic sig.{prefix}{i} = sig.{prefix}{} and sig.a",
            i - 1
        );
    }
    body
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
                 (see the table at the top of this file)\nstderr={}",
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
        let bound = if shape == "flat-or" {
            MAX_EXPR_DEPTH
        } else {
            MAX_NESTING_DEPTH
        };
        assert!(
            stderr.contains("nesting is limited to")
                && stderr.contains(&bound.to_string())
                && stderr.contains(&format!("{}:", path.display())),
            "{shape}: the message must name the bound it hit and the position so the \
             author can act on it; got {stderr}",
        );
    }
}

#[test]
fn depth_3_nesting_at_the_limit_still_parses() {
    // Each bound is a wall, not a cliff edge: a source sitting exactly on it
    // is legal, so both sides are pinned.
    let tmp = TempDir::new().expect("tempdir");
    for (shape, source) in at_limit_sources() {
        let path = write(tmp.path(), &format!("ok-{shape}.crn"), &source);
        let out = run(&["parse", path.to_str().unwrap()]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{shape}: sitting on the bound is within it; stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn depth_4_a_reverse_declared_logic_chain_is_refused_not_aborted() {
    // `synth` lowers a binding by descending into whatever it references, so
    // a chain declared in the reverse of dependency order recursed twice per
    // binding. Around 410 lines took the process down with no diagnostic —
    // while the same graph in dependency order lowered fine at several
    // thousand, so a semantically identical program crashed on statement
    // order alone.
    let tmp = TempDir::new().expect("tempdir");
    let stages = 600;
    let mut source = String::from(CHAIN_HEADER);
    let _ = writeln!(source, "  door[id=front] opened_by=sig.s{stages}");
    source.push_str(&reverse_chain("s", stages));
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
    assert!(
        stderr.contains("chained bindings"),
        "the message must speak in bindings, the unit the author wrote in — a raw \
         frame count does not match anything they can see; got {stderr}",
    );
    assert!(
        stderr.contains("declare each `logic` binding after the ones it references"),
        "the fix must be the one that applies to a chain; got {stderr}",
    );
}

#[test]
fn depth_5_a_forward_declared_chain_of_the_same_size_still_lowers() {
    // The mirror of the case above: the limit must bite on nesting depth,
    // not on how many bindings a scope has.
    let tmp = TempDir::new().expect("tempdir");
    let stages = 600;
    let mut source = String::from(CHAIN_HEADER);
    source.push_str(&forward_chain("s", stages));
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

#[test]
fn depth_6_each_independent_chain_reports_its_own_root_cause() {
    // Suppressing repeats within one chain is right; suppressing the second
    // chain is not. `lower_binary` states the policy this restores: keep
    // every root cause on one pass so the author does not fix, re-run, and
    // discover the next one.
    let tmp = TempDir::new().expect("tempdir");
    // Just over the bound, so each chain holds exactly one over-budget
    // segment and the expected count is unambiguous. A much longer chain
    // legitimately reports more than once — after the first refusal the
    // walk resumes at the next binding it has not lowered, which is the
    // start of another segment that also cannot fit.
    let stages = 200;
    let mut source = String::from(CHAIN_HEADER);
    let _ = writeln!(source, "  door[id=front] opened_by=sig.p{stages}");
    source.push_str(&reverse_chain("p", stages));
    source.push_str(&reverse_chain("q", stages));
    let _ = writeln!(source, "  door[id=back] opened_by=sig.q{stages}");
    source.push_str("  circuit region=floor void=2\n");

    let path = write(tmp.path(), "two_chains.crn", &source);
    let out = run(&[
        "synth",
        path.to_str().unwrap(),
        "--experimental-logic-synth",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let reported = stderr.matches("E_LOGIC_NESTING_TOO_DEEP").count();
    assert_eq!(
        reported, 2,
        "two independent chains are two root causes; got {reported}\nstderr={stderr}",
    );
}

#[test]
fn depth_7_the_chain_length_the_message_quotes_is_the_real_one() {
    // The message translates the internal frame budget into bindings, which
    // is the unit the author counts in. That translation is a factor of two
    // — an operand descent plus the referenced binding's own expression —
    // and nothing else pins it, so a change to the recursion shape would
    // leave the message quoting a number that matches nothing on screen.
    let quoted = MAX_LOWERING_DEPTH / 2;
    let tmp = TempDir::new().expect("tempdir");

    for (stages, want_refusal) in [(quoted - 1, false), (quoted, true)] {
        let mut source = String::from(CHAIN_HEADER);
        let _ = writeln!(source, "  door[id=front] opened_by=sig.s{stages}");
        source.push_str(&reverse_chain("s", stages));
        source.push_str("  circuit region=floor void=2\n");

        let path = write(tmp.path(), &format!("chain-{stages}.crn"), &source);
        let out = run(&[
            "synth",
            path.to_str().unwrap(),
            "--experimental-logic-synth",
        ]);
        let refused = out.status.code() == Some(1);
        assert_eq!(
            refused,
            want_refusal,
            "a reverse chain of {stages} bindings should {} — the message promises \
             about {quoted}; stderr={}",
            if want_refusal { "be refused" } else { "lower" },
            String::from_utf8_lossy(&out.stderr),
        );
    }
}
