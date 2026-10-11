use cairn_lang_core::resolve::DroppedIntent;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{lower, parse};
use cairn_lang_formats::registry::builtin_bedrock;

use super::*;

/// The whole note block, header included.
///
/// Nothing else reads these lines: the end-to-end tests look for an id
/// and a clause inside them, which leaves the header, the guard, the
/// order and the indent free to change or vanish. The header is the
/// part that matters most — it names the figure on stdout that the
/// lines below explain, and one that disagreed with the row would be
/// worse than no header at all.
#[test]
fn the_notes_answer_the_figure_they_sit_under() {
    let entries = vec![
        UnsupportedEntry {
            id: "minecraft:oak_sign".to_owned(),
            reason: UnsupportedReason::AbsentFromEdition {
                suggestion: Some("minecraft:oak_log".to_owned()),
                aliases: Vec::new(),
            },
        },
        UnsupportedEntry {
            id: "minecraft:oak_door".to_owned(),
            reason: UnsupportedReason::StatesUnmapped {
                states: "facing=north".to_owned(),
                mapped: "the stair family".to_owned(),
            },
        },
    ];
    assert_eq!(
        unsupported_notes(Edition::Bedrock, &entries),
        [
            "note: what `unsupported: 2` counts on bedrock:".to_owned(),
            format!(
                "  note: `minecraft:oak_sign` — {}",
                unsupported_reason(&entries[0].reason)
            ),
            format!(
                "  note: `minecraft:oak_door` — {}",
                unsupported_reason(&entries[1].reason)
            ),
        ],
    );
    // A clean source says nothing rather than announcing a count of
    // nothing, and every edition would otherwise get a block of its
    // own back to back.
    assert!(unsupported_notes(Edition::Java, &[]).is_empty());
}

/// The same, for the figure beside it.
///
/// `degraded_notes` has the same header, the same guard and the same
/// indent, and the end-to-end tests read it the same circumstantial
/// way — so without this, dropping the empty guard would print
/// ``note: what `degraded: 0` counts on java:`` on every clean run,
/// twice over for `--editions java,bedrock`, and the suite would stay
/// green.
#[test]
fn the_degraded_notes_answer_the_figure_they_sit_under() {
    let entries = vec![
        DegradedEntry {
            id: "minecraft:spruce_stairs".to_owned(),
            states: "facing=north,shape=outer_left".to_owned(),
            dropped: vec![DroppedIntent::Shape {
                value: "outer_left".to_owned(),
            }],
        },
        // The same id again: it is the state combination that
        // degrades, so the block reports it once per combination.
        DegradedEntry {
            id: "minecraft:spruce_stairs".to_owned(),
            states: "facing=south,shape=inner_right".to_owned(),
            dropped: vec![DroppedIntent::Shape {
                value: "inner_right".to_owned(),
            }],
        },
    ];
    assert_eq!(
        degraded_notes(Edition::Bedrock, &entries),
        [
            "note: what `degraded: 2` counts on bedrock:".to_owned(),
            format!(
                "  note: `minecraft:spruce_stairs[facing=north,shape=outer_left]` — {}",
                degradation_detail(&entries[0].dropped[0])
            ),
            format!(
                "  note: `minecraft:spruce_stairs[facing=south,shape=inner_right]` — {}",
                degradation_detail(&entries[1].dropped[0])
            ),
        ],
    );
    assert!(degraded_notes(Edition::Java, &[]).is_empty());
}

/// The block that stands in for a row an edition does not get.
///
/// Built through `portability_for_bedrock` rather than from a
/// hand-made error, so the test also holds the premise it rests on:
/// that a leaked blockstate refuses the report instead of landing in
/// the `unsupported` figure beside the ordinary answers.
///
/// The entry is interned into a real lowering rather than written in a
/// source, so the test asks about the leak a pack or the compiler
/// could make, which has no source. `roof::stair_state` builds stair
/// properties from `Cardinal` and `StairShape`, so its values are in
/// domain by construction, and a registry pack answers
/// `PackView::lookup` with `BlockState::bare`. An authored state
/// literal on a stair is the one way a source reaches the same report;
/// the CLI contract tests drive that one.
#[test]
fn a_refused_palette_says_which_edition_lost_its_row_and_whose_bug_it_is() {
    use cairn_lang_core::block_array::BlockState;

    let module = parse("struct probe size=3x3\n  walls height=2\n").expect("the probe parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(Edition::Bedrock));
    let pack = builtin_bedrock();
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&pack.view(None)));
    let mut leaked = BlockState::bare("minecraft:oak_stairs");
    leaked
        .properties
        .insert("facing".to_owned(), "up".to_owned());
    block_ir
        .structures
        .values_mut()
        .next()
        .expect("the probe lowers to one structure")
        .palette
        .intern(leaked);

    let invalid = portability_for_bedrock(&block_ir, &pack.blocks, &pack.aliases)
        .expect_err("a value outside the Java domain is not a portability figure");

    let lines = invalid_palette_report(Edition::Bedrock, &invalid);
    // What the walk does with it: no figure, so no row, so
    // `edition_specific_error` and a refusal carrying no finding —
    // `{"diagnostics": []}` and exit 1 under `--format json`. This is
    // the only place that decision can be asked, and it is stated as a
    // promise in `spec/lint` "Machine-readable payload".
    assert!(
        portability_figure(
            Edition::Bedrock,
            portability_for_bedrock(&block_ir, &pack.blocks, &pack.aliases),
        )
        .is_none(),
        "a refused palette costs the edition its row rather than printing counts over it",
    );
    assert_eq!(lines.len(), 3, "header, one leak, closing note: {lines:?}");
    assert!(
        lines[0].starts_with("error: the bedrock ") && lines[0].contains("no portability"),
        "the header names the edition and what it is not getting, got: {}",
        lines[0],
    );
    // The translator's own sentence, so the same leak reads the same
    // from `info` as from the build that would refuse the same entry.
    assert_eq!(
        lines[1],
        format!("  error: {}", invalid.leaks()[0]),
        "the leak is quoted, not reworded",
    );
    assert!(
        lines[2].contains("a state literal on a stair in the source")
            && lines[2].contains("a key other than `facing` / `half` / `shape`")
            && lines[2].contains("a leak in the pack or this compiler"),
        "the block names both places such a blockstate can come from, since the leak's own \
         `Fix:` addresses only the first, got: {}",
        lines[2],
    );
}

/// Each `unsupported` reason renders the repair it names.
///
/// The rendering is a pure function of the reason, so every variant is
/// asked here in one place. `roof::stair_state` builds stair properties
/// only for a material the family check already accepted, and a
/// registry pack answers `PackView::lookup` with `BlockState::bare`,
/// so the states branch is reached from a source only through an
/// authored state literal on a block outside the stair family
/// (`@oak_log[axis=x]`); the CLI contract tests drive that one end to
/// end. A literal on a stair never lands here: a `facing` or `half`
/// value outside the Java domain, or a key other than `facing` /
/// `half` / `shape`, refuses the edition's row instead, which
/// `a_refused_palette_says_which_edition_lost_its_row_and_whose_bug_it_is`
/// asks about.
#[test]
fn every_unsupported_reason_renders_the_repair_it_names() {
    let bare = unsupported_reason(&UnsupportedReason::AbsentFromEdition {
        suggestion: None,
        aliases: Vec::new(),
    });
    assert!(
        bare.contains("declares the block") && !bare.contains("did you mean"),
        "no suggestion means no dangling clause, got: {bare}",
    );
    let suggested = unsupported_reason(&UnsupportedReason::AbsentFromEdition {
        suggestion: Some("minecraft:oak_slab".to_owned()),
        aliases: Vec::new(),
    });
    assert!(
        suggested.contains("did you mean `minecraft:oak_slab`?"),
        "got: {suggested}",
    );
    // An edition that has the block under another name is not missing
    // it, and the row says so instead of offering a typo guess beside
    // a spelling the pack knows to be the answer.
    let renamed = unsupported_reason(&UnsupportedReason::AbsentFromEdition {
        suggestion: Some("minecraft:oak_slab".to_owned()),
        aliases: vec!["minecraft:standing_sign".to_owned()],
    });
    assert!(
        renamed.contains("it spells the block `minecraft:standing_sign`")
            && !renamed.contains("did you mean"),
        "got: {renamed}",
    );
    // The block is not missing from the edition and the edition is not
    // what cannot express the states — this compiler is, so far.
    let unmapped = unsupported_reason(&UnsupportedReason::StatesUnmapped {
        states: "facing=north".to_owned(),
        mapped: "the stair family".to_owned(),
    });
    assert!(
        unmapped.contains("facing=north")
            && unmapped.contains("has the block")
            && unmapped.contains("the stair family")
            && unmapped.contains("so far"),
        "the gap is this compiler's and it is not permanent, got: {unmapped}",
    );
}
