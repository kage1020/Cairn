//! `cairn compile` against block ids the target does not have.
//!
//! Palette validation used to ask only whether an id had exactly one `:`,
//! because the registry packs carried no id table to ask anything else of.
//! `slot wall -> @totally_not_a_block` therefore compiled at exit 0 into a
//! structure file the game loads as air, and spec versioning-editions §10.4
//! says the opposite: "unknown IDs ... are hard errors. Silent substitution
//! and implicit dropping are forbidden."

use std::path::{Path, PathBuf};
use std::process::Command;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

/// A source plus the directory its artifacts would land in, both removed
/// when the test ends.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(label: &str, source: &str) -> Self {
        let mut dir = std::env::temp_dir();
        dir.push(format!("cairn-block-ids-{}-{label}", std::process::id()));
        // A leftover from an interrupted run would let
        // `a_refused_id_writes_nothing` read a stale artifact as a fresh
        // one, so the removal has to have happened — "it was not there" is
        // the only other acceptable outcome.
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!("cannot clear {}: {err}", dir.display()),
        }
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        std::fs::write(dir.join("s.crn"), source).expect("write source");
        Self { dir }
    }

    fn source(&self) -> PathBuf {
        self.dir.join("s.crn")
    }

    fn lock(&self) -> PathBuf {
        self.dir.join("s.crn.lock")
    }

    fn out(&self) -> PathBuf {
        self.dir.join("out")
    }

    /// Every file the compile could have produced.
    ///
    /// Every I/O failure panics rather than reading as "nothing there":
    /// the absence of artifacts is what one of these tests observes, and a
    /// walk that quietly gives up folds it toward passing.
    fn artifacts(&self) -> Vec<String> {
        fn walk(dir: &Path, into: &mut Vec<String>) {
            let entries = std::fs::read_dir(dir)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
            for entry in entries {
                let path = entry
                    .unwrap_or_else(|err| {
                        panic!("cannot read an entry of {}: {err}", dir.display())
                    })
                    .path();
                if path.is_dir() {
                    walk(&path, into);
                } else if path.extension().and_then(|e| e.to_str()) != Some("crn") {
                    into.push(path.file_name().expect("named").to_string_lossy().into());
                }
            }
        }
        let mut found = Vec::new();
        walk(&self.dir, &mut found);
        found.sort();
        found
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A struct with one painted floor whose material comes from `slot floor`.
/// `{id}` is what the theme binds, so each test names exactly the id it is
/// about.
fn source_binding(id: &str) -> String {
    format!("theme t:\n  slot floor -> @{id}\nstruct s size=2x2\n  floor mat_slot=floor\n")
}

fn compile(fixture: &Fixture, edition: &str, target: &str) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("compile")
        .arg(fixture.source())
        .args(["--edition", edition, "--target", target])
        .arg("--out")
        .arg(fixture.out())
        .arg("--lock")
        .arg(fixture.lock())
        .output()
        .expect("failed to invoke cairn binary")
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

#[test]
fn an_id_no_edition_has_is_refused_rather_than_written() {
    let fixture = Fixture::new("nonsense", &source_binding("totally_not_a_block"));
    let out = compile(&fixture, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("E_UNKNOWN_ID"),
        "expected E_UNKNOWN_ID, got: {stderr}",
    );
    assert!(
        stderr.contains("minecraft:totally_not_a_block"),
        "the message must name the id it refused, got: {stderr}",
    );
}

/// Exiting non-zero while leaving a structure or a lock behind still leaves
/// a file the next reader takes for a finished build.
///
/// The empty `--out` directory counts as an artifact here for the same
/// reason it does in `cli_version_cap`: it is created before the structure
/// tags are built, so a check placed one step later leaves it behind.
#[test]
fn a_refused_id_writes_nothing() {
    let fixture = Fixture::new("nothing", &source_binding("totally_not_a_block"));
    let out = compile(&fixture, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    assert_eq!(
        fixture.artifacts(),
        Vec::<String>::new(),
        "a refused compile must leave no structure and no lock behind",
    );
}

#[test]
fn a_java_only_block_is_refused_on_bedrock_and_compiles_on_java() {
    // `minecraft:light` is Java's light block. Bedrock 1.21.60 has no id of
    // that name at all — it spells the same thing `light_block_0` …
    // `light_block_15` — so the same source must part ways at the edition.
    let bedrock = Fixture::new("light-bedrock", &source_binding("light"));
    let out = compile(&bedrock, "bedrock", "1.21.60");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("E_UNKNOWN_ID") && stderr.contains("bedrock 1.21.60"),
        "the message must name the registry it checked, got: {stderr}",
    );

    let java = Fixture::new("light-java", &source_binding("light"));
    let out = compile(&java, "java", "1.21.4");
    assert_eq!(
        out.status.code(),
        Some(0),
        "the same source must compile on the edition that has the block; stderr: {}",
        stderr_of(&out),
    );
}

#[test]
fn a_typo_is_answered_with_the_id_the_target_does_spell() {
    let fixture = Fixture::new("typo", &source_binding("oak_plank"));
    let out = compile(&fixture, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("minecraft:oak_planks"),
        "a one-letter typo must be answered with the real id, got: {stderr}",
    );
}

/// The suggestion is a typo finder, not a rename map.
///
/// Bedrock calling Java's `light` `light_block` is six edits away, past
/// `nearest_match`'s cap, and no amount of searching the id table turns
/// that into a suggestion. What the message must not do is go silent about
/// it: the note has to say there is no candidate and what to do instead.
#[test]
fn a_rename_says_plainly_that_it_has_no_candidate() {
    let fixture = Fixture::new("rename", &source_binding("light"));
    let out = compile(&fixture, "bedrock", "1.21.60");
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("is near enough to suggest"),
        "expected the no-candidate note, got: {stderr}",
    );
    assert!(
        !stderr.contains("spells the nearest block"),
        "a rename must not be dressed up as a typo suggestion, got: {stderr}",
    );
}

/// A version inside one edition's own supported range can rename a block.
///
/// Bedrock 1.21.0 predates the flattening wave that turned `stonebrick`
/// into `stone_bricks`, so an id table folded flat across the edition would
/// accept both spellings everywhere and catch neither mistake.
#[test]
fn an_id_is_checked_against_the_version_and_not_the_edition() {
    let old = Fixture::new("flat-old", &source_binding("stone_bricks"));
    let out = compile(&old, "bedrock", "1.21.0");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("minecraft:stonebrick"),
        "1.21.0 must point at its own spelling, got: {}",
        stderr_of(&out),
    );

    let new = Fixture::new("flat-new", &source_binding("stone_bricks"));
    let out = compile(&new, "bedrock", "1.21.60");
    assert_eq!(
        out.status.code(),
        Some(0),
        "1.21.60 does declare stone_bricks; stderr: {}",
        stderr_of(&out),
    );
}

/// The same rename, reached through the pack rather than through the
/// author: `@floor.stone.smooth` is a valid token on both versions, and the
/// pack respells it per version so both compile.
#[test]
fn a_material_token_follows_the_rename_across_its_editions_range() {
    for target in ["1.21.0", "1.21.40", "1.21.60"] {
        let fixture = Fixture::new(
            &format!("token-{target}"),
            "theme t:\n  slot floor -> @floor.stone.smooth\nstruct s size=2x2\n  floor mat_slot=floor\n",
        );
        let out = compile(&fixture, "bedrock", target);
        assert_eq!(
            out.status.code(),
            Some(0),
            "@floor.stone.smooth must resolve on bedrock {target}; stderr: {}",
            stderr_of(&out),
        );
    }
}

/// `cairn check` pins no version, so it has no registry to check ids
/// against and must not invent one. A source `compile` refuses for an
/// unknown id still passes `check` — the alternative is `check` guessing a
/// version and refusing ids that are fine on the one the author compiles
/// for.
#[test]
fn check_stays_silent_about_ids_because_it_pins_no_version() {
    let fixture = Fixture::new("check", &source_binding("totally_not_a_block"));
    let out = Command::new(cargo_bin())
        .arg("check")
        .arg(fixture.source())
        .output()
        .expect("failed to invoke cairn binary");
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("E_UNKNOWN_ID"),
        "check has no target to check against, got: {stderr}",
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
}

/// `cairn info` reports across a pack's whole version range, so it is in
/// the same position as `check`.
#[test]
fn info_stays_silent_about_ids_for_the_same_reason() {
    let fixture = Fixture::new("info", &source_binding("totally_not_a_block"));
    let out = Command::new(cargo_bin())
        .arg("info")
        .arg(fixture.source())
        .output()
        .expect("failed to invoke cairn binary");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        stderr_of(&out),
    );
    assert!(
        !combined.contains("E_UNKNOWN_ID"),
        "info spans every target in the range, got: {combined}",
    );
}

/// An unresolvable `--target` leaves nothing to check ids against, and the
/// message the user gets must still be the one about their `--target`.
///
/// The target is resolved before lowering now, so a regression that
/// reported the resolution failure at the point it happens would move this
/// message ahead of every parse and lowering diagnostic.
#[test]
fn an_unresolvable_target_is_still_reported_as_a_target_problem() {
    let fixture = Fixture::new("badtarget", &source_binding("oak_planks"));
    let out = compile(&fixture, "java", "1.99");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("1.99") && !stderr.contains("E_UNKNOWN_ID"),
        "expected the unsupported-target message, got: {stderr}",
    );
}

/// The refusal points at the `slot` line that names the material, not at
/// the member that uses it or at the file as a whole.
///
/// Spec versioning-editions §10.4's worked example is `E_UNKNOWN_ID line
/// 12: ...` — a line the author can go and edit. The theme sits on line 2
/// of `source_binding`, and the `@id` it resolves to starts at column 17.
#[test]
fn the_refusal_points_at_the_line_that_names_the_material() {
    let fixture = Fixture::new("span", &source_binding("totally_not_a_block"));
    let out = compile(&fixture, "java", "1.21.4");
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("s.crn:2:17: error[E_UNKNOWN_ID]"),
        "expected the slot line and column, got: {stderr}",
    );
}
