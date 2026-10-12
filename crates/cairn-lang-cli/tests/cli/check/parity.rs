//! Parity between `cairn check` and the commands that build on it: a source `check` rejects
//! builds nothing, and the build commands report `check`'s findings first and verbatim.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::common::{Reported, cairn_argv, crn_examples, json_stream, stderr_stream};

/// Shared prologue, so each fixture differs only in the construct that trips its diagnostic.
const PROLOGUE: &str = "@cairn 2026.06\n\
@requires version>=1.20\n\
\n\
theme t:\n\
\x20\x20slot floor -> @oak_planks\n\
\x20\x20slot wall  -> @cobblestone\n\
\n";

/// A fixture's source: a body after [`PROLOGUE`], or a whole file where a prologue cannot fit.
pub(crate) enum Source {
    WithPrologue(&'static str),
    Whole(&'static str),
}

impl Source {
    pub(crate) fn text(&self) -> String {
        match self {
            Self::WithPrologue(body) => format!("{PROLOGUE}{body}"),
            Self::Whole(text) => (*text).to_owned(),
        }
    }
}

/// One fixture per `Error`-severity code the syntactic passes inside `check` emit.
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
        // Whole, so the rows join the theme the struct binds; only the later `frame=` survives.
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
        "E_DUPLICATE_CIRCUIT",
        Source::WithPrologue(
            "struct s size=5x5\n  floor mat_slot=floor\n  \
             circuit region=floor void=1\n  circuit region=floor void=2\n",
        ),
    ),
    (
        "E_DUPLICATE_ID",
        Source::WithPrologue(
            "struct s size=5x5\n  floor id=x mat_slot=floor\n\
             \x20\x20walls id=x class=outer mat_slot=wall height=3\n",
        ),
    ),
    (
        // The prologue already declares `theme t`.
        "E_DUPLICATE_ITEM",
        Source::WithPrologue(
            "theme t:\n  slot floor -> @stone\n\
             \nstruct s size=5x5\n  floor mat_slot=floor\n",
        ),
    ),
    (
        "E_DUPLICATE_HEADER",
        Source::Whole(
            "@cairn 2026.06\n@cairn 2026.07\n\
             \ntheme t:\n  slot floor -> @oak_planks\n\
             \nstruct s size=5x5\n  floor mat_slot=floor\n",
        ),
    ),
    (
        "E_INVALID_REQUIRES",
        Source::Whole(
            "@requires version<1.20\n\
             \ntheme t:\n  slot floor -> @oak_planks\n\
             \nstruct s size=5x5\n  floor mat_slot=floor\n",
        ),
    ),
    (
        "E_MISSING_MATERIAL",
        Source::WithPrologue("struct s size=5x5\n  walls class=outer height=3\n"),
    ),
    (
        "E_MISPLACED_BINDING",
        Source::WithPrologue(
            "struct s size=5x5\n  walls class=outer mat_slot=wall height=3 -> sig.w\n",
        ),
    ),
    (
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
        "E_UNKNOWN_ARGUMENT",
        Source::WithPrologue("struct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n"),
    ),
    (
        // The forbidden example `spec/syntax` "Lexical" gives.
        "E_UNEXPECTED_POSITIONAL",
        Source::WithPrologue("struct s size=5x5\n  window front G 2 2 2x2 mat_slot=wall\n"),
    ),
    (
        // The site half; core's suite covers the geometry half.
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
        "E_TRUTH_TABLE_DUPLICATE_INPUT",
        Source::WithPrologue(
            "struct s size=5x5\n  floor mat_slot=floor\n\
             \x20\x20assert truth(sig.a, sig.a -> sig.o) { 00 -> 0; 11 -> 1 }\n",
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
        // Every combination assigned, so the repeat is the only finding.
        "E_TRUTH_TABLE_CONFLICT",
        Source::WithPrologue(
            "struct s size=5x5\n  floor mat_slot=floor\n\
             \x20\x20assert truth(sig.a, sig.b -> sig.o) { 00->0; 00->1; 01->0; 10->0; 11->0 }\n",
        ),
    ),
];

/// Resolver-origin fixtures, which `check` already merges.
const RESOLVER_FIXTURES: &[(&str, Source)] = &[
    (
        "E_UNRESOLVED_PLACE_REF",
        // Also emits `W_UNUSED_DEF`, a warning ahead of the error in span order.
        Source::WithPrologue(
            "def hut size=4x4:\n  floor mat_slot=floor\n\
             \nsite s:\n  place id=a use=nosuchdef theme=t at=origin\n",
        ),
    ),
    (
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
        "E_INVALID_PLACE_ID",
        Source::WithPrologue(
            "def hut size=3x3:\n  floor id=floor mat_slot=floor\n\
             \nsite s:\n  place id=\"home.1\" use=hut theme=t at=origin\n",
        ),
    ),
    (
        // A `struct hut` and a `place id=hut` both write `hut.nbt`.
        "E_OUTPUT_NAME_COLLISION",
        Source::WithPrologue(
            "struct hut size=3x3\n  floor mat_slot=floor\n\
             \ndef house size=3x3:\n  floor mat_slot=floor\n\
             \nsite s:\n  place id=hut use=house theme=t at=origin\n",
        ),
    ),
];

/// Every fixture, each a source `check` refuses with the one code it names.
pub(crate) fn all_fixtures() -> impl Iterator<Item = &'static (&'static str, Source)> {
    SYNTACTIC_FIXTURES.iter().chain(RESOLVER_FIXTURES)
}

/// Write one fixture into its own directory, numbered so no path contains a code.
fn write_fixture(root: &Path, index: usize, source: &Source) -> PathBuf {
    let dir = root.join(format!("fixture_{index:02}"));
    fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("fixture.crn");
    fs::write(&path, source.text()).expect("write fixture");
    path
}

fn exit_code(out: &std::process::Output) -> i32 {
    out.status.code().expect("process exited via a signal")
}

/// Diagnostics from `cairn check --format json`, in emission order.
fn check_stream(path: &Path) -> Vec<Reported> {
    let out = cairn_argv(&["check", path.to_str().unwrap(), "--format", "json"]);
    json_stream(&String::from_utf8(out.stdout).expect("utf-8"))
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
fn check_reports_each_fixture_code_exactly_once() {
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
        let out = cairn_argv(&["check", path.to_str().unwrap()]);
        assert_eq!(exit_code(&out), 1, "{code}: check should exit 1");
    }
}

#[test]
fn build_commands_replay_the_check_stream_verbatim() {
    // A second copy of the resolver's findings keeps the prefix intact; the count catches it.
    let tmp = TempDir::new().expect("tempdir");
    for (index, (code, body)) in all_fixtures().enumerate() {
        let path = write_fixture(tmp.path(), index, body);
        let expected = check_stream(&path);
        assert!(!expected.is_empty(), "{code}: fixture reports nothing");
        let out_dir = path.parent().expect("fixture dir").join("out");

        for (name, args) in build_commands(path.to_str().unwrap(), out_dir.to_str().unwrap()) {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = cairn_argv(&argv);
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
fn build_commands_refuse_every_check_error() {
    let tmp = TempDir::new().expect("tempdir");
    for (index, (code, body)) in all_fixtures().enumerate() {
        let path = write_fixture(tmp.path(), index, body);
        let out_dir = path.parent().expect("fixture dir").join("out");

        for (name, args) in build_commands(path.to_str().unwrap(), out_dir.to_str().unwrap()) {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = cairn_argv(&argv);
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
fn a_template_only_library_compiles_to_nothing_without_complaint() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("library.crn");
    fs::write(
        &path,
        "@cairn 2026.06\n\ntheme t:\n  slot wall -> @cobblestone\n\
         \ndef hut size=4x4:\n  walls class=outer mat_slot=wall height=3\n",
    )
    .expect("write library");
    let out_dir = tmp.path().join("out");

    let out = cairn_argv(&[
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
fn a_partially_lowered_source_is_not_certified() {
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

    let out = cairn_argv(&[
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
}

#[test]
fn every_example_still_passes_all_four_commands() {
    let tmp = TempDir::new().expect("tempdir");
    for src in crn_examples() {
        let name = src.file_name().expect("file name").to_owned();
        // `compile` writes its lockfile next to the source, which must not be `examples/`.
        let work = tmp.path().join(&name);
        fs::copy(&src, &work).expect("copy example");
        let path = work.to_str().unwrap();

        let out = cairn_argv(&["check", path]);
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
            let out = cairn_argv(&argv);
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            assert_eq!(
                exit_code(&out),
                0,
                "{name:?}: `{cmd}` regressed; stderr={stderr}",
            );
            assert!(
                stderr_stream(&stderr).starts_with(&expected),
                "{name:?}: `{cmd}` diverged from the check stream\n  \
                 expected prefix: {expected:?}\n  stderr:\n{stderr}",
            );
        }
    }
}

#[test]
fn a_pinned_check_reports_exactly_what_the_compile_reports() {
    // `--target latest` is `compile`'s default, and clears `PROLOGUE`'s only floor.
    let tmp = TempDir::new().expect("tempdir");
    for (index, (code, body)) in all_fixtures().enumerate() {
        let path = write_fixture(tmp.path(), index, body);
        let source = path.to_str().unwrap();
        let out_dir = path.parent().expect("fixture dir").join("out");
        for edition in ["java", "bedrock"] {
            let checked = cairn_argv(&[
                "check",
                source,
                "--edition",
                edition,
                "--target",
                "latest",
                "--format",
                "json",
            ]);
            let checked_code = exit_code(&checked);
            let expected = json_stream(&String::from_utf8(checked.stdout).expect("utf-8"));
            assert!(
                !expected.is_empty(),
                "{code} on {edition}: pinned check reports nothing"
            );

            let out = cairn_argv(&[
                "compile",
                source,
                "--edition",
                edition,
                "--out",
                out_dir.to_str().unwrap(),
            ]);
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            assert_eq!(
                stderr_stream(&stderr),
                expected,
                "{code} on {edition}: the pinned check and the compile must report the same \
                 stream\n  check: {expected:?}\n  compile stderr:\n{stderr}",
            );
            assert_eq!(
                checked_code,
                exit_code(&out),
                "{code} on {edition}: and refuse the same source\n  compile stderr:\n{stderr}",
            );
        }
    }
}
