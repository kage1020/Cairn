//! The playground's TypeScript golden, held to what `compile` answers for the sources it names.

use cairn_lang_core::Edition;
use cairn_lang_wasm::report::compile;

use crate::common::BUILD;

/// `website/src/playground/golden.ts`, whose array `satisfies` the playground's types.
const GOLDEN: &str = include_str!("../../../../website/src/playground/golden.ts");

/// Paints `@stonebrick`, which Bedrock declares last at 1.21.0, under a floor above it.
const STONEBRICK: &str = "@requires bedrock version>=1.21.40\ntheme t:\n  slot wall -> @stonebrick\n\n\
                          struct s size=2x2\n  floor mat_slot=wall\n";

/// A stair whose `shape` Bedrock 1.21.60 cannot carry.
const OUTER_STAIR: &str = "theme t:\n  slot step -> @oak_stairs[facing=south,shape=outer_left]\n\
                           struct s size=1x1\n  floor mat_slot=step\n";

#[test]
fn the_playgrounds_golden_is_what_compile_answers() {
    let floor = format!("@requires version>=1.21\n{BUILD}");
    let answered = serde_json::to_value([
        compile(&floor, Edition::Java, "1.20.4"),
        compile(STONEBRICK, Edition::Bedrock, "1.21.0"),
        compile(OUTER_STAIR, Edition::Bedrock, "1.21.60"),
    ])
    .expect("serialises");
    let start = GOLDEN
        .find("export const golden = ")
        .map(|at| at + "export const golden = ".len())
        .expect("golden.ts declares `golden`");
    let end = GOLDEN
        .rfind(" satisfies ")
        .expect("golden.ts holds `golden` to a type");
    let golden: serde_json::Value =
        serde_json::from_str(&GOLDEN[start..end]).expect("`golden` is written as JSON");
    assert!(
        golden == answered,
        "website/src/playground/golden.ts is stale; its array is now:\n{}",
        serde_json::to_string_pretty(&answered).expect("serialises"),
    );
}
