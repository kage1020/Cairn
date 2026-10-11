//! Deeply nested sources are refused with a finding rather than
//! overflowing the native stack, which kills the process and so can only
//! be observed from outside it: each run is a subprocess, and its exit
//! code is asserted to be `Some(1)`, which an overflow never yields
//! (`Some(-1073741571)`, `STATUS_STACK_OVERFLOW`, on Windows; `None`, a
//! signal death, on Unix).

use std::fmt::Write as _;

use cairn_lang_core::{MAX_EXPR_DEPTH, MAX_NESTING_DEPTH};
use cairn_lang_redstone::synth::MAX_LOWERING_DEPTH;
use tempfile::TempDir;

use crate::common::{assert_stream, cairn_argv, write_source};

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

/// Every shape, past the depth an unguarded debug build overflows at:
/// roughly 287 levels of `[[[…`, 380 of parentheses, 785 of `not` and 400
/// of indented blocks, and about 570 terms of a flat `or` chain, which
/// parses but overflows `cairn parse`'s serialiser. Each entry sits past
/// its own shape's figure.
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
\x20\x20pressure_plate id=p2 at=inside.front  offset=1 y=0 -> sig.b\n";

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
fn every_command_refuses_deep_nesting_instead_of_aborting() {
    let tmp = TempDir::new().expect("tempdir");
    for (shape, source) in over_limit_sources() {
        let path = write_source(tmp.path(), &format!("{shape}.crn"), &source);
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
            let out = cairn_argv(&args);
            let code = out.status.code();
            assert_eq!(
                code,
                Some(1),
                "{shape}: `{}` should refuse with a diagnostic; got {code:?}\nstderr={}",
                args[0],
                String::from_utf8_lossy(&out.stderr),
            );
        }
    }
}

#[test]
fn the_refusal_says_what_the_limit_is() {
    // Both renderings: `parse` defaults to `--format json`, where a refusal
    // is the failure document on stdout.
    let tmp = TempDir::new().expect("tempdir");
    for (shape, source) in over_limit_sources() {
        let path = write_source(tmp.path(), &format!("{shape}.crn"), &source);
        let bound = if shape == "flat-or" {
            MAX_EXPR_DEPTH
        } else {
            MAX_NESTING_DEPTH
        };

        let out = cairn_argv(&["parse", path.to_str().unwrap(), "--format", "debug"]);
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(
            stderr.contains("nesting is limited to")
                && stderr.contains(&bound.to_string())
                && stderr.contains(&format!("{}:", path.display())),
            "{shape}: the message must name the bound it hit and the position so the \
             author can act on it; got {stderr}",
        );

        let out = cairn_argv(&["parse", path.to_str().unwrap()]);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|err| panic!("{shape}: stdout should be JSON, got {stdout:?}: {err}"));
        let d = &parsed["diagnostics"][0];
        assert_eq!(d["code"], "E_PARSE", "{shape}: {stdout}");
        assert!(
            d["primary"].as_str().is_some_and(
                |p| p.contains("nesting is limited to") && p.contains(&bound.to_string())
            ),
            "{shape}: the document must name the bound too; got {stdout}",
        );
        assert!(
            d["line"].as_u64().is_some_and(|line| line >= 1),
            "{shape}: and the position; got {stdout}",
        );
    }
}

#[test]
fn nesting_at_the_limit_still_parses() {
    let tmp = TempDir::new().expect("tempdir");
    for (shape, source) in at_limit_sources() {
        let path = write_source(tmp.path(), &format!("ok-{shape}.crn"), &source);
        let out = cairn_argv(&["parse", path.to_str().unwrap()]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{shape}: sitting on the bound is within it; stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn a_reverse_declared_logic_chain_is_refused_not_aborted() {
    // `synth` descends into whatever a binding references, so a chain
    // declared against dependency order nests two frames per binding, while
    // the same graph in dependency order stays shallow.
    let tmp = TempDir::new().expect("tempdir");
    let stages = 600;
    let mut source = String::from(CHAIN_HEADER);
    let _ = writeln!(source, "  door[id=front] opened_by=sig.s{stages}");
    source.push_str(&reverse_chain("s", stages));
    source.push_str("  circuit region=floor void=2\n");

    let path = write_source(tmp.path(), "reverse_chain.crn", &source);
    let out = cairn_argv(&[
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
    assert_stream(
        &stderr,
        &[
            (10, 48, "warning", "W_IGNORED_ARGUMENT"),
            (14, 3, "error", "E_LOGIC_NESTING_TOO_DEEP"),
            (143, 3, "error", "E_LOGIC_NESTING_TOO_DEEP"),
            (272, 3, "error", "E_LOGIC_NESTING_TOO_DEEP"),
            (401, 3, "error", "E_LOGIC_NESTING_TOO_DEEP"),
        ],
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
fn a_forward_declared_chain_of_the_same_size_still_lowers() {
    // The limit is on nesting depth, not on how many bindings a scope has.
    let tmp = TempDir::new().expect("tempdir");
    let stages = 600;
    let mut source = String::from(CHAIN_HEADER);
    source.push_str(&forward_chain("s", stages));
    let _ = writeln!(source, "  door[id=front] opened_by=sig.s{stages}");
    source.push_str("  circuit region=floor void=2\n");

    let path = write_source(tmp.path(), "forward_chain.crn", &source);
    let out = cairn_argv(&[
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
fn each_independent_chain_reports_its_own_root_cause() {
    // Repeats within one chain are suppressed, a second chain is not: every
    // root cause on one pass, as `lower_binary` states.
    let tmp = TempDir::new().expect("tempdir");
    // Just over the bound, so each chain holds one over-budget segment. A
    // longer chain reports once per segment, since the walk resumes at the
    // next binding it has not lowered.
    let stages = 200;
    let mut source = String::from(CHAIN_HEADER);
    let _ = writeln!(source, "  door[id=front] opened_by=sig.p{stages}");
    source.push_str(&reverse_chain("p", stages));
    source.push_str(&reverse_chain("q", stages));
    let _ = writeln!(source, "  door[id=back] opened_by=sig.q{stages}");
    source.push_str("  circuit region=floor void=2\n");

    let path = write_source(tmp.path(), "two_chains.crn", &source);
    let out = cairn_argv(&[
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
fn a_chain_of_one_operator_bindings_reaches_the_limit_where_the_doc_says() {
    // `MAX_LOWERING_DEPTH`'s doc says a one-operator binding costs two
    // frames, so such a chain reaches the bound at half of it. Nothing else
    // pins that factor.
    let quoted = MAX_LOWERING_DEPTH / 2;
    let tmp = TempDir::new().expect("tempdir");

    for (stages, want_refusal) in [(quoted - 1, false), (quoted, true)] {
        let mut source = String::from(CHAIN_HEADER);
        let _ = writeln!(source, "  door[id=front] opened_by=sig.s{stages}");
        source.push_str(&reverse_chain("s", stages));
        source.push_str("  circuit region=floor void=2\n");

        let path = write_source(tmp.path(), &format!("chain-{stages}.crn"), &source);
        let out = cairn_argv(&[
            "synth",
            path.to_str().unwrap(),
            "--experimental-logic-synth",
        ]);
        // An exact code on both legs, so an overflow's `None` or
        // `Some(-1073741571)` cannot pass for a chain that lowered.
        let code = out.status.code();
        let want = if want_refusal { Some(1) } else { Some(0) };
        assert_eq!(
            code,
            want,
            "a reverse chain of {stages} bindings should {} — the doc puts the limit \
             at {quoted}; got {code:?}\nstderr={}",
            if want_refusal { "be refused" } else { "lower" },
            String::from_utf8_lossy(&out.stderr),
        );
    }
}
