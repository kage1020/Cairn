//! `cairn import`: the `.crn` it writes from a structure file, what it warns of, and what it
//! refuses before writing anything.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;

use crate::common::{
    Run, cairn_argv, compile, example_in_tempdir, folds_case, nbt_body, write_source,
};

/// The warning every import prints first.
const INTENT_LOSS: &str = "warning[W_SEMANTIC_LOSS]: an import recovers blocks and their \
                           regularity, not what the author meant: every member it writes is a \
                           raw block or fill, and none names a wall, a window or a material slot";

/// `cairn import` with `args`.
fn import(args: &[&Path]) -> Run {
    let args: Vec<&str> = args
        .iter()
        .map(|arg| arg.to_str().expect("a UTF-8 path"))
        .collect();
    Run::from(cairn_argv(&[["import"].as_slice(), &args].concat()))
}

/// `example` compiled for `edition` at `target` into `<tmp>/built`, and the artifact `name`.
fn built(example: &str, edition: &str, target: &str, name: &str) -> (tempfile::TempDir, PathBuf) {
    let (tmp, src) = example_in_tempdir(example);
    let out = tmp.path().join("built");
    let run = Run::from(compile(&src, edition, target, &out, None));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    let artifact = out.join(name);
    (tmp, artifact)
}

/// The note that closes a successful import.
fn stamped(edition: &str, target: &str, reason: &str, written: &Path) -> String {
    format!(
        "note: stamped {edition} {target}: {reason}; `cairn compile {} --edition {edition} \
         --target {target}` builds it",
        written.display()
    )
}

/// Every entry of `dir`, sorted.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("the directory reads")
        .map(|entry| {
            entry
                .expect("an entry reads")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn an_imported_example_compiles_back_to_the_very_file_it_came_from_on_each_edition() {
    for (edition, target, file, reason) in [
        (
            "java",
            "1.21.4",
            "cottage.nbt",
            "the shipped target the file was saved at",
        ),
        (
            "bedrock",
            "1.21.60",
            "cottage.mcstructure",
            "a shipped target that saves that block version",
        ),
    ] {
        let (tmp, artifact) = built("cottage.crn", edition, target, file);
        let run = import(&[&artifact]);
        let written = artifact.with_file_name("cottage.raw.crn");
        assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
        assert_eq!(run.stdout, format!("wrote {}\n", written.display()));
        assert_eq!(
            run.stderr,
            format!(
                "{INTENT_LOSS}\n{}\n",
                stamped(edition, target, reason, &written)
            )
        );
        let back = tmp.path().join("back");
        let run = Run::from(compile(&written, edition, target, &back, None));
        assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
        assert_eq!(
            fs::read(back.join(file)).expect("the rebuilt file reads"),
            fs::read(&artifact).expect("the imported file reads"),
            "{edition}"
        );
    }
}

#[test]
fn a_block_entity_no_raw_member_carries_is_warned_of_and_out_makes_its_directory() {
    let (tmp, artifact) = built("crossbar.crn", "java", "26.3", "crossbar.nbt");
    let written = tmp.path().join("lifted").join("raw").join("crossbar.crn");
    let run = import(&[&artifact, Path::new("--out"), &written]);
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert_eq!(run.stdout, format!("wrote {}\n", written.display()));
    assert_eq!(
        run.stderr,
        format!(
            "{INTENT_LOSS}\n\
             warning[W_SEMANTIC_LOSS]: `crossbar`: 1 block entity is not written, since an \
             import writes no block entity data, such as a chest's contents or a command \
             block's command: 1 `minecraft:comparator`\n\
             {}\n",
            stamped(
                "java",
                "26.3",
                "the shipped target the file was saved at",
                &written
            )
        )
    );
    let source = fs::read_to_string(&written).expect("the import is written");
    assert!(
        source.starts_with(
            "# imported from `crossbar.nbt`, a Java structure file saved at DataVersion 5023 \
             (26.3)\n"
        ),
        "{source}"
    );
}

#[test]
fn a_file_import_cannot_read_or_stamp_is_refused_with_nothing_written() {
    let (tmp, artifact) = built("cottage.crn", "java", "1.21.4", "cottage.nbt");
    let dir = tmp.path().join("built");
    let before = entries(&dir);
    let original = fs::read(&artifact).expect("the artifact reads");

    let source = tmp.path().join("cottage.crn");
    let legacy = dir.join("old.schematic");
    fs::write(&legacy, b"").expect("write the legacy file");
    let missing = dir.join("absent.nbt");
    let not_found = fs::File::open(&missing).expect_err("the file is absent");
    let truncated = dir.join("truncated.nbt");
    fs::write(&truncated, b"junk").expect("write the truncated file");
    let future = dir.join("future.nbt");
    fs::write(&future, with_data_version(&artifact, 9000)).expect("write the future file");
    let formats = "  note: name a `.nbt`, `.litematic`, `.schem` or `.mcstructure` file";

    let cases: [(Vec<&Path>, i32, String); 6] = [
        (
            vec![&source],
            2,
            format!(
                "error: `{}` is not a structure file `cairn import` reads\n{formats}\n",
                source.display()
            ),
        ),
        (
            vec![&legacy],
            2,
            format!(
                "error: `{}` is not a structure file `cairn import` reads\n  note: a \
                 `.schematic` stores the numeric block ids of the releases before 1.13, which \
                 this version does not read; save it as a `.schem` from WorldEdit 7 or later \
                 and name that\n{formats}\n",
                legacy.display()
            ),
        ),
        (
            vec![&missing],
            2,
            format!("error: cannot read `{}`: {not_found}\n", missing.display()),
        ),
        (
            vec![&artifact, Path::new("--out"), &artifact],
            2,
            format!(
                "error: `--out {}` names the file being imported\n  note: give `--out` a `.crn` \
                 path, or leave it out to write `<stem>.raw.crn`\n",
                artifact.display()
            ),
        ),
        (
            vec![&truncated],
            1,
            format!(
                "error: cannot import `{}`: nbt I/O: unexpected end of file\n",
                truncated.display()
            ),
        ),
        (
            vec![&future],
            1,
            format!(
                "error: cannot import `{}`: the file was saved at DataVersion 9000, past every \
                 java target this build ships (the newest is 26.3), so no target can check its \
                 blocks. Fix: save it from 26.3 or older, or import it with a newer cairn\n",
                future.display()
            ),
        ),
    ];
    for (args, code, stderr) in cases {
        let run = import(&args);
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr.as_str()),
            (Some(code), "", stderr.as_str()),
            "{args:?}"
        );
    }
    let mut after = before;
    after.extend(["future.nbt", "old.schematic", "truncated.nbt"].map(str::to_owned));
    after.sort();
    assert_eq!(entries(&dir), after);
    assert_eq!(fs::read(&artifact).expect("the artifact reads"), original);
}

/// The gzipped `.nbt` at `path` with its `DataVersion` set to `data_version`.
fn with_data_version(path: &Path, data_version: i32) -> Vec<u8> {
    let mut body = nbt_body(path);
    let tag = b"\x03\x00\x0bDataVersion";
    let at = body
        .windows(tag.len())
        .position(|window| window == tag)
        .expect("a `DataVersion` int")
        + tag.len();
    body[at..at + 4].copy_from_slice(&data_version.to_be_bytes());
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&body).expect("the body gzips");
    gz.finish().expect("the gzip finishes")
}

#[test]
fn a_cell_no_raw_member_writes_is_refused_with_nothing_written() {
    let tmp = tempfile::tempdir().expect("a temp dir");
    let src = write_source(
        tmp.path(),
        "pane.crn",
        "@cairn 2026.11.0\nstruct pane size=2x1\n  raw_block mat=minecraft:glass_pane at=0,0,0\n  \
         raw_block mat=minecraft:stone at=1,0,0\n",
    );
    let out = tmp.path().join("built");
    let run = Run::from(compile(&src, "java", "1.21.4", &out, None));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    let artifact = out.join("pane.nbt");
    let before = entries(&out);

    let run = import(&[&artifact]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (
            Some(1),
            "",
            format!(
                "error: cannot import `{}`: `pane` cell 0,0,0 holds `minecraft:glass_pane`, and \
                 the written raw members build \
                 `minecraft:glass_pane[east=true,north=false,south=false,west=false]` there, so \
                 no raw member writes it; the transliteration is refused rather than written \
                 wrong, and the file is worth reporting\n",
                artifact.display()
            )
            .as_str()
        )
    );
    assert_eq!(entries(&out), before);
}

#[test]
fn a_structure_file_is_named_by_its_extension_in_any_case() {
    let (tmp, artifact) = built("cottage.crn", "java", "1.21.4", "cottage.nbt");
    let upper = tmp.path().join("COTTAGE.NBT");
    fs::copy(&artifact, &upper).expect("copy the artifact");
    let run = import(&[&upper]);
    let written = tmp.path().join("COTTAGE.raw.crn");
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert_eq!(run.stdout, format!("wrote {}\n", written.display()));
    let source = fs::read_to_string(&written).expect("the import is written");
    assert_eq!(
        source.lines().next(),
        Some(
            "# imported from `COTTAGE.NBT`, a Java structure file saved at DataVersion 4189 \
             (1.21.4)"
        )
    );
}

/// The note under an `--out` a link makes the file being imported, then `leave_out`.
fn linked_out_fix(leave_out: &str) -> String {
    format!("  note: a link makes them one file; give `--out` a `.crn` path elsewhere{leave_out}\n")
}

/// What leaving `--out` out writes, when nothing links it to the file.
const LEAVE_OUT: &str = ", or leave it out to write `<stem>.raw.crn`";

/// Import `artifact` with `args` after it, and assert it is refused with exit `code` and `stderr`,
/// writing nothing in its directory and leaving the file as it was.
fn assert_import_refused(artifact: &Path, args: &[&Path], code: i32, stderr: &str) {
    let dir = artifact.parent().expect("the artifact has a directory");
    let original = fs::read(artifact).expect("the artifact reads");
    let before = entries(dir);

    let run = import(&[[artifact].as_slice(), args].concat());

    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(code), "", stderr),
        "{args:?}",
    );
    assert_eq!(entries(dir), before, "{args:?}");
    assert_eq!(
        fs::read(artifact).expect("the artifact reads"),
        original,
        "{args:?}"
    );
}

#[test]
fn an_out_reaching_the_imported_file_by_another_name_is_refused() {
    let (_tmp, artifact) = built("cottage.crn", "java", "1.21.4", "cottage.nbt");
    let dir = artifact.parent().expect("built").to_path_buf();
    let twin = dir.join("twin.crn");
    fs::hard_link(&artifact, &twin).expect("hard-link the artifact");
    let names = |out: &Path| {
        format!(
            "error: `--out {}` names the file being imported\n",
            out.display()
        )
    };

    assert_import_refused(
        &artifact,
        &[Path::new("--out"), &twin],
        2,
        &format!("{}{}", names(&twin), linked_out_fix(LEAVE_OUT)),
    );
    let upper = dir.join("COTTAGE.NBT");
    if folds_case(&dir) {
        assert_import_refused(
            &artifact,
            &[Path::new("--out"), &upper],
            2,
            &format!(
                "{}  note: give `--out` a `.crn` path{LEAVE_OUT}\n",
                names(&upper)
            ),
        );
    }
}

#[test]
fn an_out_whose_scratch_file_reaches_the_imported_file_is_refused() {
    let (_tmp, artifact) = built("cottage.crn", "java", "1.21.4", "cottage.nbt");
    let dir = artifact.parent().expect("built").to_path_buf();
    for name in ["staged.crn.tmp", "backup.crn.bak"] {
        fs::hard_link(&artifact, dir.join(name)).expect("hard-link the artifact");
    }

    for (out, scratch) in [
        ("staged.crn", "staged.crn.tmp"),
        ("backup.crn", "backup.crn.bak"),
    ] {
        let (out, scratch) = (dir.join(out), dir.join(scratch));
        assert_import_refused(
            &artifact,
            &[Path::new("--out"), &out],
            2,
            &format!(
                "error: `--out {}` would put its scratch file `{}` over the file being \
                 imported\n{}",
                out.display(),
                scratch.display(),
                linked_out_fix(LEAVE_OUT),
            ),
        );
    }
}

#[test]
fn a_file_at_a_scratch_path_of_the_source_refuses_the_import_and_is_left_as_it_was() {
    let (_tmp, artifact) = built("cottage.crn", "java", "1.21.4", "cottage.nbt");
    let out = artifact.with_file_name("cottage.raw.crn");
    for suffix in [".tmp", ".bak"] {
        let scratch = artifact.with_file_name(format!("cottage.raw.crn{suffix}"));
        fs::write(&scratch, "my notes").expect("plant the file");
        assert_import_refused(
            &artifact,
            &[],
            1,
            &format!(
                "error: source `{}` is written through `{}`, which already exists\n  note: a \
                 file is written at its path with `.tmp` added and renamed into place, and one it \
                 replaces is moved aside to its path with `.bak` added\n  fix: move `{}` aside, \
                 then import again\n",
                out.display(),
                scratch.display(),
                scratch.display(),
            ),
        );
        assert_eq!(
            fs::read_to_string(&scratch).expect("the planted file reads"),
            "my notes"
        );
        fs::remove_file(&scratch).expect("clear the planted file");
    }
}

#[test]
fn a_default_out_linked_to_the_imported_file_is_refused_with_no_default_offered() {
    let (_tmp, artifact) = built("cottage.crn", "java", "1.21.4", "cottage.nbt");
    let out = artifact.with_file_name("cottage.raw.crn");
    let scratch = artifact.with_file_name("cottage.raw.crn.tmp");
    fs::hard_link(&artifact, &scratch).expect("hard-link the artifact");

    assert_import_refused(
        &artifact,
        &[],
        2,
        &format!(
            "error: source `{}` would put its scratch file `{}` over the file being imported\n{}",
            out.display(),
            scratch.display(),
            linked_out_fix(""),
        ),
    );
    assert_import_refused(
        &artifact,
        &[Path::new("--out"), &out],
        2,
        &format!(
            "error: `--out {}` would put its scratch file `{}` over the file being imported\n{}",
            out.display(),
            scratch.display(),
            linked_out_fix(""),
        ),
    );
}

#[cfg(unix)]
#[test]
fn an_out_the_file_system_cannot_answer_for_is_refused() {
    let (_tmp, artifact) = built("cottage.crn", "java", "1.21.4", "cottage.nbt");
    let out = artifact.with_file_name("raw.crn");
    let looped = artifact.with_file_name("raw.crn.tmp");
    std::os::unix::fs::symlink("raw.crn.tmp", &looped).expect("symlink to itself");
    let error = fs::metadata(&looped).expect_err("a loop reaches no file");

    assert_import_refused(
        &artifact,
        &[Path::new("--out"), &out],
        1,
        &format!(
            "error: cannot tell whether source `{}` would be written over `{}`, the file being \
             imported: `{}`: {error}\n  note: check that `{}` can be read, or give `--out` a \
             `.crn` path elsewhere\n",
            out.display(),
            artifact.display(),
            looped.display(),
            looped.display(),
        ),
    );
}
