//! Helpers the harness tests share: the `cairn` binary, a compiler pinned to one target, the
//! fixture prompt set, and the replies a replay answers it with.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use eval::compiler::Compiler;
use eval::conversation::{SOURCE, Target};
use eval::model::replay::Replay;
use eval::prompts::PromptSet;
use eval::record::RunRecord;
use eval::run::{Settings, run};

/// The model id the tests' runs name.
pub const MODEL: &str = "replay-placeholder";

/// A shed whose `walls` line carries an argument `walls` does not read.
pub const SHED_UNKNOWN_ARGUMENT: &str = "\
theme stone:
  slot floor -> @oak_planks
  slot wall -> @cobblestone

struct shed size=5x5
  floor mat_slot=floor
  walls mat_slot=wall height=3 colour=grey
  door side=front at=center
";

/// The shed, as it checks clean.
pub const SHED: &str = "\
theme stone:
  slot floor -> @oak_planks
  slot wall -> @cobblestone

struct shed size=5x5
  floor mat_slot=floor
  walls mat_slot=wall height=3
  door side=front at=center
";

/// The shed with a window in its back wall, written with its arguments in another order.
pub const SHED_WITH_WINDOW: &str = "\
theme stone:
  slot floor -> @oak_planks
  slot wall -> @cobblestone
  slot glass -> @glass_pane

# The window is new.
struct shed size=5x5
  floor mat_slot=floor
  walls height=3 mat_slot=wall
  door side=front at=center
  window side=back offset=2 y=1 size=1x1 mat_slot=glass
";

/// A pavilion with a flat roof.
pub const PAVILION: &str = "\
theme pavilion:
  slot floor -> @oak_planks
  slot wall -> @cobblestone
  slot roof -> @spruce_planks

struct pavilion size=5x5
  floor mat_slot=floor
  walls mat_slot=wall height=3
  roof kind=flat mat_slot=roof
";

/// A theme and nothing it applies to, which builds no block.
pub const HUT_THEME_ONLY: &str = "\
theme hut:
  slot wall -> @oak_planks
";

/// A hut whose walls name no material.
pub const HUT_NO_MATERIAL: &str = "\
theme hut:
  slot wall -> @oak_planks

struct hut size=5x5
  walls height=3
";

/// A hut whose window runs past the end of its wall.
pub const HUT_WINDOW_PAST_WALL: &str = "\
theme hut:
  slot wall -> @oak_planks

struct hut size=5x5
  walls mat_slot=wall height=3
  window side=front offset=4 y=1 size=2x1 mat_slot=wall
";

/// A reply holding `program` in a block marked `language`.
#[must_use]
pub fn reply(language: &str, program: &str) -> String {
    format!("Here it is.\n\n```{language}\n{program}```\n")
}

/// A replay file answering the fixture set: `shed` on its second attempt, `pavilion` on its
/// first, `hut` never, and `shed-window` on its first.
#[must_use]
pub fn replay_file() -> String {
    let conversations = [
        (
            "shed",
            vec![reply("cairn", SHED_UNKNOWN_ARGUMENT), reply("cairn", SHED)],
        ),
        ("pavilion", vec![reply("cairn", PAVILION)]),
        (
            "hut",
            vec![
                "A hut needs walls before it can have windows.\n".to_owned(),
                reply("cairn", HUT_THEME_ONLY),
                reply("cairn", HUT_NO_MATERIAL),
                reply("crn", HUT_WINDOW_PAST_WALL),
            ],
        ),
        ("shed-window", vec![reply("cairn", SHED_WITH_WINDOW)]),
    ];
    let mut file = String::new();
    for (id, replies) in conversations {
        let _ = write!(file, "[[conversation]]\nid = \"{id}\"\nreplies = [\n");
        for reply in replies {
            let _ = write!(file, "'''\n{reply}''',\n");
        }
        file.push_str("]\n\n");
    }
    file
}

/// A path under this crate's `tests/harness/fixtures`.
#[must_use]
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/harness/fixtures")
        .join(name)
}

/// The fixture prompt set.
#[must_use]
pub fn prompts() -> PromptSet {
    PromptSet::load(&fixture("prompts")).expect("the fixture prompt set loads")
}

/// The target the tests check and compile at.
#[must_use]
pub fn target() -> Target {
    Target {
        edition: "java".to_owned(),
        version: "1.21.4".to_owned(),
    }
}

/// This checkout's `cairn`, built once per test binary.
#[must_use]
pub fn cairn() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| Compiler::build().expect("cargo builds `cairn`"))
}

/// A compiler checking and compiling at [`target`].
#[must_use]
pub fn compiler() -> Compiler {
    Compiler::new(cairn().to_path_buf(), target())
}

/// The fixture set's run, answered by [`replay_file`].
#[must_use]
pub fn replayed_run() -> RunRecord {
    let mut model = Replay::parse(&replay_file()).expect("the replay parses");
    let settings = Settings {
        model: MODEL.to_owned(),
        max_tokens: 16000,
    };
    run(&settings, &prompts(), &mut model, &compiler(), &mut |_| {})
        .expect("the replayed run completes")
}

/// What `cairn check` prints on stderr for `program`, at [`target`].
#[must_use]
pub fn check_report(program: &str) -> String {
    let dir = tempfile::tempdir().expect("a scratch directory");
    std::fs::write(dir.path().join(SOURCE), program).expect("the program is written");
    let target = target();
    let output = Command::new(cairn())
        .args([
            "check",
            "--edition",
            &target.edition,
            "--target",
            &target.version,
            SOURCE,
        ])
        .current_dir(dir.path())
        .output()
        .expect("cairn runs");
    String::from_utf8(output.stderr).expect("cairn prints UTF-8")
}
