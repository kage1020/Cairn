//! Parity between `cairn check` and the commands that build on it. A
//! source `check` rejects with an `Error`-severity finding does not lower,
//! report portability figures or compile, and the build commands report
//! `check`'s findings verbatim and in order before their own lowering
//! findings: the unpinned check's stream is a prefix of theirs, and a check
//! pinned to the compile's target reports exactly what the compile does.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::common::{Reported, cairn_argv, crn_examples, json_stream, stderr_stream};

/// Shared prologue so each fixture differs only in the construct that trips
/// its diagnostic.
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

/// One fixture per `Error`-severity code the syntactic passes inside
/// `check` emit.
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
        // Two rows with one keyword and attribute set select the same
        // members, so only the later `frame=` survives. Whole, so the rows
        // join the theme the struct binds.
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
        // The prologue already binds `theme t`, so the repeat is the only
        // construct this body adds.
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
        // `version<1.20` is the ceiling an author might reach for; only
        // `>=` is defined, so the line declares nothing.
        "E_INVALID_REQUIRES",
        Source::Whole(
            "@requires version<1.20\n\
             \ntheme t:\n  slot floor -> @oak_planks\n\
             \nstruct s size=5x5\n  floor mat_slot=floor\n",
        ),
    ),
    (
        // A member with no material paints nothing, an artifact empty
        // rather than wrong, so it has to be refused before anything is
        // emitted. (`run_lower` lowers first; what is ordered is the
        // output, not the passes.)
        "E_MISSING_MATERIAL",
        Source::WithPrologue("struct s size=5x5\n  walls class=outer height=3\n"),
    ),
    (
        // The tail belongs to a sensor; `walls` is not one, and the
        // signal it names is emitted by nothing.
        "E_MISPLACED_BINDING",
        Source::WithPrologue(
            "struct s size=5x5\n  walls class=outer mat_slot=wall height=3 -> sig.w\n",
        ),
    ),
    (
        // The site half of this code: a geometry row among a site's
        // placements lowers to no voxels, so this code is its only report.
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
        // One letter, and the wall is built without the height it
        // asked for.
        "E_UNKNOWN_ARGUMENT",
        Source::WithPrologue("struct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n"),
    ),
    (
        // The forbidden example `spec/syntax` "Lexical" gives.
        "E_UNEXPECTED_POSITIONAL",
        Source::WithPrologue("struct s size=5x5\n  window front G 2 2 2x2 mat_slot=wall\n"),
    ),
    (
        // The site half of this code's message; core's suite covers the
        // geometry half.
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

/// Resolver-origin fixtures. `check` merges the resolver's findings, so a
/// build command that also appended `Resolution::diagnostics` would report
/// these twice.
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
        // `id=` takes a string literal, whose contents would otherwise
        // reach the scope-key builder unexamined — and `.` is one of the
        // separators that key is joined on.
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

/// Write one fixture into its own directory.
///
/// Numbered rather than named after the code: every diagnostic line begins
/// with the path, so a directory named `E_DUPLICATE_SIZE` would satisfy any
/// search for that code.
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
    // A second copy of the resolver's findings appended after `check`'s
    // keeps the prefix intact; the occurrence count catches it.
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
    // A `def` no site instantiates is a template and lowers to no voxels:
    // declaring nothing, not losing something.
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
fn a_partially_lowered_source_is_not_certified() {
    // Every code that drops a scope is a warning, so only the refusal keeps
    // a `verified: true` lockfile from certifying half the source.
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
        // Copy into the tempdir: `compile` writes its lockfile next to the
        // source, and `examples/` is not ours to dirty.
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
            // The corpus is clean, so this also catches a build command
            // inventing diagnostics.
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
    // Equality both ways: the two run the same passes against the same
    // table. `--target latest` because that is `compile`'s default.
    //
    // A target the floors refuse is outside the equality, since `compile`
    // then pins no version and `check` does, but the only floor here is
    // `PROLOGUE`'s `version>=1.20`, which `latest` clears.
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
