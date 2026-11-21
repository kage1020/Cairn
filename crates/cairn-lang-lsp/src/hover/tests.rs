use std::collections::BTreeSet;

use cairn_lang_core::ast::ItemKind;
use cairn_lang_core::intent::{UNIVERSAL_ARGUMENTS, known_keywords, role_of};

use super::*;

const KEEP: &str = "\
theme stone:
  slot floor -> @floor.stone.smooth
  slot wall -> @cherry_planks
  slot trim -> @stonebrick
  walls[class=outer] -> trim=@spruce_log

struct keep size=5x5
  floor mat_slot=floor
  walls id=shell class=outer mat_slot=wall height=3
  door id=entry side=front at=center hinge=left
  window side=back y=2 offset=1 size=1x1 mat_slot=wall
  roof kind=gable
";

const VILLAGE: &str = "\
theme oak:
  slot wall -> @oak_planks
theme birch:
  slot wall -> @birch_planks

def hut size=3x3:
  walls mat_slot=wall height=3
  door id=entry side=front at=center

site village:
  place id=a use=hut theme=oak at=origin
  place id=b use=hut theme=birch east_of=a gap=2
  connect a.entry to b.entry path=@gravel

struct lamp size=3x3
  @edition bedrock
    raw_block mat=minecraft:stone at=1,0,1
";

/// The Markdown and range of the hover at the first byte of `needle`'s `nth` occurrence.
fn shown_at(source: &str, needle: &str, nth: usize) -> Option<(String, lsp_types::Range)> {
    let (offset, _) = source
        .match_indices(needle)
        .nth(nth)
        .expect("needle in source");
    shown_at_offset(source, offset)
}

/// The hover `past` bytes into `needle`'s first occurrence.
fn shown_inside(source: &str, needle: &str, past: usize) -> Option<(String, lsp_types::Range)> {
    let offset = source.find(needle).expect("needle in source");
    assert!(past < needle.len(), "{past} bytes reach past `{needle}`");
    shown_at_offset(source, offset + past)
}

fn shown_at_offset(source: &str, offset: usize) -> Option<(String, lsp_types::Range)> {
    let index = LineIndex::new(source);
    let found = hover(source, index.position(source, offset))?;
    let lsp_types::HoverContents::Markup(markup) = found.contents else {
        panic!("hover answers Markdown, got {:?}", found.contents);
    };
    Some((markup.value, found.range.expect("hover names its range")))
}

fn shown(source: &str, needle: &str) -> Option<(String, lsp_types::Range)> {
    shown_at(source, needle, 0)
}

fn range(line: u32, from: u32, to: u32) -> lsp_types::Range {
    lsp_types::Range {
        start: lsp_types::Position::new(line, from),
        end: lsp_types::Position::new(line, to),
    }
}

/// The page's top-level entry for `name`, as a hover quotes it.
fn quoted(name: &str) -> String {
    own_text(entry(name, None).unwrap_or_else(|| panic!("an entry for `{name}`")))
}

/// A `resolved_state` line for each edition, each saying `cairn lower` refuses the document for
/// `reason`.
fn refused(reason: &str) -> String {
    format!(
        "**`resolved_state`** on java: `cairn lower --edition java` refuses this document \
         {reason}\n\n\
         **`resolved_state`** on bedrock: `cairn lower --edition bedrock` refuses this document \
         {reason}"
    )
}

#[test]
fn an_entry_is_quoted_without_the_entries_under_it_and_with_links_that_reach_the_site() {
    let entry = "- **`roof`.** A roof, in the shape its `kind=` names. See [Compilation\n  \
                 Model](/spec/compilation/#43-gable-roof-voxel-rules) and [nesting](#52-nesting).\n  \
                 - **`kind=`.** Its shape.\n";
    assert_eq!(
        own_text(entry),
        "**`roof`.** A roof, in the shape its `kind=` names. See [Compilation \
         Model](https://cairn.kage1020.com/spec/compilation/#43-gable-roof-voxel-rules) and \
         [nesting](https://cairn.kage1020.com/spec/syntax/#52-nesting).",
    );
}

#[test]
fn an_item_keyword_shows_its_entry() {
    assert_eq!(
        shown(KEEP, "struct"),
        Some((quoted("struct"), range(6, 0, 6)))
    );
    assert_eq!(
        shown(KEEP, "theme"),
        Some((quoted("theme"), range(0, 0, 5)))
    );
}

#[test]
fn a_member_keyword_shows_its_entry_over_its_id_intent_state_and_resolved_state() {
    let state = "`id=entry`\n\n\
                 **`intent_state`**: `hinge=left`\n\n\
                 **`resolved_state`** on java, with no version pinned:\n\
                 - `struct::keep`: \
                 `minecraft:oak_door[facing=north,half=lower,hinge=left,open=false]` in 1 cell, \
                 `minecraft:oak_door[facing=north,half=upper,hinge=left,open=false]` in 1 cell\n\n\
                 **`resolved_state`** on bedrock, with no version pinned:\n\
                 - `struct::keep`: \
                 `minecraft:wooden_door[facing=north,half=lower,hinge=left,open=false]` in 1 cell, \
                 `minecraft:wooden_door[facing=north,half=upper,hinge=left,open=false]` in 1 cell";
    let door = format!("{}\n\n---\n\n{state}", quoted("door"));
    assert_eq!(shown(KEEP, "door"), Some((door.clone(), range(9, 2, 6))));
    assert_eq!(
        shown(KEEP, "entry"),
        Some((door, range(9, 2, 47))),
        "anywhere on the line a key or a token does not cover",
    );
}

#[test]
fn a_member_that_writes_no_blockstate_says_so() {
    assert_eq!(
        shown(KEEP, "walls id"),
        Some((
            format!(
                "{}\n\n---\n\n`id=shell`\n\n\
                 **`intent_state`**: nothing written\n\n\
                 **`resolved_state`** on java, with no version pinned:\n\
                 - `struct::keep`: `minecraft:cherry_planks` in 45 cells\n\n\
                 **`resolved_state`** on bedrock, with no version pinned:\n\
                 - `struct::keep`: `minecraft:cherry_planks` in 45 cells",
                quoted("walls"),
            ),
            range(8, 2, 7),
        )),
    );
}

#[test]
fn a_key_shows_the_entry_its_own_keyword_gives_it_above_that_keywords_entry() {
    assert_eq!(
        shown(KEEP, "side=front"),
        Some((
            format!(
                "**`side=`.** The wall it is cut in: `front`, `back`, `left` or `right`.\n\n{}",
                quoted("door"),
            ),
            range(9, 16, 20),
        )),
    );
    assert_eq!(
        shown(KEEP, "y=2"),
        Some((
            format!(
                "**`y=`.** The row of the cell its `anchor=` names, counted from the floor: its \
                 lowest row without one.\n\n{}",
                quoted("window"),
            ),
            range(10, 19, 20),
        )),
    );
    assert_eq!(
        shown(KEEP, "kind=gable"),
        Some((
            format!(
                "**`kind=`.** Its shape: `gable`, `shed`, `hip` or `flat`.\n\n{}",
                quoted("roof"),
            ),
            range(11, 7, 11),
        )),
        "a key an earlier keyword's entry holds too",
    );
}

#[test]
fn a_key_its_own_keyword_holds_no_entry_for_shows_nothing_though_another_keyword_does() {
    let source = "struct s size=3x3\n  door side=front y=1 kind=gable\n";
    assert_eq!(shown(source, "y="), None, "`window` and others hold a `y=`");
    assert_eq!(
        shown(source, "kind="),
        None,
        "`fill` and `roof` hold a `kind=`"
    );
}

#[test]
fn a_key_every_member_accepts_shows_the_one_entry_they_share() {
    assert_eq!(
        shown(KEEP, "id=entry"),
        Some((quoted("id="), range(9, 7, 9)))
    );
    assert_eq!(
        shown(KEEP, "id=shell"),
        Some((quoted("id="), range(8, 8, 10)))
    );
}

#[test]
fn a_material_token_shows_the_block_each_edition_names_and_what_binds_it() {
    assert_eq!(
        shown(KEEP, "@floor"),
        Some((
            "`@floor.stone.smooth`, bound to slot `floor` by theme `stone`\n\n\
             - java: `minecraft:stone_bricks`\n\
             - bedrock: `minecraft:stone_bricks`, `minecraft:stonebrick` at 1.21.0"
                .to_owned(),
            range(1, 16, 35),
        )),
        "an abstract token, with the version its catalog overrides it at",
    );
    assert_eq!(
        shown(KEEP, "@cherry").map(|(text, _)| text),
        Some(
            "`@cherry_planks`, bound to slot `wall` by theme `stone`\n\n\
             - java: `minecraft:cherry_planks`\n\
             - bedrock: `minecraft:cherry_planks`"
                .to_owned()
        ),
        "a canonical token every version declares",
    );
    assert_eq!(
        shown(KEEP, "@stonebrick").map(|(text, _)| text),
        Some(
            "`@stonebrick`, bound to slot `trim` by theme `stone`\n\n\
             - java: `minecraft:stonebrick`, which no java version declares\n\
             - bedrock: `minecraft:stonebrick`, declared by 1.21.0"
                .to_owned()
        ),
        "a canonical token only some versions declare",
    );
    assert_eq!(
        shown(VILLAGE, "@gravel"),
        Some((
            "`@gravel`, the `path=` of this `connect` row\n\n\
             - java: `minecraft:gravel`\n\
             - bedrock: `minecraft:gravel`"
                .to_owned(),
            range(12, 34, 41),
        )),
    );
}

const TOKENS: &str = "\
theme t:
  slot stairs -> @oak_stairs[facing=east]
  slot floor -> @floor.nonexistent.thing
  window[class=\"a->b\"] -> trim=@spruce_log
";

#[test]
fn a_canonical_token_keeps_its_state_literal_and_an_unknown_abstract_token_says_so() {
    assert_eq!(
        shown(TOKENS, "@oak_stairs"),
        Some((
            "`@oak_stairs[facing=east]`, bound to slot `stairs` by theme `t`\n\n\
             - java: `minecraft:oak_stairs[facing=east]`\n\
             - bedrock: `minecraft:oak_stairs[facing=east]`"
                .to_owned(),
            range(1, 17, 41),
        )),
    );
    assert_eq!(
        shown(TOKENS, "@floor"),
        Some((
            "`@floor.nonexistent.thing`, bound to slot `floor` by theme `t`\n\n\
             - java: the materials catalog has no `@floor.nonexistent.thing`\n\
             - bedrock: the materials catalog has no `@floor.nonexistent.thing`"
                .to_owned(),
            range(2, 16, 40),
        )),
    );
}

/// The selector bindings' note, linking the section the site gives `spec/materials-themes` "Slots
/// as dependency injection".
fn reserved() -> String {
    let page = cairn_lang_docs::page("spec/materials-themes").expect("the chapter's page");
    let heading = page
        .headings()
        .into_iter()
        .find(|heading| heading.text.ends_with(" Slots as dependency injection"))
        .expect("a section on slots");
    format!(
        "Selector bindings are reserved, so no block is resolved for this value. See \
         [`spec/materials-themes` \"Slots as dependency injection\"]\
         (https://cairn.kage1020.com/spec/materials-themes/#{}).",
        heading.slug,
    )
}

#[test]
fn a_selector_binding_resolves_no_block_and_its_row_keyword_shows_its_entry() {
    assert_eq!(
        shown(KEEP, "@spruce_log"),
        Some((
            format!(
                "`@spruce_log`, bound to `trim=` by the `walls[class=outer]` row of theme \
                 `stone`\n\n{}",
                reserved(),
            ),
            range(4, 29, 40),
        )),
    );
    assert_eq!(
        shown(TOKENS, "@spruce_log"),
        Some((
            format!(
                "`@spruce_log`, bound to `trim=` by the `window[class=\"a->b\"]` row of theme \
                 `t`\n\n{}",
                reserved(),
            ),
            range(3, 31, 42),
        )),
        "a `->` quoted in the selector",
    );
    assert_eq!(
        shown(KEEP, "walls["),
        Some((quoted("walls"), range(4, 2, 7)))
    );
}

#[test]
fn a_def_member_shows_what_it_resolved_to_in_each_place() {
    assert_eq!(
        shown(VILLAGE, "walls").map(|(text, _)| text),
        Some(format!(
            "{}\n\n---\n\n\
             **`intent_state`**: nothing written\n\n\
             **`resolved_state`** on java, with no version pinned:\n\
             - `site::village::a`: `minecraft:oak_planks` in 22 cells\n\
             - `site::village::b`: `minecraft:birch_planks` in 22 cells\n\n\
             **`resolved_state`** on bedrock, with no version pinned:\n\
             - `site::village::a`: `minecraft:oak_planks` in 22 cells\n\
             - `site::village::b`: `minecraft:birch_planks` in 22 cells",
            quoted("walls"),
        )),
    );
}

#[test]
fn a_member_has_no_cell_on_an_edition_its_guard_leaves_out() {
    assert_eq!(
        shown(VILLAGE, "raw_block").map(|(text, _)| text),
        Some(format!(
            "{}\n\n---\n\n\
             **`intent_state`**: nothing written\n\n\
             **`resolved_state`** on java, with no version pinned: no cell\n\n\
             **`resolved_state`** on bedrock, with no version pinned:\n\
             - `struct::lamp`: `minecraft:stone` in 1 cell",
            quoted("raw_block"),
        )),
    );
}

const LANE: &str = "\
theme oak:
  slot wall -> @oak_planks

def hut size=3x3:
  walls mat_slot=wall height=3
  door id=entry side=front at=center

def shed size=3x3:
  walls mat_slot=wall height=3
  door id=entry side=left at=center

site lane:
  place id=a use=hut theme=oak at=origin
  place id=b use=shed theme=oak east_of=a gap=3
  place id=c use=hut theme=oak east_of=b gap=3
  connect a.entry to b.entry path=@gravel
  connect a.entry to b.entry path=@gravel
  connect b.entry to c.entry path=@gravel
";

#[test]
fn a_connect_row_shows_the_cells_of_the_walkway_it_lays_and_a_row_that_lays_none_no_cell() {
    let walkway = |line: &str| {
        format!(
            "{}\n\n---\n\n\
             **`intent_state`**: nothing written\n\n\
             **`resolved_state`** on java, with no version pinned:{line}\n\n\
             **`resolved_state`** on bedrock, with no version pinned:{line}",
            quoted("connect"),
        )
    };
    assert_eq!(
        shown(LANE, "connect"),
        Some((
            walkway("\n- `walkway::lane::a.entry__b.entry`: `minecraft:gravel` in 7 cells"),
            range(15, 2, 9),
        )),
        "an L-shaped walkway, its array's other cells air",
    );
    assert_eq!(
        shown_at(LANE, "connect", 1).map(|(text, _)| text),
        Some(walkway(" no cell")),
        "a row repeating the walkway an earlier row laid",
    );
    assert_eq!(
        shown_at(LANE, "connect", 2).map(|(text, _)| text),
        Some(walkway(
            "\n- `walkway::lane::b.entry__c.entry`: `minecraft:gravel` in 11 cells"
        )),
    );
}

const LEVELS: &str = "\
theme t:
  slot w -> @stone

struct s size=5x5
  floor mat_slot=w
  level y=1
    walls mat_slot=w height=2
  circuit region=logic void=2
";

#[test]
fn a_member_under_a_level_shows_its_own_resolved_state() {
    assert_eq!(
        shown(LEVELS, "walls"),
        Some((
            format!(
                "{}\n\n---\n\n\
                 **`intent_state`**: nothing written\n\n\
                 **`resolved_state`** on java, with no version pinned:\n\
                 - `struct::s`: `minecraft:stone` in 32 cells\n\n\
                 **`resolved_state`** on bedrock, with no version pinned:\n\
                 - `struct::s`: `minecraft:stone` in 32 cells",
                quoted("walls"),
            ),
            range(6, 4, 9),
        )),
    );
}

#[test]
fn a_place_level_or_circuit_row_shows_no_resolved_state_of_its_own() {
    assert_eq!(
        shown(VILLAGE, "place").map(|(text, _)| text),
        Some(format!(
            "{}\n\n---\n\n`id=a`\n\n**`intent_state`**: nothing written",
            quoted("place"),
        )),
    );
    assert_eq!(
        shown(LEVELS, "level").map(|(text, _)| text),
        Some(format!(
            "{}\n\n---\n\n**`intent_state`**: nothing written",
            quoted("level"),
        )),
    );
    assert_eq!(
        shown(LEVELS, "circuit").map(|(text, _)| text),
        Some(format!(
            "{}\n\n---\n\n**`intent_state`**: nothing written",
            quoted("circuit"),
        )),
    );
}

#[test]
fn a_document_with_errors_shows_what_makes_cairn_lower_refuse_it_in_place_of_a_resolved_state() {
    let shown_on = |body: &str, needle: &str| {
        let source = format!("theme t:\n  slot w -> @stone\n\nstruct s size=3x3\n{body}");
        shown(&source, needle).map(|(text, _)| text)
    };
    let walls = |reason: &str| {
        Some(format!(
            "{}\n\n---\n\n**`intent_state`**: nothing written\n\n{}",
            quoted("walls"),
            refused(reason),
        ))
    };
    let floor = |reason: &str| {
        Some(format!(
            "{}\n\n---\n\n**`intent_state`**: nothing written\n\n{}",
            quoted("floor"),
            refused(reason),
        ))
    };
    assert_eq!(
        shown_on("  walls mat_slot=nope height=3\n", "walls"),
        walls("for `E_UNRESOLVED_SLOT` on this line"),
    );
    assert_eq!(
        shown_on("  walls mat_slot=w height=3 height=4\n", "walls"),
        walls("for `E_DUPLICATE_ARG` on this line"),
    );
    let elsewhere = "  walls mat_slot=w height=3 bogus=1\n  floor mat_slot=w\n";
    assert_eq!(
        shown_on(elsewhere, "walls"),
        walls("for `E_UNKNOWN_ARGUMENT` on this line"),
    );
    assert_eq!(
        shown_on(elsewhere, "floor"),
        floor("for `E_UNKNOWN_ARGUMENT` on line 5"),
    );
    let many =
        "  floor mat_slot=w\n  walls mat_slot=nope height=3 height=4\n  roof kind=hip bogus=1\n";
    assert_eq!(
        shown_on(many, "walls"),
        walls("for `E_UNRESOLVED_SLOT` and `E_DUPLICATE_ARG` on this line, and 1 more error"),
    );
    assert_eq!(
        shown_on(many, "floor"),
        floor("for `E_UNRESOLVED_SLOT` on line 6, and 2 more errors"),
    );
    let lowering_first =
        "  floor mat_slot=w\n  door side=up\n  walls mat_slot=w height=3 bogus=1\n";
    assert_eq!(
        shown_on(lowering_first, "floor"),
        floor("for `E_INVALID_MEMBER` on line 6, and 1 more error"),
        "a lowering error above a check's",
    );
}

#[test]
fn a_line_an_edition_guard_refuses_shows_its_entry_and_the_refusal() {
    let source = "struct s size=3x3\n  @edition java\n    walls height=3\n";
    assert_eq!(
        shown(source, "walls"),
        Some((
            format!(
                "{}\n\n---\n\n{}",
                quoted("walls"),
                refused("for `E_SEMANTIC_IN_EDITION_GUARD` on this line"),
            ),
            range(2, 4, 9),
        )),
    );
    assert_eq!(
        shown(source, "height").map(|(text, _)| text),
        Some(format!(
            "**`height=`.** How many rows the walls rise.\n\n{}",
            quoted("walls"),
        )),
    );
}

#[test]
fn a_cursor_inside_a_word_shows_what_its_first_byte_does() {
    let inside = [
        (KEEP, "struct", 3),
        (KEEP, "walls[", 2),
        (KEEP, "door", 2),
        (KEEP, "side=front", 2),
        (KEEP, "@floor", 5),
        (KEEP, "@spruce_log", 4),
        (VILLAGE, "@gravel", 3),
    ];
    for (source, needle, past) in inside {
        let first = shown(source, needle);
        assert!(first.is_some(), "`{needle}` has a hover");
        assert_eq!(
            shown_inside(source, needle, past),
            first,
            "{past} bytes into `{needle}`"
        );
    }
}

#[test]
fn nothing_is_shown_where_no_entry_or_value_describes_what_is_there() {
    let unknown = "struct s size=3x3\n  tower x=1\n";
    let broken = "struct s size=\n  walls height=3\n";
    assert_eq!(shown(KEEP, "slot floor"), None, "a theme's `slot` keyword");
    assert_eq!(shown(KEEP, "floor ->"), None, "a slot's name");
    assert_eq!(shown(KEEP, "keep"), None, "an item's name");
    assert_eq!(shown_at(KEEP, "\n", 4), None, "a blank line");
    assert_eq!(
        shown(unknown, "tower"),
        None,
        "a keyword the reference does not hold"
    );
    assert_eq!(shown(unknown, "x="), None, "a key no member accepts");
    assert_eq!(
        shown(broken, "walls"),
        None,
        "a document that does not parse"
    );
    let past = lsp_types::Position::new(40, 0);
    assert_eq!(hover(KEEP, past), None, "a position past the end");
}

/// How many of the keyword reference's entries named `name` nest in no other entry, or in the
/// entry for the keyword `within`.
fn counted(name: &str, within: Option<&str>) -> usize {
    let within = within.map(|keyword| format!("`{keyword}`"));
    entries(name)
        .into_iter()
        .filter(|(_, owner)| *owner == within.as_deref())
        .count()
}

#[test]
fn every_keyword_and_key_has_one_entry_and_no_entry_names_anything_else() {
    let page = cairn_lang_docs::page(REFERENCE).expect("the reference's page");
    let mut expected = BTreeSet::new();
    for kind in ItemKind::ALL {
        expected.insert(format!("`{}`", kind.keyword()));
    }
    for key in UNIVERSAL_ARGUMENTS {
        expected.insert(format!("`{key}=`"));
    }
    for keyword in known_keywords() {
        expected.insert(format!("`{keyword}`"));
        entry(keyword, None).expect("an entry for every member keyword");
        let own_keys = role_of(keyword).own_keys().expect("a known keyword's keys");
        let nested: BTreeSet<&str> = page
            .titles()
            .into_iter()
            .filter_map(|title| title.strip_prefix('`')?.strip_suffix("=`"))
            .filter(|key| counted(&format!("{key}="), Some(keyword)) > 0)
            .collect();
        assert_eq!(
            nested,
            own_keys.iter().copied().collect(),
            "the keys `{keyword}`'s entry holds",
        );
        for key in own_keys {
            assert_eq!(
                counted(&format!("{key}="), Some(keyword)),
                1,
                "`{keyword} {key}=`"
            );
        }
    }
    let top: BTreeSet<String> = page
        .titles()
        .into_iter()
        .filter(|title| title.starts_with('`'))
        .filter(|title| counted(title.trim_matches('`'), None) > 0)
        .map(str::to_owned)
        .collect();
    assert_eq!(top, expected);
    for name in &expected {
        let name = name.trim_matches('`');
        assert_eq!(counted(name, None), 1, "`{name}`");
        let text = quoted(name);
        assert!(
            !text.contains("](/") && !text.contains("](#"),
            "`{name}` links outside the site: {text}",
        );
    }
}
