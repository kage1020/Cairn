//! Parity between `cairn check` and the commands that build on it.
//!
//! `check` is the gate the other subcommands sit behind: a source it rejects
//! with an `Error`-severity diagnostic must not lower, must not report
//! portability figures, and must not compile. Before this suite existed,
//! `check` ran only from `cairn check` and `cairn synth`, so `cairn compile`
//! wrote artifacts — and a `verified: true` lockfile — for a file `cairn
//! check` had just rejected with exit 1.
//!
//! Parity here means the whole diagnostic stream, not just the exit code.
//! The build commands report `check`'s findings verbatim and in order, then
//! append their own lowering diagnostics; anything else (a dropped code, a
//! duplicated one, a reordered one) is a regression these tests catch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// Shared prologue so each fixture differs only in the construct that trips
/// its diagnostic. Named for its role rather than for the `@directive`
/// lines it happens to open with — `Source::Whole` exists because one
/// fixture is about those, and calling both "header" made the two read
/// as the same thing.
const PROLOGUE: &str = "@cairn 2026.06\n\
@requires version>=1.20\n\
\n\
theme t:\n\
\x20\x20slot floor -> @oak_planks\n\
\x20\x20slot wall  -> @cobblestone\n\
\n";

/// A fixture's source.
///
/// Most fixtures differ only in the construct that trips their diagnostic,
/// so they are written as a body appended to [`PROLOGUE`]. Two cannot be.
/// A header diagnostic repeats a directive that is legal only before the
/// first item; and a selector fixture has to add rows to the file's *only*
/// theme, because a second `theme` block leaves no theme auto-bound and the
/// rows then reach no member. Both own their whole source.
enum Source {
    WithPrologue(&'static str),
    Whole(&'static str),
}

impl Source {
    fn text(&self) -> String {
        match self {
            Self::WithPrologue(body) => format!("{PROLOGUE}{body}"),
            Self::Whole(text) => (*text).to_owned(),
        }
    }
}

/// One fixture per `Error`-severity code the syntactic passes inside
/// `check` emit.
///
/// `cairn_lang_core`'s `syntactic_error_codes_are_covered_by_cli_fixtures`
/// pins that set by iterating the `DiagnosticCode` enum, and names this file
/// when the two disagree. Adding a variant there forces a classification;
/// classifying it as a syntactic `Error` then fails that assertion until a
/// fixture lands here.
const SYNTACTIC_FIXTURES: &[(&str, Source)] = &[
    (
        "E_DUPLICATE_SIZE",
        Source::WithPrologue("struct s size=5x5 size=6x6\n  floor mat_slot=floor\n"),
    ),
    (
        "E_DUPLICATE_SLOT",
        // Declares its own theme: the shared header binds each slot once.
        Source::WithPrologue(
            "theme dup:\n  slot wall -> @cobblestone\n  slot wall -> @stone_bricks\n\
             \nstruct s size=5x5\n  walls mat_slot=wall height=3\n",
        ),
    ),
    (
        // Two rows with one keyword and one attribute set select the same
        // members, so the later `frame=` is the only one anything reads.
        // Written whole so the rows join the theme the struct binds.
        "E_DUPLICATE_SELECTOR",
        Source::Whole(
            "@cairn 2026.06\n@requires version>=1.20\n\n\
             theme t:\n  slot wall -> @cobblestone\n  \
             window[class=small] -> frame=@spruce_wood\n  \
             window[class=small] -> frame=@dark_oak_wood\n\n\
             struct s size=5x5\n  \
             window class=small side=front offset=1 y=1 size=1x1 mat_slot=wall\n",
        ),
    ),
    (
        "E_DUPLICATE_ARG",
        Source::WithPrologue("struct s size=5x5\n  floor id=a id=b mat_slot=floor\n"),
    ),
    (
        "E_DUPLICATE_ID",
        Source::WithPrologue(
            "struct s size=5x5\n  floor id=x mat_slot=floor\n\
             \x20\x20walls id=x class=outer mat_slot=wall height=3\n",
        ),
    ),
    (
        // The prologue already binds `theme t`, so the repeat is the only
        // construct this body adds.
        "E_DUPLICATE_ITEM",
        Source::WithPrologue(
            "theme t:\n  slot floor -> @stone\n\
             \nstruct s size=5x5\n  floor mat_slot=floor\n",
        ),
    ),
    (
        // Headers are legal only before the first item, so this fixture
        // cannot be a body appended to the shared prologue.
        "E_DUPLICATE_HEADER",
        Source::Whole(
            "@cairn 2026.06\n@cairn 2026.07\n\
             \ntheme t:\n  slot floor -> @oak_planks\n\
             \nstruct s size=5x5\n  floor mat_slot=floor\n",
        ),
    ),
    (
        // Also a header, so also a whole source. `version<1.20` is the
        // shape an author reaches for when they want a ceiling; only `>=`
        // is defined, and the line used to declare nothing at all.
        "E_INVALID_REQUIRES",
        Source::Whole(
            "@requires version<1.20\n\
             \ntheme t:\n  slot floor -> @oak_planks\n\
             \nstruct s size=5x5\n  floor mat_slot=floor\n",
        ),
    ),
    (
        // The site half of this code. It is the half that had no reporter
        // at either stage — a geometry row among a site's placements
        // produced neither voxels nor a diagnostic — so it is the one
        // worth carrying through the build commands.
        "E_MISPLACED_MEMBER",
        Source::WithPrologue(
            "def hut size=3x3:\n  floor id=floor mat_slot=floor\n\
             \nsite s:\n  place id=a use=hut theme=t at=origin\n\
             \x20\x20floor id=stray mat_slot=floor\n",
        ),
    ),
    (
        "E_UNKNOWN_KEYWORD",
        Source::WithPrologue("struct s size=5x5\n  torch mat_slot=wall\n"),
    ),
    (
        // The issue's own repro: one letter, and the wall is built
        // without the height it asked for.
        "E_UNKNOWN_ARGUMENT",
        Source::WithPrologue("struct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n"),
    ),
    (
        // `spec/syntax.md` §5.1's own forbidden example.
        "E_UNEXPECTED_POSITIONAL",
        Source::WithPrologue("struct s size=5x5\n  window front G 2 2 2x2 mat_slot=wall\n"),
    ),
    (
        // The site half of this code's message. The geometry half is
        // covered by `cairn-lang-core`'s own suite; what the CLI path
        // adds is that the whole stream survives the trip through the
        // build commands, and the two scopes render different prose.
        "E_UNSUPPORTED_NESTING",
        Source::WithPrologue(
            "def hut size=3x3:\n  floor id=floor mat_slot=floor\n\
             \nsite s:\n  place id=a use=hut theme=t at=origin\n\
             \x20\x20\x20\x20place id=b use=hut theme=t east_of=a gap=4\n",
        ),
    ),
    (
        "E_TYPE_MISMATCH_LABEL",
        Source::WithPrologue("struct s size=5x5\n  floor id=1 mat_slot=floor\n"),
    ),
    (
        "E_TYPE_MISMATCH_SIZE",
        Source::WithPrologue("struct s size=abc\n  floor mat_slot=floor\n"),
    ),
    (
        "E_CONNECT_ARITY",
        Source::WithPrologue(
            "def hut size=3x3:\n  floor id=floor mat_slot=floor\n\
             \x20\x20door id=entry side=front at=center\n\
             \nsite s:\n  place id=a use=hut theme=t at=origin\n\
             \x20\x20place id=b use=hut theme=t east_of=a gap=4\n\
             \x20\x20connect a.entry b.entry path=@gravel\n",
        ),
    ),
    (
        "E_TRUTH_TABLE_EMPTY",
        Source::WithPrologue(
            "struct s size=5x5\n  floor mat_slot=floor\n\
             \x20\x20assert truth(sig.a, sig.b -> sig.o) { }\n",
        ),
    ),
    (
        // Every combination assigned, so the repeat is the only finding:
        // a table short of a row would add a warning, and the parity
        // assertions are written per error.
        "E_TRUTH_TABLE_CONFLICT",
        Source::WithPrologue(
            "struct s size=5x5\n  floor mat_slot=floor\n\
             \x20\x20assert truth(sig.a, sig.b -> sig.o) { 00->0; 00->1; 01->0; 10->0; 11->0 }\n",
        ),
    ),
];

/// Resolver-origin fixtures. `check` merges the resolver's findings, so
/// these travel the same path — and they are the ones that were reported
/// twice when the build commands appended `Resolution::diagnostics` on top
/// of `check`'s already-merged output. Without them in the population the
/// duplicated half is not represented at all.
const RESOLVER_FIXTURES: &[(&str, Source)] = &[
    (
        "E_UNRESOLVED_PLACE_REF",
        // Also emits `W_UNUSED_DEF`, which puts a warning *before* the error
        // in span order — so a stream that is re-sorted or re-ordered shows up.
        Source::WithPrologue(
            "def hut size=4x4:\n  floor mat_slot=floor\n\
             \nsite s:\n  place id=a use=nosuchdef theme=t at=origin\n",
        ),
    ),
    (
        // The site half of the place-row family: three keys, one code, and
        // the row is dropped from the build for each of them.
        "E_INCOMPLETE_PLACE",
        Source::WithPrologue(
            "def hut size=3x3:\n  floor id=floor mat_slot=floor\n\
             \nsite s:\n  place id=a use=hut theme=t at=origin\n\
             \x20\x20place id=b theme=t east_of=a gap=4\n",
        ),
    ),
    (
        "E_UNRESOLVED_SLOT",
        Source::WithPrologue("struct s size=5x5\n  floor mat_slot=nosuchslot\n"),
    ),
    (
        // `id=` takes a string literal, so its contents used to reach the
        // scope-key builder unexamined — and `.` is one of the separators
        // that key is joined on.
        "E_INVALID_PLACE_ID",
        Source::WithPrologue(
            "def hut size=3x3:\n  floor id=floor mat_slot=floor\n\
             \nsite s:\n  place id=\"home.1\" use=hut theme=t at=origin\n",
        ),
    ),
];

fn all_fixtures() -> impl Iterator<Item = &'static (&'static str, Source)> {
    SYNTACTIC_FIXTURES.iter().chain(RESOLVER_FIXTURES)
}

/// Write one fixture into its own directory.
///
/// The directory is numbered rather than named after the diagnostic code on
/// purpose: every diagnostic line begins with the file path, so a directory
/// called `E_DUPLICATE_SIZE` made `stderr.contains("E_DUPLICATE_SIZE")` true
/// no matter which code — if any — the command actually reported.
fn write_fixture(root: &Path, index: usize, source: &Source) -> PathBuf {
    let dir = root.join(format!("fixture_{index:02}"));
    fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("fixture.crn");
    fs::write(&path, source.text()).expect("write fixture");
    path
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

fn exit_code(out: &std::process::Output) -> i32 {
    out.status.code().expect("process exited via a signal")
}

/// A diagnostic reduced to what parity is about: where it points, how bad it
/// is, and which code it carries. The prose `primary` is deliberately left
/// out — `diagnostic.rs` states the code, not the prose, is the contract.
type Reported = (u64, u64, String, String);

/// Diagnostics from `cairn check --format json`, in emission order.
fn check_stream(path: &Path) -> Vec<Reported> {
    let out = run(&["check", path.to_str().unwrap(), "--format", "json"]);
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    parsed
        .as_array()
        .expect("array of diagnostics")
        .iter()
        .map(|d| {
            (
                d["line"].as_u64().expect("line"),
                d["col"].as_u64().expect("col"),
                d["severity"].as_str().expect("severity").to_owned(),
                d["code"].as_str().expect("code").to_owned(),
            )
        })
        .collect()
}

/// Diagnostics parsed back out of a build command's stderr, in emission
/// order. Lines that are not `FILE:LINE:COL: SEVERITY[CODE]: ...` — notes,
/// `wrote ...`, plain `error:` messages — are skipped.
///
/// Parsing rather than substring-matching is what makes these assertions
/// mean something: it pins which code was reported, where, and how often.
fn stderr_stream(stderr: &str) -> Vec<Reported> {
    stderr
        .lines()
        .filter_map(|line| {
            // The file path may itself contain `:` (`C:\...`), so anchor on
            // the severity marker and walk backwards from there.
            let (head, tail) = ["error[", "warning["].iter().find_map(|marker| {
                let needle = format!(": {marker}");
                line.find(&needle)
                    .map(|i| (&line[..i], &line[i + ": ".len()..]))
            })?;
            let (severity, tail) = tail.split_once('[')?;
            let (code, _) = tail.split_once(']')?;
            let (head, col) = head.rsplit_once(':')?;
            let (_, line_no) = head.rsplit_once(':')?;
            Some((
                line_no.parse().ok()?,
                col.parse().ok()?,
                severity.to_owned(),
                code.to_owned(),
            ))
        })
        .collect()
}

/// The four subcommands, and how to invoke each against a fixture.
fn build_commands(path: &str, out_dir: &str) -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("lower", vec!["lower".into(), path.into()]),
        ("info", vec!["info".into(), path.into()]),
        (
            "compile",
            vec![
                "compile".into(),
                path.into(),
                "--edition".into(),
                "java".into(),
                "--out".into(),
                out_dir.into(),
            ],
        ),
    ]
}

#[test]
fn parity_1_check_reports_each_fixture_code_exactly_once() {
    let tmp = TempDir::new().expect("tempdir");
    for (index, (code, body)) in all_fixtures().enumerate() {
        let path = write_fixture(tmp.path(), index, body);
        let stream = check_stream(&path);
        let errors: Vec<&Reported> = stream.iter().filter(|d| d.2 == "error").collect();
        assert_eq!(
            errors.len(),
            1,
            "{code}: fixture should trip exactly one error, got {errors:?}",
        );
        assert_eq!(errors[0].3, *code, "{code}: wrong code reported");
        let out = run(&["check", path.to_str().unwrap()]);
        assert_eq!(exit_code(&out), 1, "{code}: check should exit 1");
    }
}

#[test]
fn parity_2_build_commands_replay_the_check_stream_verbatim() {
    // The single assertion this suite exists for. Each build command must
    // report exactly what `check` reported, in the same order, before adding
    // any lowering diagnostics of its own.
    //
    // Prefix equality alone would not be enough: appending a second copy of
    // the resolver's findings after `check`'s output keeps the prefix intact
    // while doubling every resolver diagnostic and, because `check` sorts by
    // span and the resolver does not, printing the copy out of order. The
    // occurrence count is what catches that.
    let tmp = TempDir::new().expect("tempdir");
    for (index, (code, body)) in all_fixtures().enumerate() {
        let path = write_fixture(tmp.path(), index, body);
        let expected = check_stream(&path);
        assert!(!expected.is_empty(), "{code}: fixture reports nothing");
        let out_dir = path.parent().expect("fixture dir").join("out");

        for (name, args) in build_commands(path.to_str().unwrap(), out_dir.to_str().unwrap()) {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = run(&argv);
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            let actual = stderr_stream(&stderr);

            assert!(
                actual.starts_with(&expected),
                "{code}: `{name}` must replay the check stream first\n  \
                 expected prefix: {expected:?}\n  actual: {actual:?}\n  stderr:\n{stderr}",
            );
            for diagnostic in &expected {
                let seen = actual.iter().filter(|d| *d == diagnostic).count();
                assert_eq!(
                    seen, 1,
                    "{code}: `{name}` reported {diagnostic:?} {seen} times\n  \
                     actual: {actual:?}\n  stderr:\n{stderr}",
                );
            }
        }
    }
}

#[test]
fn parity_3_build_commands_refuse_every_check_error() {
    let tmp = TempDir::new().expect("tempdir");
    for (index, (code, body)) in all_fixtures().enumerate() {
        let path = write_fixture(tmp.path(), index, body);
        let out_dir = path.parent().expect("fixture dir").join("out");

        for (name, args) in build_commands(path.to_str().unwrap(), out_dir.to_str().unwrap()) {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = run(&argv);
            assert_eq!(
                exit_code(&out),
                1,
                "{code}: `{name}` accepted a source check rejects; stderr={}",
                String::from_utf8_lossy(&out.stderr),
            );
            assert!(
                out.stdout.is_empty(),
                "{code}: `{name}` wrote {} bytes to stdout while refusing; a redirect \
                 keeps that file regardless of the exit code",
                out.stdout.len(),
            );
        }
    }
}

#[test]
fn parity_4_a_refused_compile_leaves_no_artifact_and_no_lockfile() {
    let tmp = TempDir::new().expect("tempdir");
    for (index, (code, body)) in all_fixtures().enumerate() {
        let path = write_fixture(tmp.path(), index, body);
        // Pre-create the output directory so its emptiness is a real
        // observation rather than the read failing on a missing path.
        let out_dir = path.parent().expect("fixture dir").join("out");
        fs::create_dir_all(&out_dir).expect("create out dir");

        let out = run(&[
            "compile",
            path.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.to_str().unwrap(),
        ]);
        assert_eq!(exit_code(&out), 1, "{code}: compile should refuse");

        let lock = path.with_extension("crn.lock");
        assert!(
            !lock.exists(),
            "{code}: a refused compile must not certify the source with {}",
            lock.display(),
        );
        let artifacts: Vec<PathBuf> = fs::read_dir(&out_dir)
            .expect("out dir exists")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        assert!(
            artifacts.is_empty(),
            "{code}: a refused compile must not leave artifacts, found {artifacts:?}",
        );
    }
}

#[test]
fn parity_5_a_template_only_library_compiles_to_nothing_without_complaint() {
    // The case `c26_bare_def_without_place_emits_w_unused_def_and_no_nbt`
    // pins: a `def` no site instantiates is a template, templates lower to
    // no voxels, and that is not a failure. Declaring nothing is different
    // from losing something — which is what `parity_6` covers.
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("library.crn");
    fs::write(
        &path,
        "@cairn 2026.06\n\ntheme t:\n  slot wall -> @cobblestone\n\
         \ndef hut size=4x4:\n  walls class=outer mat_slot=wall height=3\n",
    )
    .expect("write library");
    let out_dir = tmp.path().join("out");

    let out = run(&[
        "compile",
        path.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(exit_code(&out), 0, "stderr={stderr}");
    assert!(
        !stderr.contains("E_PARTIAL_BUILD"),
        "a template library requests no scopes, so nothing was dropped; stderr={stderr}",
    );
    // `W_UNUSED_DEF` reaches the stream through `check`, exactly once.
    let reported = stderr_stream(&stderr);
    let unused: Vec<&Reported> = reported.iter().filter(|d| d.3 == "W_UNUSED_DEF").collect();
    assert_eq!(
        unused.len(),
        1,
        "expected one W_UNUSED_DEF, got {reported:?}"
    );
    assert!(
        path.with_extension("crn.lock").exists(),
        "a successful compile still writes its lockfile",
    );
}

#[test]
fn parity_6_a_partially_lowered_source_is_not_certified() {
    // Every code that drops a scope is Warning severity, so before this the
    // exit code stayed 0 and the lockfile still said `verified: true` for a
    // build missing one of the two structures the source asked for.
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("partial.crn");
    fs::write(
        &path,
        "@cairn 2026.06\n\ntheme t:\n  slot wall -> @cobblestone\n\
         \nstruct good size=4x4\n  walls class=outer mat_slot=wall height=3\n\
         \nstruct bad\n  walls class=outer mat_slot=wall height=3\n",
    )
    .expect("write partial source");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).expect("create out dir");

    let out = run(&[
        "compile",
        path.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(exit_code(&out), 1, "stderr={stderr}");
    assert!(
        stderr.contains("E_PARTIAL_BUILD") && stderr.contains("struct::bad"),
        "the refusal must name the scope that went missing; stderr={stderr}",
    );
    assert!(
        !path.with_extension("crn.lock").exists(),
        "a partial build must not be certified by a lockfile",
    );
    let artifacts: Vec<PathBuf> = fs::read_dir(&out_dir)
        .expect("out dir exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    assert!(
        artifacts.is_empty(),
        "the structure that did lower must not be written either, found {artifacts:?}",
    );
}

#[test]
fn parity_7_every_example_still_passes_all_four_commands() {
    let tmp = TempDir::new().expect("tempdir");
    let mut seen = 0usize;
    for entry in fs::read_dir(examples_dir()).expect("read examples") {
        let src = entry.expect("dir entry").path();
        if src.extension().and_then(|e| e.to_str()) != Some("crn") {
            continue;
        }
        seen += 1;
        let name = src.file_name().expect("file name").to_owned();
        // Copy into the tempdir: `compile` writes its lockfile next to the
        // source, and `examples/` is not ours to dirty.
        let work = tmp.path().join(&name);
        fs::copy(&src, &work).expect("copy example");
        let path = work.to_str().unwrap();

        let out = run(&["check", path]);
        assert_eq!(
            exit_code(&out),
            0,
            "{name:?}: check regressed; stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
        let expected = check_stream(&work);

        let out_dir = tmp.path().join(format!("out-{}", name.to_string_lossy()));
        for (cmd, args) in build_commands(path, out_dir.to_str().unwrap()) {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = run(&argv);
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            assert_eq!(
                exit_code(&out),
                0,
                "{name:?}: `{cmd}` regressed; stderr={stderr}",
            );
            // The corpus is clean, so this pins that a build command does not
            // *invent* diagnostics either — the direction an exit-code-only
            // assertion cannot see.
            assert!(
                stderr_stream(&stderr).starts_with(&expected),
                "{name:?}: `{cmd}` diverged from the check stream\n  \
                 expected prefix: {expected:?}\n  stderr:\n{stderr}",
            );
        }
    }
    assert!(seen > 0, "no examples found — the corpus path is wrong");
}
