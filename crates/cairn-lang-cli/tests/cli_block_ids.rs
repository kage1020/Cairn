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

/// A rename is answered from the pack's alias table, which is the case a
/// typo finder structurally cannot reach.
///
/// Bedrock spells Java's `light` `light_block_0` … `light_block_15`, eight
/// edits away and sixteen ids wide. No distance threshold that keeps
/// `oak_plank` → `oak_planks` honest will ever connect the two, so the
/// answer has to come from a table that states outright that these names
/// are one block.
#[test]
fn a_rename_is_answered_with_the_name_the_target_uses() {
    let fixture = Fixture::new("rename", &source_binding("light"));
    let out = compile(&fixture, "bedrock", "1.21.60");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("minecraft:light_block_0") && stderr.contains("alias table"),
        "expected the alias answer, got: {stderr}",
    );
    assert!(
        !stderr.contains("is near enough to suggest"),
        "the no-candidate note is what this replaces, got: {stderr}",
    );
}

/// The table is symmetric because a group carries no direction: which
/// spelling is the local one is a question about the target, and the
/// target's own id table answers it.
#[test]
fn the_same_group_answers_from_either_edition() {
    let on_bedrock = Fixture::new("sign-bedrock", &source_binding("oak_sign"));
    let out = compile(&on_bedrock, "bedrock", "1.21.60");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("minecraft:standing_sign"),
        "bedrock spells the oak sign `standing_sign`, got: {}",
        stderr_of(&out),
    );

    let on_java = Fixture::new("sign-java", &source_binding("standing_sign"));
    let out = compile(&on_java, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("minecraft:oak_sign"),
        "java spells the same block `oak_sign`, got: {}",
        stderr_of(&out),
    );
}

/// An id no group names still says plainly that it has no candidate.
///
/// The alias table is not a second guess at what an author meant — it
/// answers where the pack knows the block under another name and says
/// nothing where it does not, which leaves the original message intact for
/// an id that is simply not a block.
#[test]
fn an_id_no_group_names_still_says_it_has_no_candidate() {
    let fixture = Fixture::new("no-group", &source_binding("totally_not_a_block"));
    let out = compile(&fixture, "bedrock", "1.21.60");
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("is near enough to suggest") && !stderr.contains("alias table"),
        "expected the no-candidate note, got: {stderr}",
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

/// `cairn check` does not run block-array lowering at all, so no
/// lowering-stage code reaches it — `E_UNKNOWN_ID` included. A source
/// `compile` refuses for an unknown id therefore still passes `check`.
///
/// Worth pinning rather than leaving implicit: it is a real hole for
/// anyone gating CI on `cairn check`, and it widened by one code here.
#[test]
fn check_stays_silent_about_ids_because_it_does_not_lower() {
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

/// A walkway's `path=@ID` reaches the check by a second route.
///
/// `lower_connects` resolves it outside `resolve_member_state`, so the
/// member arm passing says nothing about this one. Losing it would put a
/// walkway strip of a non-existent block into a `.nbt` at exit 0 — and
/// unlike a member, a walkway that quietly does not appear is easy to
/// mistake for a routing decision.
#[test]
fn a_walkway_path_id_is_checked_too() {
    let fixture = Fixture::new("walkway", &two_huts_joined_by("totally_not_a_block"));
    let out = compile(&fixture, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("E_UNKNOWN_ID") && stderr.contains("minecraft:totally_not_a_block"),
        "expected the path material to be refused, got: {stderr}",
    );

    // The same shape with a real block has to still build, or the test
    // above would pass on any source that happens to fail.
    let ok = Fixture::new("walkway-ok", &two_huts_joined_by("gravel"));
    let out = compile(&ok, "java", "1.21.4");
    assert_eq!(
        out.status.code(),
        Some(0),
        "a walkway of a real block must compile; stderr: {}",
        stderr_of(&out),
    );
}

/// Two placed huts and a walkway between them, with `{path}` as the strip's
/// material.
fn two_huts_joined_by(path: &str) -> String {
    format!(
        "theme t:\n  slot floor -> @oak_planks\n  slot wall -> @cobblestone\n\n\
         def hut size=3x3:\n  floor id=f mat_slot=floor\n  \
         walls id=w mat_slot=wall height=2\n  door id=entry side=front at=center\n\n\
         site duo:\n  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=2\n  \
         connect a.entry to b.entry path=@{path}\n"
    )
}

/// `cairn lower` takes no `--target` at all, so it is in the same position
/// as `info` — it lowers, but against no version.
#[test]
fn lower_stays_silent_about_ids_because_it_takes_no_target() {
    let fixture = Fixture::new("lower", &source_binding("totally_not_a_block"));
    let out = Command::new(cargo_bin())
        .arg("lower")
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
        "lower has no target to check against, got: {combined}",
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_of(&out));
}

/// `cairn info` never says an id is unknown without naming the target that
/// says so.
///
/// It used to say nothing about ids at all, and the reason was that it had
/// no target to be right about: the range-wide pass gets no id table, so
/// an id valid for part of the range would have been refused against a
/// version nobody chose. That premise is gone — the `buildable targets`
/// row pins each supported version in turn — so the rule is now the
/// narrower one it was always standing in for. What must not appear is a
/// bare `E_UNKNOWN_ID` about the *source*; what may appear is one about a
/// named version, which is exactly what `compile --target` would print for
/// that version.
#[test]
fn info_blames_a_named_target_or_says_nothing_about_ids() {
    let fixture = Fixture::new("info", &source_binding("totally_not_a_block"));
    let out = Command::new(cargo_bin())
        .arg("info")
        .arg(fixture.source())
        .output()
        .expect("failed to invoke cairn binary");
    let stderr = stderr_of(&out);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        !stdout.contains("E_UNKNOWN_ID"),
        "the rows are about the source and carry no id verdict, got: {stdout}",
    );
    // Every id finding is introduced by the version it belongs to, and
    // there are as many introductions as findings.
    let findings = stderr.matches("E_UNKNOWN_ID").count();
    let attributions = stderr.matches("refuses this source").count();
    assert!(findings > 0, "the fixture's id is in no version: {stderr}");
    assert_eq!(
        findings, attributions,
        "each id finding should sit under the target that raised it: {stderr}",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "info reports; it does not refuse: {stderr}",
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
