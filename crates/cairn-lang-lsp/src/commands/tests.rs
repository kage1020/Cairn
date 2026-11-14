use std::path::{Path, PathBuf};
use std::str::FromStr;

use cairn_lang_core::parse;
use cairn_lang_formats::build::{BuildTarget, lower_for_build};
use cairn_lang_formats::data_version::resolve_java_target;
use cairn_lang_formats::java_structure::write_structure_gzip;
use serde_json::json;

use super::*;
use crate::test_fixtures::{CLEAN, DUPLICATE};

const SHOP: &str = "file:///no/such/dir/shop.crn";

/// The path [`SHOP`] names on this platform.
fn shop_path() -> PathBuf {
    PathBuf::from(if cfg!(windows) {
        r"\no\such\dir\shop.crn"
    } else {
        "/no/such/dir/shop.crn"
    })
}

fn uri(text: &str) -> lsp_types::Uri {
    lsp_types::Uri::from_str(text).expect("a valid URI")
}

/// A store holding `text` at `at`.
fn holding(at: &str, text: &str) -> DocumentStore {
    let mut store = DocumentStore::new();
    store.open(uri(at), text.to_owned());
    store
}

fn run(
    store: &DocumentStore,
    command: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, CommandRefused> {
    execute_command(store, json!({ "command": command, "arguments": arguments }))
}

/// What `cairn info --format json` prints for `source` at [`shop_path`], over `editions`.
fn printed(source: &str, editions: &[&str]) -> serde_json::Value {
    let file = shop_path();
    let editions: Vec<String> = editions.iter().map(|&e| e.to_owned()).collect();
    let editions = requested_editions(&editions).expect("the editions parse");
    let lock = default_lock_path(&file);
    let request = InfoRequest {
        file: &file,
        source,
        editions: &editions,
        lock: &lock,
        errors_in_document: true,
    };
    match info(&request, &mut Unsent) {
        Ok(document) => serde_json::to_value(document),
        Err(refused) => serde_json::to_value(FailureDocument::new(source, &refused)),
    }
    .expect("the document serialises")
}

fn invalid(message: &str) -> Result<serde_json::Value, CommandRefused> {
    Err(CommandRefused {
        code: ErrorCode::InvalidParams as i32,
        message: message.to_owned(),
    })
}

#[test]
fn info_answers_with_the_report_cairn_info_prints_for_the_file() {
    let store = holding(SHOP, CLEAN);
    let answer = run(&store, INFO, &json!([{ "uri": SHOP }])).expect("a clean source is reported");
    assert_eq!(answer, printed(CLEAN, &["java", "bedrock"]));
    assert_eq!(
        answer["verified_target"],
        json!({
            "path": default_lock_path(&shop_path()).display().to_string(),
            "status": "missing",
        }),
    );
}

#[test]
fn info_reports_on_the_editions_its_argument_names() {
    let store = holding(SHOP, CLEAN);
    let answer = run(
        &store,
        INFO,
        &json!([{ "uri": SHOP, "editions": ["bedrock"] }]),
    )
    .expect("a clean source is reported");
    assert_eq!(answer, printed(CLEAN, &["bedrock"]));
    assert_ne!(answer, printed(CLEAN, &["java", "bedrock"]));
}

#[test]
fn info_answers_a_refused_source_with_the_failure_document() {
    let store = holding(SHOP, DUPLICATE);
    let answer =
        run(&store, INFO, &json!([{ "uri": SHOP }])).expect("a refusal is still an answer");
    assert_eq!(answer, printed(DUPLICATE, &["java", "bedrock"]));
    let codes: Vec<&str> = answer["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert_eq!(
        codes,
        [
            "E_DUPLICATE_SIZE",
            "E_MISSING_MATERIAL",
            "E_DUPLICATE_ARG",
            "E_DUPLICATE_ID",
            "E_INVALID_MEMBER",
            "E_INVALID_MEMBER",
        ],
    );
}

#[test]
fn info_carries_an_edition_refusal_in_the_document() {
    // Bedrock's variant leaves `floating_text` unbound, which only the per-edition walk sees.
    let source = "theme shop_java:\n  slot wall -> @cobblestone\n  slot floating_text -> \
                  @sign.oak\n\ntheme shop_bedrock:\n  slot wall -> @cobblestone\n\nstruct shop \
                  size=5x5\n  walls class=outer mat_slot=wall height=3\n  window class=display \
                  side=front offset=2 y=2 size=1x1 mat_slot=floating_text\n";
    let store = holding(SHOP, source);
    let answer =
        run(&store, INFO, &json!([{ "uri": SHOP }])).expect("a refusal is still an answer");
    assert_eq!(answer, printed(source, &["java", "bedrock"]));
    let codes: Vec<&str> = answer["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert_eq!(codes, ["E_UNRESOLVED_SLOT"]);
}

#[test]
fn each_refusal_says_what_the_command_takes() {
    let store = holding(SHOP, CLEAN);
    let usage = "`cairn.info` takes one argument, `{ \"uri\": <document>, \"editions\"?: [...] }`";
    assert_eq!(
        run(&store, "cairn.lower", &json!([])),
        invalid("unknown command `cairn.lower`; this server runs `cairn.info`, `cairn.diffBlocks`"),
    );
    assert_eq!(
        run(&store, INFO, &json!([])),
        invalid(&format!("{usage}, and got 0"))
    );
    assert_eq!(
        run(&store, INFO, &json!([{ "uri": SHOP }, {}])),
        invalid(&format!("{usage}, and got 2")),
    );
    assert_eq!(
        run(&store, INFO, &json!([{ "uri": SHOP, "lock": "shop.lock" }])),
        invalid(&format!(
            "{usage}: unknown field `lock`, expected `uri` or `editions`"
        )),
    );
    assert_eq!(
        run(
            &store,
            INFO,
            &json!([{ "uri": SHOP, "editions": ["java", ""] }])
        ),
        invalid("`cairn.info` `editions`: --editions value must not contain empty entries"),
    );
    assert_eq!(
        run(
            &store,
            INFO,
            &json!([{ "uri": "file:///no/such/dir/other.crn" }])
        ),
        invalid("document not open: file:///no/such/dir/other.crn"),
    );
    assert_eq!(
        execute_command(&store, json!({ "arguments": [] })),
        invalid("malformed `workspace/executeCommand` params: missing field `command`"),
    );
}

#[test]
fn info_needs_a_local_file_to_find_the_lock_beside() {
    let store = holding("untitled:Untitled-1", CLEAN);
    assert_eq!(
        run(&store, INFO, &json!([{ "uri": "untitled:Untitled-1" }])),
        invalid(
            "`cairn.info` reads the lock beside the source, so it needs a local `file:` URI, and \
             got untitled:Untitled-1"
        ),
    );
}

#[test]
fn a_file_uri_decodes_to_the_path_it_names() {
    let native =
        |unix: &str, windows: &str| Some(PathBuf::from(if cfg!(windows) { windows } else { unix }));
    assert_eq!(
        file_path(&uri("file:///site/my%20shop.crn")),
        native("/site/my shop.crn", r"\site\my shop.crn"),
    );
    assert_eq!(
        file_path(&uri("file:///C:/site/shop.crn")),
        native("/C:/site/shop.crn", r"C:\site\shop.crn"),
    );
    assert_eq!(
        file_path(&uri("file:///c%3A/site/shop.crn")),
        native("/c:/site/shop.crn", r"c:\site\shop.crn"),
    );
    assert_eq!(
        file_path(&uri("FILE://localhost/site/shop.crn")),
        native("/site/shop.crn", r"\site\shop.crn"),
    );
    assert_eq!(file_path(&uri("file://server/share/shop.crn")), None);
    assert_eq!(file_path(&uri("https://example.com/shop.crn")), None);
    assert_eq!(file_path(&uri("file:shop.crn")), None);
    assert_eq!(file_path(&uri("file:///site/%FF.crn")), None);
}

/// `CLEAN` with its walls a row short.
const SHORT_WALLS: &str = "\
theme t:
  slot wall -> @stone

struct s size=2x2
  floor mat_slot=wall
  walls height=3 mat_slot=wall
";

/// A directory holding `name`, the `.nbt` that `source` compiles to at Java 1.21.4.
fn structure_file(source: &str, name: &str) -> (tempfile::TempDir, PathBuf) {
    let module = parse(source).expect("the source parses");
    let target = resolve_java_target("1.21.4").expect("a shipped target");
    let pinned = BuildTarget {
        mc_version: &target.mc_version,
        order_key: i64::from(target.data_version),
    };
    let (_, build) = lower_for_build(&module, Edition::Java, Some(pinned));
    let array = build
        .block_ir
        .structures
        .values()
        .next()
        .expect("one struct");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join(name);
    let mut file = std::fs::File::create(&path).expect("the structure file is created");
    write_structure_gzip(&mut file, array, &target).expect("the structure file is written");
    (dir, path)
}

/// The `file:` URI naming `path`.
fn file_uri(path: &Path) -> String {
    let path = path.to_str().expect("a UTF-8 path").replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

/// What `cairn diff-blocks --format json` prints for `source` against the structure at `path`.
fn printed_diff(source: &str, path: &Path) -> serde_json::Value {
    let decoded = read_structure(path).expect("the structure decodes");
    let run = diff_blocks(source, &decoded);
    match &run.outcome {
        Ok(document) => serde_json::to_value(document),
        Err(_) => serde_json::to_value(run.failure_document(source)),
    }
    .expect("the document serialises")
}

#[test]
fn diff_blocks_answers_with_the_report_cairn_diff_blocks_prints() {
    let (_dir, path) = structure_file(CLEAN, "s.nbt");
    let store = holding(SHOP, CLEAN);
    let answer = run(
        &store,
        DIFF_BLOCKS,
        &json!([{ "uri": SHOP, "structure": file_uri(&path) }]),
    )
    .expect("a lift is compared");
    assert_eq!(answer, printed_diff(CLEAN, &path));
    assert_eq!(
        [
            &answer["structure"],
            &answer["target"],
            &answer["converged"],
            &answer["block_iou"],
            &answer["regions"],
        ],
        [
            &json!("s.nbt"),
            &json!({ "edition": "java", "version": "1.21.4" }),
            &json!(true),
            &json!(1.0),
            &json!([]),
        ],
    );
}

#[test]
fn diff_blocks_compares_the_text_the_editor_holds() {
    let (_dir, path) = structure_file(CLEAN, "s.nbt");
    let store = holding("untitled:Untitled-1", SHORT_WALLS);
    let answer = run(
        &store,
        DIFF_BLOCKS,
        &json!([{ "uri": "untitled:Untitled-1", "structure": file_uri(&path) }]),
    )
    .expect("a lift is compared");
    assert_eq!(answer, printed_diff(SHORT_WALLS, &path));
    assert_eq!(
        [
            &answer["converged"],
            &answer["code"],
            &answer["patch"]["edits"]
        ],
        [
            &json!(false),
            &json!("E_DECOMPILE_DIFF"),
            &json!(["edit walls[0] set height=4"]),
        ],
    );
}

#[test]
fn diff_blocks_answers_a_refused_source_with_the_failure_document() {
    let (_dir, path) = structure_file(CLEAN, "s.nbt");
    let store = holding(SHOP, DUPLICATE);
    let answer = run(
        &store,
        DIFF_BLOCKS,
        &json!([{ "uri": SHOP, "structure": file_uri(&path) }]),
    )
    .expect("a refusal is still an answer");
    assert_eq!(answer, printed_diff(DUPLICATE, &path));
    let codes: Vec<&str> = answer["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert_eq!(
        codes,
        [
            "E_DUPLICATE_SIZE",
            "E_MISSING_MATERIAL",
            "E_DUPLICATE_ARG",
            "E_DUPLICATE_ID",
            "E_INVALID_MEMBER",
            "E_INVALID_MEMBER",
        ],
    );
}

#[test]
fn diff_blocks_refuses_a_run_the_cli_refuses_whole_with_an_empty_document() {
    let (_dir, path) = structure_file(CLEAN, "s.nbt");
    let source = format!("@requires version>=26.3\n{CLEAN}");
    let store = holding(SHOP, &source);
    let answer = run(
        &store,
        DIFF_BLOCKS,
        &json!([{ "uri": SHOP, "structure": file_uri(&path) }]),
    )
    .expect("a refusal is still an answer");
    assert_eq!(answer, json!({ "diagnostics": [] }));
}

#[test]
fn each_diff_blocks_refusal_says_what_the_command_takes() {
    let (dir, path) = structure_file(CLEAN, "s.nbt");
    let structure = file_uri(&path);
    let store = holding(SHOP, CLEAN);
    let usage = "`cairn.diffBlocks` takes one argument, `{ \"uri\": <document>, \"structure\": \
                 <file> }`";
    assert_eq!(
        run(&store, DIFF_BLOCKS, &json!([])),
        invalid(&format!("{usage}, and got 0")),
    );
    assert_eq!(
        run(&store, DIFF_BLOCKS, &json!([{ "uri": SHOP }])),
        invalid(&format!("{usage}: missing field `structure`")),
    );
    assert_eq!(
        run(
            &store,
            DIFF_BLOCKS,
            &json!([{ "uri": SHOP, "structure": structure, "format": "text" }]),
        ),
        invalid(&format!(
            "{usage}: unknown field `format`, expected `uri` or `structure`"
        )),
    );
    assert_eq!(
        run(
            &store,
            DIFF_BLOCKS,
            &json!([{ "uri": "file:///no/such/dir/other.crn", "structure": structure }]),
        ),
        invalid("document not open: file:///no/such/dir/other.crn"),
    );
    assert_eq!(
        run(
            &store,
            DIFF_BLOCKS,
            &json!([{ "uri": SHOP, "structure": "https://example.com/s.nbt" }]),
        ),
        invalid(
            "`cairn.diffBlocks` reads the structure file from disk, so `structure` needs a local \
             `file:` URI, and got https://example.com/s.nbt"
        ),
    );
    let refused = |path: &Path| {
        run(
            &store,
            DIFF_BLOCKS,
            &json!([{ "uri": SHOP, "structure": file_uri(path) }]),
        )
    };
    let schematic = dir.path().join("s.schematic");
    assert_eq!(
        refused(&schematic),
        invalid(&format!(
            "`cairn.diffBlocks` `structure`: `{}` is not a structure file `cairn diff-blocks` \
             reads; name a `.nbt`, `.litematic`, `.schem` or `.mcstructure` file",
            schematic.display()
        )),
    );
    let missing = dir.path().join("missing.nbt");
    let Err(CommandRefused { code, message }) = refused(&missing) else {
        panic!("a missing structure file is refused");
    };
    let prefix = format!(
        "`cairn.diffBlocks` `structure`: cannot read `{}`: ",
        missing.display()
    );
    assert_eq!(
        (code, message.starts_with(&prefix)),
        (ErrorCode::InvalidParams as i32, true),
        "{message}"
    );
    let truncated = dir.path().join("truncated.nbt");
    let bytes = std::fs::read(&path).expect("the structure file reads");
    std::fs::write(&truncated, &bytes[..bytes.len() / 2]).expect("the cut file is written");
    assert_eq!(
        refused(&truncated),
        invalid(&format!(
            "`cairn.diffBlocks` `structure`: cannot compare `{}`: nbt I/O: incomplete deflate stream",
            truncated.display()
        )),
    );
}
