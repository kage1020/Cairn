use super::anchor::{PlaceAnchor, PlacementCorner, PlacementOutOfRange};
use super::canvas::{Canvas, ScopePalette, prune_unreferenced};
use super::connect::edge_pushers;
use super::members::Phase;
use super::plate::{PRESSURE_PLATE_BASE_ID, PRESSURE_PLATE_TOKEN};
use super::roof_fill::{GeometryMemberDescription, geometry_material_id};
use super::site_floor::FloorExtent;
use super::*;
use crate::ast::ValueKind;
use crate::block_array::material::resolve_block_state;
use crate::block_array::roof::is_stair;
use crate::block_array::walkway::{ROUTE_AREA_CAP, SearchRect, l_path_area};
use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockState, Dims, Footprint, Palette, PaletteFull, PaletteIndex};
use crate::diagnostic::{DiagnosticCode, DiagnosticData, Severity};
use crate::intent::ValueWithSpan;

/// [`PlaceAnchor::origin`] measures the far edge as `origin + dims − 1`,
/// which is one cell before the origin when an extent is 0. No body has
/// one, since `size=` is a `NonZeroU32`, so the only way to show that
/// the check fails loud rather than measuring from the wrong cell is to
/// hand it one. Debug-only because that is where `debug_assert!` lives.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "a placed body has a zero extent")]
fn a_zero_extent_is_not_measured_for_its_far_edge() {
    let _ = PlaceAnchor::WorldOrigin.origin(Dims { x: 0, y: 1, z: 1 });
}

#[test]
fn a_placement_whose_far_edge_leaves_i32_is_refused_on_either_axis() {
    // No source reaches the `z` case: `gap=-1` is `E_PARSE`, and with a
    // `gap=` of 0 or more no origin's `z` is above the `0` of
    // `at=origin` (`east_of` keeps the prior's `z`, `north_of` moves back
    // from it). So the anchor is checked directly, with a negative `gap`.
    let dims = Dims { x: 5, y: 1, z: 5 };
    let north = |prior_origin, gap| PlaceAnchor::NorthOf { prior_origin, gap };
    // In range: the last cell on each axis is `i32::MAX`.
    assert_eq!(
        north((i32::MAX - 4, 0, i32::MAX), -1).origin(dims),
        Ok((i32::MAX - 4, 0, i32::MAX - 4)),
    );
    // One wider on `x`: a `north_of` row keeps the prior's `x`.
    assert_eq!(
        north((i32::MAX - 3, 0, 0), 0).origin(dims),
        Err(PlacementOutOfRange {
            corner: PlacementCorner::FarEdge,
            axis: 'x',
            coord: i128::from(i32::MAX) + 1,
        }),
    );
    let z_past = north((0, 0, i32::MAX), -2).origin(dims);
    assert_eq!(
        z_past,
        Err(PlacementOutOfRange {
            corner: PlacementCorner::FarEdge,
            axis: 'z',
            coord: i128::from(i32::MAX) + 1,
        }),
    );
    // No source renders the sentence with `z` in it, so it is rendered
    // here.
    assert_eq!(
        z_past.map_err(PlacementOutOfRange::deferral),
        Err(
            "this placement's body reaches z=2147483648, past the -2147483648 to \
             2147483647 range a placement's cells are addressed in; shrink the body with \
             its `def`'s `size=` or a roof's `overhang=`, or shorten the `gap=` on this row \
             or on a row it is placed relative to"
                .to_owned()
        ),
    );
}

/// One placed row as `lower_site` leaves it in `structures` and
/// `placed`: a body `width` cells wide and one cell deep and tall, every
/// cell stone, at `origin`.
fn one_placed_row(
    origin: (i32, i32, i32),
    width: u32,
) -> (IndexMap<String, BlockArray>, IndexMap<String, PlacedBody>) {
    let key = "site::s::b".to_owned();
    let dims = Dims {
        x: width,
        y: 1,
        z: 1,
    };
    let mut palette = Palette::new_with_air();
    let stone = palette.intern(BlockState::bare("minecraft:stone"));
    let mut structures = IndexMap::new();
    structures.insert(
        key.clone(),
        BlockArray {
            dims,
            palette,
            voxels: vec![stone; width as usize],
            block_entities: Vec::new(),
            entities: Vec::new(),
            source_scope: key.clone(),
        },
    );
    let mut placed = IndexMap::new();
    placed.insert(
        key,
        PlacedBody {
            placement: Placement {
                site: SiteName::new("s").expect("a site name"),
                place_id: PlaceId::new("b").expect("a place id"),
                source_def: "hut".to_owned(),
                theme: "t".to_owned(),
                origin,
                dims,
            },
            walls: WallColumn::default(),
            cut: HashSet::new(),
        },
    );
    (structures, placed)
}

/// A body whose last column is `i32::MAX` lays each of its cells under
/// a column of its own, and its extent ends on the edge.
#[test]
fn a_body_ending_on_the_i32_edge_lays_every_cell_once() {
    let (structures, placed) = one_placed_row((i32::MAX - 2, 0, 0), 3);
    let plan = collect_floor_cells(&structures, &placed);
    assert_eq!(
        plan.cells,
        HashSet::from([(i32::MAX - 2, 0, 0), (i32::MAX - 1, 0, 0), (i32::MAX, 0, 0)]),
    );
    assert!(
        plan.owners.values().all(|owners| owners.len() == 1),
        "{:?}",
        plan.owners,
    );
    assert_eq!(plan.extents["site::s::b"].max_x, i32::MAX);
}

/// The guard behind the far-edge refusal, for a body that runs past
/// `i32` and reaches the floor plan anyway. No source can: the refusal
/// stops the row first, so the plan is built by hand. A debug build
/// stops at the guard. A release build lays the cells that have a
/// coordinate and folds none of the rest onto the edge, where a fold
/// would give the edge cell a second owner.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "has no world coordinate"))]
fn a_body_past_the_i32_edge_folds_nothing_onto_it() {
    let (structures, placed) = one_placed_row((i32::MAX - 1, 0, 0), 3);
    let plan = collect_floor_cells(&structures, &placed);
    assert_eq!(
        plan.cells,
        HashSet::from([(i32::MAX - 1, 0, 0), (i32::MAX, 0, 0)]),
    );
    assert!(
        plan.owners.values().all(|owners| owners.len() == 1),
        "{:?}",
        plan.owners,
    );
    assert_eq!(plan.extents["site::s::b"].max_x, i32::MAX);
}

/// The other two guards in [`collect_floor_cells`]: a placement with no
/// structure, and a body with no row 0. `lower_site` produces neither,
/// so each is handed in, and each stops a debug build.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "has no structure")]
fn a_placement_with_no_structure_is_not_a_silent_skip() {
    let (_, placed) = one_placed_row((0, 0, 0), 3);
    let _ = collect_floor_cells(&IndexMap::new(), &placed);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "is outside")]
fn a_body_with_no_row_0_is_not_a_silent_skip() {
    let (mut structures, placed) = one_placed_row((0, 0, 0), 3);
    structures["site::s::b"].dims.y = 0;
    let _ = collect_floor_cells(&structures, &placed);
}

/// The plate is the one hardcoded id a pack *can* redirect, and the
/// list would be making a false promise if it carried it: the id is a
/// Java spelling, and Bedrock declares nothing by that name.
#[test]
fn the_redirectable_plate_id_is_not_in_that_list() {
    assert!(
        !BUILTIN_BLOCK_IDS.contains(&PRESSURE_PLATE_BASE_ID),
        "BUILTIN_BLOCK_IDS promises every target declares its entries, and no Bedrock \
         target declares `{PRESSURE_PLATE_BASE_ID}`; the plate is covered by the pack \
         tests around `{PRESSURE_PLATE_TOKEN}` instead",
    );
}
use crate::{lower, parse, resolve};

fn lowered(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// [`lowered`], with every body's palette holding `palette_capacity`
/// entries rather than [`PALETTE_CAPACITY`].
fn lowered_within(source: &str, palette_capacity: usize) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array_within(&ir, &resolution, None, palette_capacity)
}

/// `theme t`, binding `states` slots, `s0` up, each to a block of its
/// own, and a 1x1 `body` header followed by one `walls` row per slot.
/// Every row paints the body's one ring cell, so the body writes
/// `states` distinct states and the finished body keeps the last.
fn overwriting_walls_in(body: &str, states: usize) -> String {
    use std::fmt::Write as _;

    let mut source = String::from("theme t:\n");
    for i in 0..states {
        writeln!(source, "  slot s{i} -> @b{i}").expect("writing to a String");
    }
    writeln!(source, "\n{body}").expect("writing to a String");
    for i in 0..states {
        writeln!(source, "  walls mat_slot=s{i} height=1").expect("writing to a String");
    }
    source
}

/// [`overwriting_walls_in`] a `struct s`.
fn overwriting_walls(states: usize) -> String {
    overwriting_walls_in("struct s size=1x1", states)
}

/// [`overwriting_walls_in`] a `def d`, placed once as `a`.
fn placed_overwriting_walls(states: usize) -> String {
    let mut source = overwriting_walls_in("def d size=1x1:", states);
    source.push_str("\nsite v:\n  place id=a use=d theme=t at=origin\n");
    source
}

/// `theme t` binding slot `$slot` to `@$block` (`@cobblestone` when no
/// block is given), followed by `$rest`, as one `&'static str`: the
/// theme most sources here open with.
macro_rules! theme_t {
    ($slot:literal, $rest:literal $(,)?) => {
        theme_t!($slot -> "cobblestone", $rest)
    };
    ($slot:literal -> $block:literal, $rest:literal $(,)?) => {
        concat!("theme t:\n  slot ", $slot, " -> @", $block, "\n", $rest)
    };
}

/// The findings of `code` in `ir`.
fn findings(ir: &BlockArrayIr, code: DiagnosticCode) -> Vec<&Diagnostic> {
    ir.diagnostics.iter().filter(|d| d.code == code).collect()
}

/// The `W_DEFERRED_MEMBER` findings in `out`.
fn deferrals(out: &BlockArrayIr) -> Vec<&Diagnostic> {
    findings(out, DiagnosticCode::DeferredMember)
}

/// How many `W_DEFERRED_MEMBER` findings `out` holds.
fn deferred_count(out: &BlockArrayIr) -> usize {
    deferrals(out).len()
}

/// The `E_INCOMPATIBLE_MATERIAL` findings in `out`.
fn refusals(out: &BlockArrayIr) -> Vec<&Diagnostic> {
    findings(out, DiagnosticCode::IncompatibleMaterial)
}

/// The primaries of the findings of `code` in `out`.
fn primaries_of(out: &BlockArrayIr, code: DiagnosticCode) -> Vec<&str> {
    findings(out, code)
        .into_iter()
        .map(|d| d.primary.as_str())
        .collect()
}

/// Each `W_DEFERRED_MEMBER` `src` lowered to, as the text of `src` its
/// span covers and its primary.
fn deferred_lines<'a>(src: &'a str, out: &'a BlockArrayIr) -> Vec<(&'a str, &'a str)> {
    deferrals(out)
        .into_iter()
        .map(|d| (&src[d.span.start..d.span.end], d.primary.as_str()))
        .collect()
}

/// Asserts `out` holds a `W_DEFERRED_MEMBER` on the member written as
/// `line` in `src`, whose primary contains `needle`.
#[track_caller]
fn assert_defers_on(src: &str, out: &BlockArrayIr, line: &str, needle: &str) {
    let found = deferred_lines(src, out);
    assert!(
        found
            .iter()
            .any(|&(at, primary)| at == line && primary.contains(needle)),
        "expected a deferral on `{line}` saying `{needle}`, got {found:#?}\nall findings: {:#?}",
        out.diagnostics,
    );
}

/// The three properties `tests/check/diagnostic_text.rs` holds every string
/// its corpus renders to. That corpus lowers at [`PALETTE_CAPACITY`],
/// so it cannot reach `W_PALETTE_TOO_LARGE`, and its text is checked
/// here instead.
fn assert_reads_as_prose(origin: &str, text: &str) {
    assert!(
        !text.contains("  "),
        "{origin} renders a run of spaces, which is a dropped `\\` line \
         continuation in the literal: {text:?}",
    );
    assert!(
        !text.contains('\n') && !text.contains('\t'),
        "{origin} embeds its own line break: {text:?}",
    );
    assert!(!text.trim().is_empty(), "{origin} renders nothing");
    assert_eq!(text.trim(), text, "{origin} has leading or trailing space");
}

/// A body that paints more states than its palette can hold is refused
/// with `W_PALETTE_TOO_LARGE` rather than panicking. Run at a capacity
/// of four (air and three states) so the refusal is reachable without
/// painting 65,536 states; the boundary itself is pinned on
/// `Palette::try_intern`. States a later member covers count, which is
/// why the refused body would have kept a single state.
///
/// The refusal comes before the phase conflicts are reported: a body
/// that is not built has no conflict to resolve, and at the real
/// capacity a body one state short of it reports 65,534 of them.
#[test]
fn a_body_painting_more_states_than_its_palette_holds_is_refused() {
    let fits = lowered_within(&overwriting_walls(3), 4);
    assert!(
        findings(&fits, DiagnosticCode::PaletteTooLarge).is_empty(),
        "three states fit: {:#?}",
        fits.diagnostics,
    );
    let array = fits
        .structures
        .get("struct::s")
        .expect("three states build");
    assert_eq!(
        array.palette.entries.len(),
        2,
        "air and the last wall's state: the covered ones are pruned after painting",
    );
    assert!(
        !findings(&fits, DiagnosticCode::PhaseConflict).is_empty(),
        "a built body reports the walls that overwrite one another: {:#?}",
        fits.diagnostics,
    );

    let over = lowered_within(&overwriting_walls(4), 4);
    let refused = findings(&over, DiagnosticCode::PaletteTooLarge);
    assert_eq!(refused.len(), 1, "{:#?}", over.diagnostics);
    assert_eq!(refused[0].severity(), Severity::Warning);
    assert!(
        refused[0]
            .primary
            .starts_with("`s` paints more than 3 distinct non-air block states"),
        "{}",
        refused[0].primary,
    );
    assert!(
        !over.structures.contains_key("struct::s"),
        "the refused body must not reach a writer",
    );
    assert_eq!(
        findings(&over, DiagnosticCode::PhaseConflict),
        Vec::<&Diagnostic>::new(),
        "a refused body is not built, so it has no conflict to report",
    );
}

/// The rendered text of the refusal reads as prose: a dropped `\`
/// continuation in the primary's or the note's literal is caught here.
#[test]
fn the_palette_refusal_renders_as_prose() {
    let over = lowered_within(&overwriting_walls(4), 4);
    let refused = findings(&over, DiagnosticCode::PaletteTooLarge);
    assert_eq!(refused.len(), 1, "{:#?}", over.diagnostics);
    assert_reads_as_prose("W_PALETTE_TOO_LARGE primary", &refused[0].primary);
    assert_eq!(refused[0].notes.len(), 1, "{:#?}", refused[0].notes);
    assert_reads_as_prose("W_PALETTE_TOO_LARGE note 0", &refused[0].notes[0].message);
}

/// The `def` path to the same refusal, as `tests/lower/structure_volume.rs`
/// covers it for the extent: a `place` paints its `def` through the body
/// lowering a `struct` uses, and a refused body places nothing. The
/// finding names the row and anchors on it.
#[test]
fn a_place_whose_def_paints_more_states_than_its_palette_holds_is_refused() {
    let fits = lowered_within(&placed_overwriting_walls(3), 4);
    assert!(
        findings(&fits, DiagnosticCode::PaletteTooLarge).is_empty(),
        "three states fit: {:#?}",
        fits.diagnostics,
    );
    assert!(
        fits.placements.contains_key("site::v::a"),
        "{:?}",
        fits.placements.keys(),
    );

    let source = placed_overwriting_walls(4);
    let over = lowered_within(&source, 4);
    let refused = findings(&over, DiagnosticCode::PaletteTooLarge);
    assert_eq!(refused.len(), 1, "{:#?}", over.diagnostics);
    assert!(
        refused[0]
            .primary
            .starts_with("`a` paints more than 3 distinct non-air block states"),
        "{}",
        refused[0].primary,
    );
    assert!(
        source[refused[0].span.clone()].starts_with("place id=a"),
        "anchored on the row, not the def: {:?}",
        &source[refused[0].span.clone()],
    );
    assert!(over.structures.is_empty(), "{:?}", over.structures.keys());
    assert!(over.placements.is_empty(), "{:?}", over.placements.keys());
    assert_eq!(
        findings(&over, DiagnosticCode::PhaseConflict),
        Vec::<&Diagnostic>::new(),
    );
}

/// Once a paint has overflowed, `intern` answers air for every later
/// state, one the palette already holds included, without looking it
/// up: the body is refused, so nothing reads what a later paint gets.
#[test]
fn a_scope_palette_looks_nothing_up_once_it_has_overflowed() {
    let mut palette = ScopePalette::with_capacity(2);
    let kept = BlockState::bare("test:kept");
    assert_eq!(palette.intern(kept.clone()), PaletteIndex(1));
    assert_eq!(
        palette.intern(BlockState::bare("test:refused")),
        PaletteIndex::AIR,
    );
    assert_eq!(
        palette.intern(kept),
        PaletteIndex::AIR,
        "a state the palette holds is not looked up after the overflow",
    );
    assert_eq!(
        palette.into_palette(),
        Err(PaletteFull { capacity: 2 }),
        "the refusal kept is the one at the capacity in force",
    );
}

fn lowered_with_resolver(source: &str, resolver: &dyn TargetRegistry) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, Some(resolver))
}

/// In-memory registry for the lowering tests. Declares no id table,
/// so these tests exercise lowering the way `cairn check` runs it —
/// with no target pinned and no id refuted. The `E_UNKNOWN_ID` path
/// has its own tests in `material.rs` and `cairn-lang-cli/tests/cli/compile/block_ids.rs`.
struct FakeResolver {
    entries: Vec<(&'static str, &'static str)>,
}

impl TargetRegistry for FakeResolver {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        self.entries
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
    }

    fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
        None
    }
}

/// A registry whose tokens carry blockstates.
///
/// [`FakeResolver`] answers with `BlockState::bare`, as the real
/// `PackView` does, so it cannot reach the branch that keeps a binding's
/// id while dropping its properties.
struct StatefulResolver {
    token: &'static str,
    id: &'static str,
    properties: Vec<(&'static str, &'static str)>,
}

impl TargetRegistry for StatefulResolver {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        (token == self.token).then(|| BlockState {
            id: format!("minecraft:{}", self.id),
            properties: self
                .properties
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        })
    }

    fn known_tokens(&self) -> Vec<String> {
        vec![self.token.to_owned()]
    }

    fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
        None
    }
}

/// A registry with a pinned target, for the `E_UNKNOWN_ID` path.
///
/// Separate from [`FakeResolver`] so the tests using that one stay in
/// no-target mode; a table there would refuse ids in a hundred tests that
/// are about something else.
struct PinnedRegistry {
    entries: Vec<(&'static str, &'static str)>,
    /// Sorted, fully namespaced ids the pinned target declares.
    ids: Vec<String>,
    /// One alias group, in the order a pack wrote it. Empty for a pack
    /// shipping no `aliases` component.
    group: Vec<String>,
}

impl PinnedRegistry {
    fn new(entries: Vec<(&'static str, &'static str)>, ids: &[&str]) -> Self {
        let mut ids: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
        ids.sort();
        Self {
            entries,
            ids,
            group: Vec::new(),
        }
    }

    /// Declare one alias group over the same ids.
    fn aliasing(mut self, group: &[&str]) -> Self {
        self.group = group.iter().map(|id| (*id).to_owned()).collect();
        self
    }
}

impl TargetRegistry for PinnedRegistry {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        self.entries
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
    }

    fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
        Some(crate::block_array::BlockIdSet::new("test 1.0", &self.ids))
    }

    fn aliases_for(&self, id: &str) -> Vec<String> {
        if !self.group.iter().any(|spelling| spelling == id) {
            return Vec::new();
        }
        self.group
            .iter()
            .filter(|spelling| self.ids.binary_search(spelling).is_ok())
            .cloned()
            .collect()
    }
}

struct UnknownIdPayload {
    id: String,
    registry: String,
    origin: String,
    token: Option<String>,
    suggestion: Option<String>,
    aliases: Vec<String>,
}

fn unknown_id_payload(out: &BlockArrayIr) -> UnknownIdPayload {
    let found = findings(out, DiagnosticCode::UnknownId);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one E_UNKNOWN_ID, got {:?}",
        out.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
    );
    match found[0].data.clone() {
        Some(DiagnosticData::UnknownId {
            id,
            registry,
            origin,
            token,
            suggestion,
            aliases,
        }) => UnknownIdPayload {
            id,
            registry,
            origin,
            token,
            suggestion,
            aliases,
        },
        other => panic!("expected an UnknownId payload, got {other:?}"),
    }
}

/// The payload's `token` is what tells a consumer whose mistake it is,
/// and it has to come from the origin rather than be a constant.
///
/// `mat_slot=` reaches the same code either way, so the two halves are
/// asserted against one another: a payload that hardcoded `None` would
/// pass the authored case on its own.
#[test]
fn the_payload_names_the_token_only_when_the_catalog_chose_the_id() {
    let registry = PinnedRegistry::new(
        vec![("floor.stone.smooth", "stone_bricks")],
        &["minecraft:stonebrick"],
    );

    let authored = lowered_with_resolver(
        "theme t:\n  slot floor -> @stone_brick\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &registry,
    );
    let payload = unknown_id_payload(&authored);
    assert_eq!(payload.id, "minecraft:stone_brick");
    assert_eq!(payload.registry, "test 1.0");
    assert_eq!(payload.origin, "authored");
    assert_eq!(payload.token, None, "the author wrote this id themselves");
    assert_eq!(payload.suggestion.as_deref(), Some("minecraft:stonebrick"));

    let from_catalog = lowered_with_resolver(
        "theme t:\n  slot floor -> @floor.stone.smooth\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &registry,
    );
    let payload = unknown_id_payload(&from_catalog);
    assert_eq!(payload.id, "minecraft:stone_bricks");
    assert_eq!(payload.origin, "catalog");
    assert_eq!(
        payload.token.as_deref(),
        Some("floor.stone.smooth"),
        "the catalog produced this id, so the author's token is not the fix site",
    );
}

/// The member default that comes from the pack is the one id that
/// reaches a palette without passing `resolve_block_state`, so it has
/// its own path through the check and its own origin.
///
/// Both halves matter. A registry that *declares* the token but maps it
/// onto a missing id is the pack's mapping being wrong; a registry with
/// no row at all sends lowering to the id compiled into this crate,
/// which is a Java spelling and wrong on Bedrock.
#[test]
fn a_member_default_from_the_pack_is_checked_like_any_other_id() {
    let plate = "struct s size=3x3\n  pressure_plate id=p at=front.outside\n";

    let mapped_wrong = PinnedRegistry::new(
        vec![(PRESSURE_PLATE_TOKEN, "oak_pressure_plate")],
        &["minecraft:wooden_pressure_plate"],
    );
    let out = lowered_with_resolver(plate, &mapped_wrong);
    let payload = unknown_id_payload(&out);
    assert_eq!(payload.id, "minecraft:oak_pressure_plate");
    assert_eq!(payload.origin, "catalog");
    assert_eq!(payload.token.as_deref(), Some(PRESSURE_PLATE_TOKEN));

    let no_row = PinnedRegistry::new(vec![], &["minecraft:wooden_pressure_plate"]);
    let out = lowered_with_resolver(plate, &no_row);
    let payload = unknown_id_payload(&out);
    assert_eq!(
        payload.id, PRESSURE_PLATE_BASE_ID,
        "a pack with no row sends lowering to the compiled-in default",
    );
    assert_eq!(payload.origin, "builtin");
    assert_eq!(payload.token.as_deref(), Some(PRESSURE_PLATE_TOKEN));

    let right = PinnedRegistry::new(
        vec![(PRESSURE_PLATE_TOKEN, "wooden_pressure_plate")],
        &["minecraft:wooden_pressure_plate"],
    );
    let out = lowered_with_resolver(plate, &right);
    assert!(
        findings(&out, DiagnosticCode::UnknownId).is_empty(),
        "a pack that spells it the way the target does must not be refused: {:?}",
        out.diagnostics
            .iter()
            .map(|d| d.primary.as_str())
            .collect::<Vec<_>>(),
    );
}

/// A rename reaches the message as a statement, and a typo as a
/// guess, and the note says which it is holding.
///
/// The wording is the load-bearing part: "spells this block" is the
/// pack asserting two names are one block, "spells the nearest block"
/// is a distance search offering its best candidate, and an author who
/// cannot tell them apart has to go and check either way.
#[test]
fn a_rename_and_a_typo_read_as_different_claims() {
    let renamed = PinnedRegistry::new(vec![], &["minecraft:standing_sign"])
        .aliasing(&["minecraft:oak_sign", "minecraft:standing_sign"]);
    let out = lowered_with_resolver(
        "theme t:\n  slot floor -> @oak_sign\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &renamed,
    );
    let payload = unknown_id_payload(&out);
    assert_eq!(payload.aliases, ["minecraft:standing_sign".to_owned()]);
    let note = unknown_id_note(&out);
    assert!(
        note.contains("spells this block `minecraft:standing_sign`")
            && note.contains("alias table"),
        "the note must name the alias table it is quoting, got: {note}",
    );

    let typo = PinnedRegistry::new(vec![], &["minecraft:oak_planks"]);
    let out = lowered_with_resolver(
        "theme t:\n  slot floor -> @oak_plank\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &typo,
    );
    let note = unknown_id_note(&out);
    assert!(
        note.contains("spells the nearest block") && !note.contains("alias table"),
        "a distance guess must not be dressed up as the pack's word, got: {note}",
    );
}

/// A split lists the first few candidates and counts the rest, and the
/// payload keeps all of them.
///
/// The truncation is about the sentence a person reads;
/// `spec/versioning-editions` "Fail-loud and minimum-version inference"
/// asks the error to return the closed set of candidates, and the `data`
/// payload is where a consumer finds it whole.
#[test]
fn a_split_is_summarised_in_the_note_and_kept_whole_in_the_payload() {
    let ids: Vec<String> = (0..16)
        .map(|i| format!("minecraft:light_block_{i}"))
        .collect();
    let mut group = vec!["minecraft:light".to_owned()];
    group.extend(ids.iter().cloned());
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let group_refs: Vec<&str> = group.iter().map(String::as_str).collect();
    let registry = PinnedRegistry::new(vec![], &refs).aliasing(&group_refs);
    let out = lowered_with_resolver(
        "theme t:\n  slot floor -> @light\nstruct s size=2x2\n  floor mat_slot=floor\n",
        &registry,
    );
    let payload = unknown_id_payload(&out);
    assert_eq!(payload.aliases.len(), 16, "every candidate reaches a tool");
    let note = unknown_id_note(&out);
    assert!(
        note.contains("and 12 more"),
        "the sentence counts what it does not list, got: {note}",
    );
}

/// The one note the `E_UNKNOWN_ID` finding carries about candidates —
/// the last one, after the origin note the two pack-chosen origins add.
fn unknown_id_note(out: &BlockArrayIr) -> String {
    findings(out, DiagnosticCode::UnknownId)
        .first()
        .expect("an E_UNKNOWN_ID finding")
        .notes
        .last()
        .expect("the candidate note")
        .message
        .clone()
}

fn block_id(ba: &BlockArray, x: u32, y: u32, z: u32) -> &str {
    let i = ba.dims.index(x, y, z).expect("in-range coordinate");
    let pi = ba.voxels[i];
    ba.palette.entries[usize::from(pi.0)].id.as_str()
}

#[test]
fn floor_only_fills_y_zero_plane() {
    let src = theme_t!("f", "\nstruct s size=3x3\n  floor mat_slot=f\n");
    let out = lowered(src);
    let ba = out.structures.get("struct::s").expect("structure lowered");
    assert_eq!(ba.dims, Dims { x: 3, y: 1, z: 3 });
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), "minecraft:cobblestone");
        }
    }
    assert!(
        out.diagnostics.is_empty(),
        "no diagnostics expected, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn walls_only_fills_outline_above_floor() {
    let src = theme_t!("w", "\nstruct s size=3x3\n  walls mat_slot=w height=2\n");
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims, Dims { x: 3, y: 3, z: 3 });
    // y=0 stays air everywhere — there is no floor in this struct.
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID);
        }
    }
    // y=1 and y=2 carry the outline; the centre cell stays air.
    for y in 1..=2 {
        assert_eq!(block_id(ba, 1, y, 1), BlockState::AIR_ID, "centre at y={y}");
        for z in 0..3 {
            for x in 0..3 {
                let on_edge = x == 0 || x == 2 || z == 0 || z == 2;
                let expected = if on_edge {
                    "minecraft:cobblestone"
                } else {
                    BlockState::AIR_ID
                };
                assert_eq!(block_id(ba, x, y, z), expected, "({x},{y},{z})");
            }
        }
    }
}

#[test]
fn floor_and_walls_combine() {
    let src = "theme t:\n  slot f -> @oak_planks\n  slot w -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  walls mat_slot=w height=2\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims, Dims { x: 3, y: 3, z: 3 });
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), "minecraft:oak_planks");
        }
    }
    assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
    assert_eq!(block_id(ba, 1, 1, 1), BlockState::AIR_ID);
}

#[test]
fn a_stair_without_kind_defers_without_touching_a_voxel() {
    let src = theme_t!(
        "f",
        "\nstruct s size=3x3\n  floor mat_slot=f\n  stair side=front\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(deferred_count(&out), 1);
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), "minecraft:cobblestone");
        }
    }
}

#[test]
fn circuit_region_recognised_without_deferred_warning() {
    // A routing marker for place-and-route, accepted without a
    // `W_DEFERRED_MEMBER`.
    let src = theme_t!(
        "f",
        "\nstruct s size=3x3\n  floor mat_slot=f\n  circuit region=floor void=2\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "circuit should not emit W_DEFERRED_MEMBER, got {:?}",
        out.diagnostics,
    );
}

/// A member that lowering defers, and what the one `W_DEFERRED_MEMBER`
/// it earns says.
struct OneDeferral {
    /// The member lines, after the source's shared head.
    members: &'static str,
    /// Text the primary contains.
    says: &'static [&'static str],
    /// Text the primary does not contain.
    never: &'static [&'static str],
    /// A note the finding carries, word for word.
    note: Option<&'static str>,
}

impl OneDeferral {
    /// A row that checks only what the primary says.
    const fn says(members: &'static str, says: &'static [&'static str]) -> Self {
        Self {
            members,
            says,
            never: &[],
            note: None,
        }
    }
}

/// Lowers `head` followed by each row's members, and checks that each
/// earns exactly one `W_DEFERRED_MEMBER`, saying what its row says.
fn assert_each_defers_once(head: &str, rows: &[OneDeferral]) {
    for row in rows {
        let src = format!("{head}{}", row.members);
        let out = lowered(&src);
        let deferred = deferrals(&out);
        assert_eq!(deferred.len(), 1, "{src}{:#?}", out.diagnostics);
        let primary = &deferred[0].primary;
        for text in row.says {
            assert!(
                primary.contains(text),
                "expected the primary to name `{text}`, got {primary}\n{src}",
            );
        }
        for text in row.never {
            assert!(
                !primary.contains(text),
                "expected the primary not to say `{text}`, got {primary}\n{src}",
            );
        }
        if let Some(note) = row.note {
            assert!(
                deferred[0].notes.iter().any(|n| n.message == note),
                "expected the note {note:?}, got {:?}\n{src}",
                deferred[0].notes,
            );
        }
    }
}

#[test]
fn a_malformed_circuit_defers_once_and_names_its_fault() {
    assert_each_defers_once(
        theme_t!("f", "\nstruct s size=3x3\n  floor mat_slot=f\n"),
        &[
            OneDeferral::says("  circuit void=2\n", &["region="]),
            OneDeferral::says("  circuit region=floor\n", &["void="]),
            OneDeferral::says("  circuit region=floor void=0\n", &["void=0"]),
            // Past `u32`: `nonneg_int_or_defer` reports it, and the
            // recogniser must not report it again.
            OneDeferral::says("  circuit region=floor void=99999999999\n", &["void="]),
            // An empty string is its own fault, not a missing `region=`.
            OneDeferral::says("  circuit region=\"\" void=2\n", &["non-empty"]),
            // The wrong kind, named, rather than a missing key.
            OneDeferral::says(
                "  circuit region=42 void=2\n",
                &["identifier or string label", "integer"],
            ),
        ],
    );
}

#[test]
fn actuator_patch_opened_by_recognised_without_deferred_warning() {
    // The patch never reaches the openings phase: no "missing side="
    // deferral, no second carve.
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  \
         walls mat_slot=w height=3\n  \
         door id=front side=front at=center\n  \
         door[id=front] opened_by=sig.open\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "actuator patch should not emit W_DEFERRED_MEMBER, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn a_malformed_actuator_patch_defers_once_and_names_its_fault() {
    // Rows below run in a 5x5 walled struct whose front door is labelled
    // `front`.
    assert_each_defers_once(
        theme_t!(
            "w",
            "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  \
             door id=front side=front at=center\n",
        ),
        &[
            // A selector key past `id=` is named, with the `id=<label>`
            // shape.
            OneDeferral::says(
                "  door[id=front class=main] opened_by=sig.open\n",
                &["class", "id="],
            ),
            // The parser accepts an empty selector. It reaches the
            // missing-id arm alone, not the unknown-key arm above.
            OneDeferral {
                members: "  door[] opened_by=sig.open\n",
                says: &["id="],
                never: &["unknown attribute"],
                note: None,
            },
            // Not a label: the primary names the kind, as circuit's does.
            OneDeferral::says("  door[id=3] opened_by=sig.open\n", &["integer"]),
            // No such door: the primary names the id and lists the ones
            // declared.
            OneDeferral::says("  door[id=back] opened_by=sig.open\n", &["back", "front"]),
            // A patch with no `opened_by=` binds nothing.
            OneDeferral::says("  door[id=front]\n", &["opened_by"]),
            // Not a signal reference: the `sig.<name>` shape and the kind.
            OneDeferral::says("  door[id=front] opened_by=3\n", &["sig.", "integer"]),
            // A two-segment `DotRef` whose head is not `sig`
            // (`spec/redstone` "Signal binding").
            OneDeferral::says("  door[id=front] opened_by=foo.bar\n", &["sig."]),
            // Only `opened_by=` is read; a reserved key such as
            // `powered_by=` (`spec/redstone` "Signal binding") is named
            // beside the one that is.
            OneDeferral::says(
                "  door[id=front] opened_by=sig.open powered_by=sig.on\n",
                &["powered_by", "opened_by"],
            ),
            // Head `sig` but three segments: a guard on the head alone
            // would pass it. The primary names the two-segment rule and
            // quotes the path.
            OneDeferral::says(
                "  door[id=front] opened_by=sig.open.extra\n",
                &["two-segment", "sig.open.extra"],
            ),
            // A second `door id=front` under a `level` makes two doors of
            // one id once flattened, which the per-body `duplicate` check
            // does not flag. Binding the first would guess; the ambiguity
            // is reported, with a note telling the doors apart.
            OneDeferral {
                members: "  level y=0\n    door id=front side=back at=center\n  \
                          door[id=front] opened_by=sig.open\n",
                says: &["2 physical doors"],
                never: &[],
                note: Some(
                    "Fix: give the 2 `door` members distinct ids, so the selector names one.",
                ),
            },
        ],
    );
    // Rows below run in the same struct with no labelled door.
    assert_each_defers_once(
        theme_t!("w", "\nstruct s size=5x5\n  walls mat_slot=w height=3\n"),
        &[
            // A door declared without `id=` can never be picked, but it is
            // a door: the primary counts it rather than saying none is
            // declared, and the note points at labelling it.
            OneDeferral {
                members: "  door side=front at=center\n  door[id=front] opened_by=sig.open\n",
                says: &["1 physical door is declared in this scope, without an `id=`"],
                never: &[],
                note: Some("Fix: add `id=front` to the `door` this patch is meant to bind."),
            },
            // No doors at all: said outright, rather than as an empty
            // list of known ids that reads like a truncation.
            OneDeferral::says(
                "  door[id=front] opened_by=sig.open\n",
                &["no physical door"],
            ),
        ],
    );
}

#[test]
fn actuator_patch_selects_door_declared_inside_level() {
    // The recogniser searches the flattened paint set, so nesting does
    // not hide the id. `y=0` keeps the wall-top interaction of a higher
    // level out of it.
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  \
         walls mat_slot=w height=3\n  \
         level y=0\n    \
           door id=inner side=front at=center\n  \
         door[id=inner] opened_by=sig.tick\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "actuator patch should resolve a level-nested door id, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn actuator_patch_does_not_repaint_the_door_voxels() {
    // The door carves (2, 1..=2, 4); the patch must neither re-carve nor
    // add a palette entry.
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  \
         walls mat_slot=w height=3\n  \
         door id=front side=front at=center\n  \
         door[id=front] opened_by=sig.open\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 2, 2, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 0, 1, 4), "minecraft:cobblestone");
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec![BlockState::AIR_ID, "minecraft:cobblestone"]);
}

#[test]
fn missing_theme_warns_and_air_fills() {
    let src = "struct s size=3x3\n  floor mat_slot=f\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert!(!findings(&out, DiagnosticCode::NoThemeBound).is_empty());
    for z in 0..3 {
        for x in 0..3 {
            assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID);
        }
    }
}

/// The `W_NO_THEME_BOUND` a themeless `struct s` that reads a slot
/// earns.
const S_HAS_NO_THEME: &str = "struct `s` has no theme bound; every `mat_slot=` will lower to air";

/// `src`'s `cairn check` findings, and its lowering: the two streams
/// `cairn check --edition E --target V` prints together.
fn checked_and_lowered(src: &str) -> (Vec<Diagnostic>, BlockArrayIr) {
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    (
        crate::check::check(&module, &ir, None),
        lower_to_block_array(&ir, &resolution, None),
    )
}

/// The array `struct name` lowered to. A silence about a scope means
/// something only when lowering built that scope: a source that lowered
/// nothing says nothing about any of them.
fn lowered_struct<'a>(out: &'a BlockArrayIr, name: &str, src: &str) -> &'a BlockArray {
    out.structures
        .get(&format!("struct::{name}"))
        .unwrap_or_else(|| panic!("`struct {name}` did not lower\n{src}{:#?}", out.diagnostics))
}

/// The ids `ba` paints, air left out.
fn painted_ids(ba: &BlockArray) -> std::collections::BTreeSet<&str> {
    ba.voxels
        .iter()
        .filter(|index| index.0 != 0)
        .map(|index| ba.palette.entries[usize::from(index.0)].id.as_str())
        .collect()
}

#[test]
fn a_scope_that_reads_no_slot_is_not_told_its_slots_lower_to_air() {
    // `spec/lint` "Materials and targets": a scope that reads no
    // `mat_slot=` is not reported. An empty struct, and one whose only
    // member takes its material from the roof fallback. Each struct
    // lowered, and the roof's painted, so the silence is lowering's
    // answer about a scope it built.
    let src = "struct s size=3x3\n";
    let out = lowered(src);
    lowered_struct(&out, "s", src);
    assert_eq!(
        primaries_of(&out, DiagnosticCode::NoThemeBound),
        Vec::<&str>::new(),
        "{src}",
    );

    let src = "struct s size=3x3\n  roof kind=flat\n";
    let out = lowered(src);
    assert_eq!(
        painted_ids(lowered_struct(&out, "s", src)),
        [FLAT_BASE_ID].into(),
        "{src}",
    );
    assert_eq!(
        primaries_of(&out, DiagnosticCode::NoThemeBound),
        Vec::<&str>::new(),
        "{src}",
    );

    // Per scope, not per module: the reader beside it does not make
    // the roof's struct a reader.
    let src = "struct a size=3x3\n  roof kind=flat\n\n\
               struct b size=3x3\n  floor mat_slot=f\n";
    let out = lowered(src);
    assert_eq!(
        painted_ids(lowered_struct(&out, "a", src)),
        [FLAT_BASE_ID].into(),
        "{src}",
    );
    assert_eq!(
        primaries_of(&out, DiagnosticCode::NoThemeBound),
        ["struct `b` has no theme bound; every `mat_slot=` will lower to air"],
        "{src}",
    );
}

/// One themeless struct per keyword whose role reads a `mat_slot=`
/// ([`Member::read_slot`]), in which that keyword's member is the only
/// reader. Every one is reported.
///
/// [`every_keyword_is_a_case_on_the_side_its_role_reads`] ties the rows
/// to the role table, so a role that starts reading a slot arrives as a
/// missing row rather than as a gate nothing measured for it.
const SLOT_READERS: &[(&str, &str)] = &[
    ("floor", "struct s size=3x3\n  floor mat_slot=f\n"),
    ("walls", "struct s size=3x3\n  walls mat_slot=w height=2\n"),
    (
        "window",
        "struct s size=5x5\n  window side=front y=1 offset=1 size=1x1 mat_slot=g\n",
    ),
    ("roof", "struct s size=3x3\n  roof kind=flat mat_slot=r\n"),
    // An eave band sits on a roof that draws an overhang, and that roof
    // reads no slot.
    (
        "stair",
        "struct s size=3x3\n  roof kind=gable overhang=1\n  \
         stair kind=stairs side=front half=bottom facing=out shape=straight mat_slot=s\n",
    ),
    (
        "pressure_plate",
        "struct s size=3x3\n  pressure_plate at=inside.front offset=1 mat_slot=p\n",
    ),
];

/// One themeless struct per keyword whose role reads no `mat_slot=`,
/// with a `mat_slot=d` on that keyword's member and on nothing else.
/// None is reported, and `cairn check` reports the slot instead, as
/// `W_IGNORED_ARGUMENT`.
///
/// The `level`'s children read no slot either, so the struct paints
/// from the roof fallback. A `place` and a `connect` are misplaced in a
/// struct body (`E_MISPLACED_MEMBER`), and are rows here because that
/// is how one reaches the list the gate reads.
const SLOT_IGNORERS: &[(&str, &str)] = &[
    (
        "door",
        "struct s size=3x3\n  door id=front side=front at=center mat_slot=d\n",
    ),
    (
        "level",
        "struct s size=3x3\n  level y=0 mat_slot=d\n    roof kind=flat\n",
    ),
    (
        "circuit",
        "struct s size=3x3\n  circuit region=floor void=2 mat_slot=d\n",
    ),
    (
        "place",
        "struct s size=3x3\n  place id=a use=hut theme=t at=origin mat_slot=d\n",
    ),
    (
        "connect",
        "struct s size=3x3\n  connect a.entry to b.entry path=@gravel mat_slot=d\n",
    ),
];

#[test]
fn a_scope_whose_only_reader_is_any_reading_role_is_told() {
    for (keyword, src) in SLOT_READERS {
        let out = lowered(src);
        lowered_struct(&out, "s", src);
        assert_eq!(
            primaries_of(&out, DiagnosticCode::NoThemeBound),
            [S_HAS_NO_THEME],
            "`{keyword}`\n{src}",
        );
    }
    // An unknown keyword's slot reads too, as `Member::read_slot` says:
    // the line is `E_UNKNOWN_KEYWORD` either way, and the theme the slot
    // will need once the keyword is repaired is missing.
    let src = "struct s size=3x3\n  torch mat_slot=t\n";
    assert_eq!(
        primaries_of(&lowered(src), DiagnosticCode::NoThemeBound),
        [S_HAS_NO_THEME],
        "{src}",
    );
}

#[test]
fn a_mat_slot_on_a_role_that_reads_none_does_not_make_its_scope_a_reader() {
    for (keyword, src) in SLOT_IGNORERS {
        let (checked, out) = checked_and_lowered(src);
        lowered_struct(&out, "s", src);
        assert_eq!(
            primaries_of(&out, DiagnosticCode::NoThemeBound),
            Vec::<&str>::new(),
            "`{keyword}`\n{src}",
        );
        // `check` owns the slot instead: one `W_IGNORED_ARGUMENT`, on
        // its value, saying the name is not looked up. A
        // `W_NO_THEME_BOUND` beside it would say the same name lowers
        // to air.
        let ignored: Vec<(&str, &str)> = checked
            .iter()
            .filter(|d| d.code == DiagnosticCode::IgnoredArgument)
            .map(|d| (&src[d.span.start..d.span.end], d.primary.as_str()))
            .collect();
        assert!(
            matches!(
                ignored.as_slice(),
                [("d", primary)]
                    if primary.starts_with("`mat_slot=` ")
                        && primary.contains(&format!("`{keyword}`"))
            ),
            "`{keyword}`: {ignored:#?}\n{src}",
        );
    }
}

/// Every keyword the role table knows is a row above, on the side its
/// role reads.
///
/// Asked of [`Member::read_slot`] on a member written with each
/// keyword, so a role that starts or stops reading a slot is a row on
/// the wrong side here, whichever table it was in.
#[test]
fn every_keyword_is_a_case_on_the_side_its_role_reads() {
    let reads = |keyword: &str| {
        let src = format!("struct s size=3x3\n  {keyword} mat_slot=x\n");
        let ir = lower(&parse(&src).expect("parse"));
        ir.structs[0].members[0].read_slot().is_some()
    };
    let (mut readers, mut ignorers): (Vec<&str>, Vec<&str>) = crate::intent::known_keywords()
        .iter()
        .copied()
        .partition(|keyword| reads(keyword));
    readers.sort_unstable();
    ignorers.sort_unstable();
    let rows = |table: &[(&'static str, &str)]| {
        let mut keywords: Vec<&'static str> = table.iter().map(|(keyword, _)| *keyword).collect();
        keywords.sort_unstable();
        keywords
    };
    assert_eq!(
        rows(SLOT_READERS),
        readers,
        "every keyword whose role reads a `mat_slot=` needs a row in SLOT_READERS, and only those",
    );
    assert_eq!(
        rows(SLOT_IGNORERS),
        ignorers,
        "every keyword whose role reads no `mat_slot=` needs a row in SLOT_IGNORERS, and only \
         those",
    );
}

#[test]
fn a_slot_read_under_a_level_still_warns_and_one_the_level_drops_does_not() {
    // A `level` is where the only reader in a scope can sit, at the
    // base plane and above it.
    for src in [
        "struct s size=3x3\n  level y=0\n    floor mat_slot=f\n",
        "struct s size=3x3\n  level y=1\n    walls mat_slot=f height=2\n",
    ] {
        assert_eq!(
            primaries_of(&lowered(src), DiagnosticCode::NoThemeBound),
            [S_HAS_NO_THEME],
            "{src}",
        );
    }
    // Every way `flatten_members` drops a reader: by its role above
    // `y=0`, or with the whole `level` it sits under. The member paints
    // nothing whatever the theme, so it does not count as a reader, and
    // the deferral is what says it went.
    for (src, deferral) in [
        (
            "struct s size=3x3\n  level y=1\n    floor mat_slot=f\n",
            "level-scoped `floor` is not yet supported",
        ),
        (
            "struct s size=3x3\n  level y=1\n    roof kind=flat mat_slot=r\n",
            "level-scoped `roof` is not yet supported",
        ),
        (
            "struct s size=3x3\n  level\n    walls mat_slot=f height=2\n",
            "level requires `y=N` (non-negative integer) to place its children",
        ),
        (
            "struct s size=3x3\n  level y=top\n    walls mat_slot=f height=2\n",
            "`y=` must be a non-negative integer that fits in u32",
        ),
        (
            "struct s size=3x3\n  level y=0\n    level y=1\n      walls mat_slot=f height=2\n",
            "nested `level` blocks are not yet supported; this level and every member \
             declared under it were dropped",
        ),
    ] {
        let out = lowered(src);
        lowered_struct(&out, "s", src);
        assert_eq!(
            primaries_of(&out, DiagnosticCode::NoThemeBound),
            Vec::<&str>::new(),
            "{src}",
        );
        assert_eq!(
            primaries_of(&out, DiagnosticCode::DeferredMember),
            [deferral],
            "{src}",
        );
    }
}

#[test]
fn already_diagnosed_slot_does_not_re_warn() {
    // The resolver reports `E_UNRESOLVED_SLOT`; a second finding on the
    // span would read as a second problem.
    let src = theme_t!("f", "\nstruct s size=3x3\n  floor mat_slot=missing\n");
    let out = lowered(src);
    assert!(
        deferrals(&out).is_empty()
            && findings(&out, DiagnosticCode::AbstractTokenDeferred).is_empty(),
        "no follow-on diagnostics expected, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn abstract_token_lifts_through_supplied_resolver() {
    let resolver = FakeResolver {
        entries: vec![
            ("floor.wood.broadleaf", "oak_planks"),
            ("wall.stone.cobble", "cobblestone"),
        ],
    };
    let src = "theme t:\n  \
               slot f -> @floor.wood.broadleaf\n  \
               slot w -> @wall.stone.cobble\n\n\
               struct s size=3x3\n  \
               floor mat_slot=f\n  \
               walls mat_slot=w height=2\n";
    let out = lowered_with_resolver(src, &resolver);
    assert!(
        findings(&out, DiagnosticCode::AbstractTokenDeferred).is_empty(),
        "no abstract-token deferral expected when the resolver covers every token, got {:?}",
        out.diagnostics,
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_planks");
    assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
}

/// One arm of the abstract-token deferral tests: members that read
/// `@wood.dark` from slot `trim` (slot `wall` is `@cobblestone`), what
/// `W_ABSTRACT_TOKEN_DEFERRED` says the member does instead, and a check
/// that the voxels say the same. `None` is a member turned away before it
/// asks for its material, which earns no such warning at all.
struct DeferralArm {
    members: &'static str,
    consequence: Option<&'static str>,
    voxels: fn(&BlockArrayIr),
}

fn check_deferral_arms(arms: &[DeferralArm]) {
    for arm in arms {
        let members = arm.members;
        let out = lowered(&format!(
            "theme t:\n  slot wall -> @cobblestone\n  slot trim -> @wood.dark\n\n\
             struct s size=5x5\n  {members}\n"
        ));
        let expected: Vec<String> = arm
            .consequence
            .map(|c| {
                format!(
                    "abstract token `@wood.dark` cannot be lowered without the registry \
                     pack; {c}"
                )
            })
            .into_iter()
            .collect();
        assert_eq!(
            primaries_of(&out, DiagnosticCode::AbstractTokenDeferred),
            expected,
            "{members}",
        );
        (arm.voxels)(&out);
    }
}

fn struct_s(out: &BlockArrayIr) -> &BlockArray {
    out.structures.get("struct::s").expect("struct::s lowered")
}

fn has_id(out: &BlockArrayIr, id: &str) -> bool {
    struct_s(out).palette.entries.iter().any(|e| e.id == id)
}

fn refused_for_no_wall(out: &BlockArrayIr) -> bool {
    primaries_of(out, DiagnosticCode::DeferredMember)
        .iter()
        .any(|p| p.contains("has no wall to cut into"))
}

#[test]
fn abstract_token_deferral_says_what_each_member_does_instead() {
    let default_block = Some("the member is built from its default block");
    check_deferral_arms(&[
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      window side=front offset=1 y=1 size=1x2 mat_slot=trim",
            consequence: Some("the window is not cut, and the wall stays"),
            voxels: |out| {
                for y in 1..=2 {
                    assert_eq!(block_id(struct_s(out), 1, y, 4), "minecraft:cobblestone");
                }
            },
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  floor mat_slot=trim",
            consequence: Some("the cell falls back to air"),
            voxels: |out| assert_eq!(block_id(struct_s(out), 1, 0, 1), BlockState::AIR_ID),
        },
        // The walls' rows are absent rather than air, and a window cut
        // into them is refused.
        DeferralArm {
            members: "floor mat_slot=wall\n  \
                      walls mat_slot=trim height=3\n  \
                      window side=front offset=1 y=1 size=1x2 mat_slot=wall",
            consequence: Some(
                "the walls are not built and take up no rows, so a door or window cut into \
                 them is refused",
            ),
            voxels: |out| {
                assert_eq!(struct_s(out).dims.y, 1);
                assert!(refused_for_no_wall(out));
            },
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  roof kind=gable mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  roof kind=flat mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, FLAT_BASE_ID)),
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      roof kind=shed slope_to=front mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      roof kind=flat mat_slot=wall overhang=1\n  \
                      stair kind=stairs side=front mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      pressure_plate at=front.outside offset=2 y=0 mat_slot=trim",
            consequence: default_block,
            voxels: |out| assert!(has_id(out, PRESSURE_PLATE_BASE_ID)),
        },
    ]);
}

#[test]
fn a_member_refused_before_its_material_defers_no_abstract_token() {
    check_deferral_arms(&[
        // `cut_window` refuses a window with no wall before it resolves
        // the material.
        DeferralArm {
            members: "floor mat_slot=wall\n  \
                      window side=front offset=1 y=1 size=1x2 mat_slot=trim",
            consequence: None,
            voxels: |out| {
                assert_eq!(struct_s(out).dims.y, 1);
                assert!(refused_for_no_wall(out));
            },
        },
        // A shed with no usable `slope_to=` draws nothing, so it claims no
        // material either: the one finding is the one that says why.
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  roof kind=shed mat_slot=trim",
            consequence: None,
            voxels: |out| {
                assert_eq!(struct_s(out).dims.y, 4);
                assert!(!has_id(out, STAIR_BASE_ID));
                assert_eq!(
                    primaries_of(out, DiagnosticCode::DeferredMember),
                    vec!["shed roof requires `slope_to=` (one of front, back, left, right)"],
                );
            },
        },
        DeferralArm {
            members: "walls mat_slot=wall height=3\n  \
                      roof kind=shed slope_to=sideways mat_slot=trim",
            consequence: None,
            voxels: |out| {
                assert_eq!(struct_s(out).dims.y, 4);
                assert!(!has_id(out, STAIR_BASE_ID));
                assert_eq!(
                    primaries_of(out, DiagnosticCode::DeferredMember),
                    vec![
                        "unknown shed `slope_to=sideways` (expected one of front, back, left, \
                         right)"
                    ],
                );
            },
        },
    ]);
}

#[test]
fn unknown_abstract_token_emits_e_unknown_abstract_token() {
    let resolver = FakeResolver {
        entries: vec![("floor.wood.broadleaf", "oak_planks")],
    };
    let src = "theme t:\n  \
               slot f -> @floor.wood.broadlef\n\n\
               struct s size=3x3\n  \
               floor mat_slot=f\n";
    let out = lowered_with_resolver(src, &resolver);
    let diag = *findings(&out, DiagnosticCode::UnknownAbstractToken)
        .first()
        .expect("expected E_UNKNOWN_ABSTRACT_TOKEN, got {:?}");
    assert_eq!(diag.severity(), Severity::Error);
    assert!(
        diag.notes
            .iter()
            .any(|n| n.message.contains("floor.wood.broadleaf")),
        "expected suggestion note in {:?}",
        diag.notes,
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 1, 0, 1), BlockState::AIR_ID);
}

#[test]
fn struct_without_size_is_skipped_with_warning() {
    let src = theme_t!("f", "\nstruct s\n  floor mat_slot=f\n");
    let out = lowered(src);
    assert!(!out.structures.contains_key("struct::s"));
    assert!(!findings(&out, DiagnosticCode::StructNoSize).is_empty());
}

#[test]
fn state_literal_round_trips_through_palette() {
    // Exercises the palette/material path directly, below the parser,
    // to lock the canonical-id and property-bag contract on its own.
    let mut palette = Palette::new_with_air();
    let token = ValueWithSpan::from_value(crate::ast::Value::new(
        ValueKind::Token("oak_log[axis=x]".to_owned()),
        0..16,
    ));
    let bs = resolve_block_state(&token, None).unwrap();
    let idx = palette.intern(bs);
    assert_eq!(palette.entries[usize::from(idx.0)].id, "minecraft:oak_log");
    assert_eq!(
        palette.entries[usize::from(idx.0)]
            .properties
            .get("axis")
            .map(String::as_str),
        Some("x"),
    );
}

// --- door / window / roof voxelisation ----------------------------------

#[test]
fn phase_order_independent_of_source_order() {
    // The door precedes the walls in source; openings still run after
    // massing, so the carve survives the wall fill.
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  door side=front at=center\n  walls mat_slot=w height=3\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    // Front wall is z = dims.z - 1 = 4. Center x = (5-1)/2 = 2. Door y=1,2.
    assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 2, 2, 4), BlockState::AIR_ID);
    // Wall corners survived.
    assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
}

#[test]
fn roof_increases_dims_y_by_ceil_half_span() {
    // size=9x7, walls height=4, kind=gable, overhang=0.
    // roof bbox short axis = min(9, 7) = 7 → ridge_extra = ceil(7/2) = 4.
    // dims.y = 1 + 4 + 4 = 9.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims, Dims { x: 9, y: 9, z: 7 });
}

#[test]
fn roof_overhang_extends_xz_dims_and_shifts_walls() {
    // overhang=1 → dims.x = 9+2 = 11, dims.z = 7+2 = 9.
    // Floor is the 9x7 interior placed at x∈[1, 9], z∈[1, 7].
    let src = "theme t:\n  slot f -> @oak_planks\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  floor mat_slot=f\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r overhang=1\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.x, 11);
    assert_eq!(ba.dims.z, 9);
    // Floor inside the interior, air at the overhang ring.
    assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_planks");
    assert_eq!(block_id(ba, 9, 0, 7), "minecraft:oak_planks");
    assert_eq!(block_id(ba, 0, 0, 0), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 10, 0, 8), BlockState::AIR_ID);
    // Wall corner shifted to (1, 1, 1) rather than (0, 1, 0).
    assert_eq!(block_id(ba, 1, 1, 1), "minecraft:cobblestone");
    assert_eq!(block_id(ba, 0, 1, 0), BlockState::AIR_ID);
}

// ------------------------------------------------------------------
// A geometry pass may only attach stair states to a stair.
// ------------------------------------------------------------------

/// Every distinct palette entry the lowering produced for `struct::s`.
fn palette_of(out: &BlockArrayIr) -> Vec<BlockState> {
    out.structures
        .get("struct::s")
        .expect("scope lowered")
        .palette
        .entries
        .clone()
}

/// Ids of the palette entries that carry blockstate properties.
fn ids_carrying_properties(out: &BlockArrayIr) -> Vec<String> {
    palette_of(out)
        .into_iter()
        .filter(|e| !e.properties.is_empty())
        .map(|e| e.id)
        .collect()
}

/// A struct whose roof and eave read `slot r`, bound to `material`.
fn roofed(kind: &str, material: &str) -> String {
    format!(
        "{}  slot r -> @{material}\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind={kind} mat_slot=r overhang=1\n",
        theme_t!("w", ""),
    )
}

#[test]
fn a_sloped_roof_refuses_a_material_that_is_not_a_stair() {
    // A whole block has nowhere to carry `facing` / `half` / `shape`,
    // so a palette entry pairing them would name a blockstate that does
    // not exist. Refusing at the binding is the only answer that does
    // not either write it or silently pick a different material.
    for kind in ["gable", "shed", "hip"] {
        let src =
            roofed(kind, "cobblestone").replace("roof kind=shed", "roof slope_to=front kind=shed");
        let out = lowered(&src);
        let reasons: Vec<&str> = refusals(&out).iter().map(|d| d.primary.as_str()).collect();
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("minecraft:cobblestone") && r.contains("is not a stair")),
            "kind={kind}: expected the family refusal, got {reasons:?}",
        );
        assert!(
            !ids_carrying_properties(&out)
                .iter()
                .any(|id| id == "minecraft:cobblestone"),
            "kind={kind}: stair states must never land on a non-stair",
        );
        assert!(
            ids_carrying_properties(&out)
                .iter()
                .any(|id| id == STAIR_BASE_ID),
            "kind={kind}: the roof still gets built, out of the fallback species",
        );
    }
}

#[test]
fn a_sloped_roof_takes_any_stair_species_without_comment() {
    // Every roof species the registry pack ships is a stair
    // (`roof.dark_wood` → `dark_oak_stairs`); choosing one is the point
    // of binding a roof slot.
    let out = lowered(&roofed("gable", "dark_oak_stairs"));
    assert!(
        out.diagnostics.is_empty(),
        "a stair species is a legal roof material, got {:?}",
        out.diagnostics,
    );
    // `.all()` alone would also hold if the roof painted nothing, so
    // presence is pinned first.
    let stated = ids_carrying_properties(&out);
    assert!(!stated.is_empty(), "the roof must paint stairs");
    assert!(
        stated.iter().all(|id| id == "minecraft:dark_oak_stairs"),
        "the species must be the one bound, got {stated:?}",
    );
}

#[test]
fn a_roof_with_no_material_binding_keeps_the_fallback_silently() {
    let src = theme_t!(
        "w",
        "\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable overhang=1\n"
    );
    let out = lowered(src);
    assert!(out.diagnostics.is_empty(), "got {:?}", out.diagnostics);
    let stated = ids_carrying_properties(&out);
    assert!(!stated.is_empty(), "the roof must paint stairs");
    assert!(
        stated.iter().all(|id| id == STAIR_BASE_ID),
        "got {stated:?}",
    );
}

#[test]
fn a_roof_whose_binding_resolves_to_nothing_still_says_so() {
    // A distinct failure from an unusable material, with a distinct
    // fix: here the slot names nothing at all. Folding the two messages
    // together would send the author looking for the wrong thing.
    let src = theme_t!(
        "w",
        "\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=nosuchslot overhang=1\n"
    );
    let out = lowered(src);
    assert!(
        deferrals(&out)
            .iter()
            .any(|d| d.primary.contains("did not resolve")),
        "got {:?}",
        deferrals(&out),
    );
}

#[test]
fn a_roof_reading_a_slot_from_a_theme_that_never_bound_says_so_once() {
    // `geometry_material_id`'s doc claims this arm: the struct already
    // has `W_NO_THEME_BOUND` against it, and the member-level finding
    // is what says *which* member wore the fallback. A module with two
    // logical themes and no `place theme=` binds nothing.
    let src = "theme a:
  slot r -> @dark_oak_stairs

theme b:
  slot r -> @birch_stairs

struct s size=9x7
  roof kind=gable mat_slot=r overhang=1
";
    let out = lowered(src);
    assert!(
        deferrals(&out)
            .iter()
            .any(|d| d.primary.contains("did not resolve")),
        "got {:?}",
        out.diagnostics,
    );
    assert!(refusals(&out).is_empty(), "got {:?}", out.diagnostics);
    let stated = ids_carrying_properties(&out);
    assert!(
        !stated.is_empty() && stated.iter().all(|id| id == STAIR_BASE_ID),
        "the roof wears the fallback, got {stated:?}",
    );
}

#[test]
fn a_flat_roof_takes_any_block_because_it_attaches_no_states() {
    // Including a stair id: a flat roof paints `minecraft:oak_stairs`
    // bare, which is a valid blockstate — every property takes its
    // default. Requiring a family here would refuse a legal build.
    for material in ["stone_bricks", "oak_stairs"] {
        let out = lowered(&roofed("flat", material));
        assert!(
            deferrals(&out).is_empty(),
            "material={material}: {:?}",
            deferrals(&out),
        );
        assert!(
            palette_of(&out)
                .iter()
                .any(|e| e.id == format!("minecraft:{material}") && e.properties.is_empty()),
            "material={material}: the flat deck must be that block, bare",
        );
    }
}

#[test]
fn an_eave_stair_refuses_a_material_that_is_not_a_stair() {
    // A `stair` member takes its `facing` / `half` / `shape` from its
    // own arguments rather than from a slope, and attaches them to
    // whatever it paints just the same. The obligation follows the
    // states, not the member kind.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\
         \x20\x20slot trim -> @cobblestone\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=r overhang=1\n  \
         stair id=e kind=stairs mat_slot=trim side=front half=top shape=outer_left\n"
    );
    let out = lowered(src);
    assert!(
        refusals(&out)
            .iter()
            .any(|d| d.primary.contains("eave `stair`") && d.primary.contains("is not a stair")),
        "got {:?}",
        out.diagnostics,
    );
    assert!(
        !ids_carrying_properties(&out)
            .iter()
            .any(|id| id == "minecraft:cobblestone"),
    );
}

#[test]
fn an_eave_stair_with_no_binding_falls_back_to_the_stair_id() {
    // The fallback must be the stair constant: the plank one would put
    // `spruce_planks[facing=…]` in the palette, the shape this check
    // refuses. The roof binds a different species so the eave's
    // fallback is identifiable.
    let src = theme_t!(
        "w",
        "  slot r -> @dark_oak_stairs\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=r overhang=1\n  \
         stair id=e kind=stairs side=front half=top shape=outer_left\n"
    );
    let out = lowered(src);
    assert!(out.diagnostics.is_empty(), "got {:?}", out.diagnostics);
    let stated = ids_carrying_properties(&out);
    assert!(
        stated.iter().any(|id| id == STAIR_BASE_ID),
        "the eave wears the stair fallback, got {stated:?}",
    );
    assert!(
        stated.iter().all(|id| is_stair(id)),
        "nothing outside the family may carry stair states, got {stated:?}",
    );
}

#[test]
fn an_eave_stair_takes_any_stair_species_without_comment() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\
         \x20\x20slot trim -> @birch_stairs\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=r overhang=1\n  \
         stair id=e kind=stairs mat_slot=trim side=front half=top shape=outer_left\n"
    );
    let out = lowered(src);
    assert!(deferrals(&out).is_empty(), "got {:?}", deferrals(&out));
    assert!(
        ids_carrying_properties(&out)
            .iter()
            .any(|id| id == "minecraft:birch_stairs"),
        "got {:?}",
        ids_carrying_properties(&out),
    );
}

#[test]
fn a_binding_inside_the_family_keeps_its_id_and_loses_its_states() {
    // The geometry derives `facing` / `half` / `shape` and the binding
    // asked for its own; the id survives and the states do not, which
    // is a smaller thing than the material being the wrong shape and so
    // stays a warning.
    let src = theme_t!(
        "w",
        "  slot r -> @roof.fancy\n\n\
         struct s size=9x7\n  walls mat_slot=w height=4\n  \
         roof kind=gable mat_slot=r overhang=1\n"
    );
    let out = lowered_with_resolver(
        src,
        &StatefulResolver {
            token: "roof.fancy",
            id: "birch_stairs",
            properties: vec![("facing", "north")],
        },
    );
    assert!(
        deferrals(&out)
            .iter()
            .any(|d| d.primary.contains("also carried properties")),
        "got {:?}",
        out.diagnostics,
    );
    assert!(refusals(&out).is_empty(), "got {:?}", out.diagnostics);
    let stated = ids_carrying_properties(&out);
    assert!(
        stated.iter().all(|id| id == "minecraft:birch_stairs"),
        "the bound id survives, got {stated:?}",
    );
    // And the states are the geometry's, not the binding's: a gable
    // paints both slope facings, which one hardcoded `facing=north`
    // could not produce.
    let facings: std::collections::BTreeSet<String> = palette_of(&out)
        .iter()
        .filter_map(|e| e.properties.get("facing").cloned())
        .collect();
    assert!(
        facings.len() > 1,
        "the geometry's facings, not the binding's one: {facings:?}",
    );
}

#[test]
fn the_family_is_reported_before_the_properties_it_makes_moot() {
    // A source reaches this through a state literal on a block outside
    // the stair family — `slot r -> @cobblestone[facing=north]` bound
    // to a gable roof — and that literal is the only way in: the
    // registry pack answers `PackView::lookup` with `BlockState::bare`.
    // The state is built here rather than parsed so the test asks
    // about the precedence alone: once the id is refused it is not
    // painted, and reporting that its unused properties were also
    // dropped would ask the author to fix something that is not there.
    let mut properties = IndexMap::new();
    properties.insert("facing".to_owned(), "north".to_owned());
    let state = BlockState {
        id: "minecraft:cobblestone".to_owned(),
        properties,
    };
    let module = parse(&roofed("gable", "cobblestone")).expect("parse");
    let ir = lower(&module);
    let member = ir.structs[0]
        .members
        .iter()
        .find(|m| m.role == MemberRole::Roof)
        .expect("the roof member");
    // A real scope, so the payload carries the theme's slot value the
    // way it does in a build — `None` here would leave `token` empty
    // and quietly weaken every assertion below it.
    let resolution = resolve(&ir, None);
    let scope = resolution.scopes.get("struct::s").expect("scope");
    let mut diagnostics = Vec::new();
    let id = geometry_material_id(
        member,
        Some(scope),
        Some(&state),
        STAIR_BASE_ID,
        &GeometryMemberDescription {
            subject: "`gable` roof".to_owned(),
            states_from: "the geometry",
            requires_stair: true,
        },
        &mut diagnostics,
    );
    assert_eq!(id, STAIR_BASE_ID);
    assert_eq!(diagnostics.len(), 1, "one finding, got {diagnostics:?}");
    assert_eq!(
        diagnostics[0].code,
        DiagnosticCode::IncompatibleMaterial,
        "got {:?}",
        diagnostics[0],
    );
    // The payload is the whole reason this is an error rather than a
    // softer finding: it says where the material came from, so a
    // consumer can tell a source line from a pack mapping without
    // parsing the sentence (`spec/lint` "Machine-readable payload").
    let Some(DiagnosticData::IncompatibleMaterial {
        id,
        required,
        slot,
        token,
    }) = &diagnostics[0].data
    else {
        panic!("expected the payload, got {:?}", diagnostics[0].data);
    };
    assert_eq!(id, "minecraft:cobblestone");
    assert_eq!(required, "stair");
    assert_eq!(slot.as_deref(), Some("r"));
    assert_eq!(
        token.as_deref(),
        Some("cobblestone"),
        "the theme's slot value, so a dotted one reads as a pack mapping",
    );
}

#[test]
fn gable_roof_places_stairs_with_facing() {
    let src = "theme t:\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  walls mat_slot=r height=4\n  roof kind=gable mat_slot=r overhang=1\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    // Layer 0 of the roof sits at y=5. Ridge along x (long axis with
    // overhang dims.x=11, dims.z=9 → span=9 along z).
    let north_eave = block_state_at(ba, 0, 5, 0);
    assert_eq!(north_eave.id, "minecraft:spruce_stairs");
    assert_eq!(north_eave.properties.get("facing").unwrap(), "south");
    assert_eq!(north_eave.properties.get("half").unwrap(), "bottom");
    let south_eave = block_state_at(ba, 0, 5, 8);
    assert_eq!(south_eave.properties.get("facing").unwrap(), "north");
    // Apex: gable_extra_height(9) = 5 → y = 4 + 5 = 9, z = 4 (centre).
    let apex = block_state_at(ba, 0, 9, 4);
    assert_eq!(apex.properties.get("half").unwrap(), "top");
    assert_eq!(apex.properties.get("facing").unwrap(), "south");
}

#[test]
fn door_carves_opening_through_front_wall() {
    let src = theme_t!(
        "w",
        "\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=center\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    // Front wall at z=6 (no overhang). Center x = (9-1)/2 = 4. y=1,2.
    assert_eq!(block_id(ba, 4, 1, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 4, 2, 6), BlockState::AIR_ID);
    // Surrounding wall cells still cobblestone.
    assert_eq!(block_id(ba, 3, 1, 6), "minecraft:cobblestone");
    assert_eq!(block_id(ba, 4, 3, 6), "minecraft:cobblestone");
}

#[test]
fn door_at_left_carves_first_column_of_front_wall() {
    // `at=left` is wall-local u = 0. Front wall at z=6 (no overhang).
    let src = theme_t!(
        "w",
        "\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=left\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 0, 1, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 0, 2, 6), BlockState::AIR_ID);
    // The centre column, where `at=center` would cut, stays solid.
    assert_eq!(block_id(ba, 4, 1, 6), "minecraft:cobblestone");
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn door_at_right_carves_last_column_of_front_wall() {
    // `at=right` is `wall_length - 1`: front wall length 9 → x = 8.
    let src = theme_t!(
        "w",
        "\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=right\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 8, 1, 6), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 8, 2, 6), BlockState::AIR_ID);
    // Centre column stays solid.
    assert_eq!(block_id(ba, 4, 1, 6), "minecraft:cobblestone");
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn door_at_unknown_value_defers_with_named_anchors_in_note() {
    let src = theme_t!(
        "w",
        "\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=middle\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(deferred_count(&out), 1);
    // Front wall row remains intact.
    for x in 0..9 {
        assert_eq!(block_id(ba, x, 1, 6), "minecraft:cobblestone");
    }
    assert_defers_on(
        src,
        &out,
        "door side=front at=middle",
        "at=center | left | right",
    );
}

#[test]
fn window_places_glass_with_symmetry() {
    let src = theme_t!(
        "w",
        "  slot g -> @glass_pane\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  window side=front offset=2 y=2 size=2x2 sym=true mat_slot=g\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    // Front wall at z=6. Primary rectangle: x∈[2,4), y∈[2,4).
    for dx in 0..2 {
        for dy in 0..2 {
            assert_eq!(
                block_id(ba, 2 + dx, 2 + dy, 6),
                "minecraft:glass_pane",
                "primary ({},{})",
                2 + dx,
                2 + dy,
            );
        }
    }
    // Mirror: wall length = 9, mirror_offset = 9 - 2 - 2 = 5 → x∈[5,7).
    for dx in 0..2 {
        for dy in 0..2 {
            assert_eq!(
                block_id(ba, 5 + dx, 2 + dy, 6),
                "minecraft:glass_pane",
                "mirror ({},{})",
                5 + dx,
                2 + dy,
            );
        }
    }
}

#[test]
fn window_out_of_bounds_warns_and_skips() {
    let src = theme_t!(
        "w",
        "  slot g -> @glass_pane\n\nstruct s size=5x5\n  walls mat_slot=w height=4\n  window side=front offset=3 y=2 size=3x2 mat_slot=g\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    let deferred = deferred_count(&out);
    assert_eq!(deferred, 1);
    // Front wall at z=4 should retain cobblestone (no glass painted).
    for x in 0..5 {
        assert_eq!(block_id(ba, x, 2, 4), "minecraft:cobblestone");
    }
}

#[test]
fn unknown_roof_kind_warns_and_skips() {
    // dims.y stays `1 + wall_height`: an unknown kind adds 0 to
    // `max_roof_extra_height`.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=4\n  roof kind=pyramid mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(deferred_count(&out), 1);
    assert_eq!(ba.dims.y, 5);
}

#[test]
fn shed_roof_voxelises_with_slope_to_front() {
    // size=5x5, walls height=3, shed slope_to=front, no overhang.
    // slope_span = roof_h = 5 → extra_height = 5 → dims.y = 1 + 3 + 5 = 9.
    // Slope axis is z (Front=+z). Layer 0 (y=4) sits at z=0; apex
    // (y=8) sits at z=4. Stairs facing south.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=shed slope_to=front mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.y, 9);
    let layer0 = block_state_at(ba, 0, 4, 0);
    assert_eq!(layer0.id, "minecraft:spruce_stairs");
    assert_eq!(layer0.properties.get("facing").unwrap(), "south");
    assert_eq!(layer0.properties.get("half").unwrap(), "bottom");
    let apex = block_state_at(ba, 0, 8, 4);
    assert_eq!(apex.properties.get("half").unwrap(), "top");
}

#[test]
fn shed_roof_without_slope_to_emits_deferred_warning() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=shed mat_slot=r\n"
    );
    let out = lowered(src);
    assert_eq!(deferred_count(&out), 1);
    assert_defers_on(src, &out, "roof kind=shed mat_slot=r", "slope_to");
}

#[test]
fn hip_roof_voxelises_square_footprint() {
    // size=5x5, walls height=3. hip_extra_height = ceil(5/2) = 3.
    // dims.y = 1 + 3 + 3 = 7 (so highest valid y is 6). Apex sits at
    // y = wall_top + extra_height = 6, single cell at (2, 6, 2).
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=hip mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.y, 7);
    let apex = block_state_at(ba, 2, 6, 2);
    assert_eq!(apex.id, "minecraft:spruce_stairs");
    assert_eq!(apex.properties.get("half").unwrap(), "top");
    // North-west corner of layer 0 uses `shape=outer_left`.
    let nw_corner = block_state_at(ba, 0, 4, 0);
    assert_eq!(nw_corner.properties.get("shape").unwrap(), "outer_left");
    assert_eq!(nw_corner.properties.get("facing").unwrap(), "south");
}

#[test]
fn flat_roof_voxelises_single_layer_of_planks() {
    // size=5x5, walls height=3, flat → extra_height=1, dims.y = 5.
    // Every cell at y=4 is spruce_planks.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_planks\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.y, 5);
    for z in 0..5 {
        for x in 0..5 {
            assert_eq!(block_id(ba, x, 4, z), "minecraft:spruce_planks");
        }
    }
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn flat_roof_honours_bound_mat_slot_id() {
    // A flat deck attaches no blockstates, so it requires nothing of
    // the block it names: the binding lands in the palette verbatim,
    // including a stair id, which is then simply a stair in its
    // default state. `FLAT_BASE_ID` is what a deck falls back to with
    // no binding, not a species it is held to.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected diagnostics: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    // Flat deck sits at wall_top + 1 = 4. Interior x∈[0, 4], z∈[0, 4].
    assert_eq!(block_id(ba, 2, 4, 2), "minecraft:spruce_stairs");
}

fn block_state_at(ba: &BlockArray, x: u32, y: u32, z: u32) -> &BlockState {
    let i = ba.dims.index(x, y, z).expect("in-range coord");
    let pi = ba.voxels[i];
    &ba.palette.entries[usize::from(pi.0)]
}

// --- an opening that cannot be cut says so ------------------------------

#[test]
fn door_without_side_emits_deferred_warning() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  door at=center\n"
    );
    let out = lowered(src);
    assert_eq!(deferred_count(&out), 1);
    assert_defers_on(src, &out, "door at=center", "missing `side=");
}

#[test]
fn window_with_non_ident_side_emits_deferred_warning() {
    // `side=` present but an integer: `side_of`'s wrong-type arm.
    let src = theme_t!(
        "w",
        "  slot g -> @glass_pane\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  window side=3 offset=1 y=1 size=1x1 mat_slot=g\n"
    );
    let out = lowered(src);
    let deferred = deferred_count(&out);
    assert!(deferred >= 1, "expected a side= diagnostic, got {deferred}");
    assert_defers_on(
        src,
        &out,
        "window side=3 offset=1 y=1 size=1x1 mat_slot=g",
        "`side=`",
    );
}

#[test]
fn sym_window_overlap_skips_mirror_with_warning() {
    // wall length=6, offset=2, size=3 → mirror_offset = 6-2-3 = 1.
    // [2..5) and [1..4) overlap.
    let src = theme_t!(
        "w",
        "  slot g -> @glass_pane\n\nstruct s size=6x5\n  walls mat_slot=w height=4\n  window side=front offset=2 y=2 size=3x1 sym=true mat_slot=g\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_defers_on(
        src,
        &out,
        "window side=front offset=2 y=2 size=3x1 sym=true mat_slot=g",
        "overlap",
    );
    // Primary rectangle [x=2..5, y=2] painted.
    for x in 2..5 {
        assert_eq!(block_id(ba, x, 2, 4), "minecraft:glass_pane");
    }
    // Mirror cells outside the primary stay cobblestone (x=1).
    assert_eq!(block_id(ba, 1, 2, 4), "minecraft:cobblestone");
}

#[test]
fn a_sym_window_that_coincides_with_its_mirror_is_reported() {
    // `spec/syntax` "Selectors": a mirror overlapping the primary is
    // `W_DEFERRED_MEMBER`. A centred window's mirror is the same
    // rectangle, so the author asked for two windows and got one.
    // All three centred shapes on a 5-wide wall, the solutions of
    // 2*offset + width == 5: mirror_offset = 5 - 2 - 1 = 2,
    // 5 - 1 - 3 = 1 and 5 - 0 - 5 = 0. The last puts `mirror_offset`
    // at 0, the floor its `saturating_sub` clamps to.
    for (offset, width) in [(2, 1), (1, 3), (0, 5)] {
        let src = format!(
            "{}  slot g -> @glass_pane\n\n\
             struct s size=5x5\n  walls mat_slot=w height=3\n  \
             window side=front offset={offset} y=1 size={width}x1 sym=true mat_slot=g\n",
            theme_t!("w", ""),
        );
        let out = lowered(&src);
        let found: Vec<_> = out
            .diagnostics
            .iter()
            .map(|d| (d.code, d.primary.as_str()))
            .collect();
        assert_eq!(
            found,
            vec![(
                DiagnosticCode::DeferredMember,
                format!(
                    "`sym=true` window at offset={offset} size={width}x1 on the `front` wall \
                     coincides with its mirror (wall length=5); the mirror was skipped"
                )
                .as_str(),
            )],
        );
        // The primary is still painted.
        let ba = out.structures.get("struct::s").unwrap();
        for x in offset..offset + width {
            assert_eq!(block_id(ba, x, 1, 4), "minecraft:glass_pane", "x={x}");
        }
    }
}

// The one-row course under a roof, and the struct with no walls at
// all, live in `tests/lower/door_wall_fit.rs`, which holds each to its own
// sentence: a door with no wall anywhere and a door with no wall at its
// row are two findings, and a check for "walls" in the message would
// pass for either.

#[test]
fn at_center_picks_right_of_centre_on_even_width_walls() {
    // size=8x5 → wall length 8. `at=center` should pick column 4 (the
    // right half-block of the geometric centre), not column 3, so the
    // door is consistent with round-half-up semantics.
    let src = theme_t!(
        "w",
        "\nstruct s size=8x5\n  walls mat_slot=w height=3\n  door side=front at=center\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    // Front wall z=4. y=1 air at x=4, cobblestone at x=3.
    assert_eq!(block_id(ba, 4, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 3, 1, 4), "minecraft:cobblestone");
}

#[test]
fn gable_honours_bound_mat_slot_id() {
    // A binding inside the stair family is used verbatim, which is what
    // choosing a species means.
    let src = theme_t!(
        "w",
        "  slot r -> @oak_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected diagnostics: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert!(
        ba.palette
            .entries
            .iter()
            .any(|s| s.id == "minecraft:oak_stairs"),
        "expected oak_stairs in palette, got {:?}",
        ba.palette
            .entries
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
    );
}

#[test]
fn gable_with_matching_mat_slot_stays_silent() {
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "expected silence on matching mat_slot, got {:?}",
        out.diagnostics,
    );
}

#[test]
fn even_span_gable_apex_uses_half_top() {
    // size=8x4 → roof span (short axis) = 4 (even). The apex layer
    // must cap with two half=top rows or the ridge has an open V.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=8x4\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    // gable_extra_height(4) = 2 layers. Apex layer at y = 4+2 = 6.
    let apex_low = block_state_at(ba, 0, 6, 1);
    let apex_high = block_state_at(ba, 0, 6, 2);
    assert_eq!(apex_low.properties.get("half").unwrap(), "top");
    assert_eq!(apex_high.properties.get("half").unwrap(), "top");
}

/// Pruning must not become the place generator bugs go to be tidied
/// away. A slot a write named and a later phase covered is the
/// layering working; a slot no write ever named is a generator
/// interning a material for geometry it does not emit, and it stays in
/// the palette where the artifact, `cairn info`, and
/// `tests/lower/palette_is_referenced.rs` can all still see it. The caller
/// turns the returned list into a `debug_assert!`, which is why this
/// asserts on the list rather than on a panic — the behaviour under
/// test is what a release build does.
#[test]
fn a_slot_no_write_ever_named_is_kept_rather_than_swept_out() {
    let mut palette = Palette::new_with_air();
    palette.intern(BlockState::bare("minecraft:cobblestone"));
    let canvas = Canvas::new(Dims { x: 1, y: 1, z: 1 }, vec![None]);

    let (voxels, never_painted) = prune_unreferenced(&mut palette, &canvas);
    assert_eq!(never_painted, [1]);
    assert_eq!(
        palette
            .entries
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["minecraft:air", "minecraft:cobblestone"],
    );
    assert_eq!(voxels, [PaletteIndex::AIR]);
}

/// The other half of the pair: the same unreferenced slot, this time
/// reached by a write that a later one covered, is dropped without a
/// word and the survivors renumber onto the gap.
#[test]
fn a_slot_a_later_write_covered_is_pruned_and_the_rest_renumber() {
    let mut palette = Palette::new_with_air();
    let covered = palette.intern(BlockState::bare("minecraft:glass_pane"));
    let winner = palette.intern(BlockState::bare("minecraft:cobblestone"));
    let mut canvas = Canvas::new(Dims { x: 1, y: 1, z: 1 }, vec![Some(Phase::Massing)]);
    canvas.for_member(0).paint((0, 0, 0), || covered);
    canvas.for_member(0).paint((0, 0, 0), || winner);

    let (voxels, never_painted) = prune_unreferenced(&mut palette, &canvas);
    assert_eq!(never_painted, Vec::<usize>::new());
    assert_eq!(
        palette
            .entries
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["minecraft:air", "minecraft:cobblestone"],
    );
    assert_eq!(voxels, [PaletteIndex(1)]);
}

/// The clip in [`MemberCanvas::paint`] is unreachable from source — every
/// generator's coordinates come from the dims the volume was built
/// against — so the only way to show that it fails loud rather than
/// dropping the write is to call it with the disagreement it exists to
/// catch. Debug-only because that is where `debug_assert!` lives; a
/// release build clips and carries on.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "the dim math and the generator disagree")]
fn painting_outside_the_volume_is_not_a_silent_drop() {
    let mut canvas = Canvas::new(Dims { x: 2, y: 2, z: 2 }, vec![Some(Phase::Massing)]);
    canvas.for_member(0).paint((2, 0, 0), || PaletteIndex(1));
}

#[test]
fn even_span_gable_apex_rows_face_away_from_the_ridge() {
    // The two apex faces differ only in `facing`, so a `half=top` check
    // cannot tell them apart, and one wrong arm in the face → state
    // mapping would collapse the two rows into one entry.
    //
    // The ridge runs between z=1 and z=2, so the low row points -z and
    // the high row +z: each cap turns its open half under the ridge.
    // Pointing them inward instead leaves that open half on the outer
    // face, a 0.5 x 0.5 undercut running the length of the roof.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=8x4\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    let apex_low = block_state_at(ba, 0, 6, 1);
    let apex_high = block_state_at(ba, 0, 6, 2);
    assert_eq!(apex_low.properties.get("facing").unwrap(), "north");
    assert_eq!(apex_high.properties.get("facing").unwrap(), "south");
}

// ---- site lowering: per-place IR emission and the coord solver ----

#[test]
fn place_lowers_def_with_referenced_theme() {
    // The def has no theme of its own; `place ... theme=t` supplies
    // `t`'s slots, and the result is keyed `site::s::home1`, not
    // `struct::cottage`.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=home1 use=cottage theme=t at=origin\n",
    );
    let out = lowered(src);
    let ba = out
        .structures
        .get("site::s::home1")
        .expect("place lowered under site::s::home1 key");
    assert_eq!(
        ba.dims,
        Dims { x: 3, y: 3, z: 3 },
        "place inherits the def's interior size, no overhang",
    );
    // Wall voxel at the corner should be cobblestone (theme slot
    // resolved across scopes).
    assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");

    let placement = out
        .placements
        .get("site::s::home1")
        .expect("placement record present");
    assert_eq!(placement.site, "s");
    assert_eq!(placement.place_id, "home1");
    assert_eq!(placement.source_def, "cottage");
    assert_eq!(placement.theme, "t");
    assert_eq!(placement.origin, (0, 0, 0));
    assert_eq!(placement.dims, ba.dims);
}

#[test]
fn east_of_offset_sums_prior_dims_and_gap() {
    // east_of advances along +x past the prior placement's full inflated
    // dims.x (no overhang here, so just the interior 3) plus gap=2.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t east_of=a gap=2\n",
    );
    let out = lowered(src);
    let b = out
        .placements
        .get("site::s::b")
        .expect("placement b present");
    assert_eq!(
        b.origin,
        (5, 0, 0),
        "x = prior.x(0) + prior.dims.x(3) + gap(2)"
    );
    assert_eq!(b.origin.2, 0, "east_of does not move along z");
}

#[test]
fn north_of_subtracts_dims_and_gap_on_z_axis() {
    // north_of retreats along -z by the new placement's full inflated
    // dims.z plus gap. Both cottages are 3 deep, so this pins the
    // arithmetic, not whose depth it reads — the unequal-depth tests
    // below do that. Front-is-+z (`spec/syntax` "Selectors" and
    // `spec/components-editing-sites` "Multi-building with `site`") means
    // north sits at the negative-z half-space.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def cottage size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=a use=cottage theme=t at=origin\n",
        "  place id=b use=cottage theme=t north_of=a gap=4\n",
    );
    let out = lowered(src);
    let b = out
        .placements
        .get("site::s::b")
        .expect("placement b present");
    assert_eq!(
        b.origin,
        (0, 0, -7),
        "z = prior.z(0) - new.dims.z(3) - gap(4)"
    );
}

/// A two-place site with `a` built from `a_def` and `b` placed next to
/// it with `selector=a gap=gap`.
///
/// Each pair a test draws from here differs on exactly the axis its
/// selector reads: `small` (3x3) against `deep` (3x9) on `z` for
/// `north_of`, `small` against `wide` (9x3) on `x` for `east_of`.
/// `eaved` is `deep` with a roof of `overhang=1`, so it lowers to 5x11:
/// its `dims` differ from its `size=`, which is what tells an origin
/// taken from the lowered body apart from one taken from `size=` (known
/// before lowering, and wrong whenever a roof overhangs).
fn unequal_pair(a_def: &str, b_def: &str, selector: &str, gap: i32) -> BlockArrayIr {
    lowered(&format!(
        concat!(
            "def small size=3x3:\n",
            "  floor id=f mat_slot=floor\n",
            "  walls id=w mat_slot=wall height=2\n",
            "\n",
            "def deep size=3x9:\n",
            "  floor id=f mat_slot=floor\n",
            "  walls id=w mat_slot=wall height=2\n",
            "\n",
            "def eaved size=3x9:\n",
            "  floor id=f mat_slot=floor\n",
            "  walls id=w mat_slot=wall height=2\n",
            "  roof  id=r kind=flat mat_slot=wall overhang=1\n",
            "\n",
            "def wide size=9x3:\n",
            "  floor id=f mat_slot=floor\n",
            "  walls id=w mat_slot=wall height=2\n",
            "\n",
            "theme t:\n",
            "  slot floor -> @oak_planks\n",
            "  slot wall  -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a use={a_def} theme=t at=origin\n",
            "  place id=b use={b_def} theme=t {selector}=a gap={gap}\n",
        ),
        a_def = a_def,
        b_def = b_def,
        selector = selector,
        gap = gap,
    ))
}

#[test]
fn north_of_leaves_exactly_gap_rows_between_unequal_depths() {
    // `gap` counts the empty rows between `b`'s +z face and `a`'s -z
    // face, whichever of the two is deeper. Stepping back by the prior
    // placement's depth instead buries a small `a` inside a deep `b`
    // at gap=0 and pushes a small `b` six rows too far behind a deep
    // `a`. The `eaved` row fails if the step is taken from `size=`
    // rather than the lowered dims.
    //
    // `b_z` is `b`'s origin at gap=0: `-new.dims.z`, since `a` sits at 0.
    for (a_def, b_def, b_z) in [
        ("small", "deep", -9),
        ("deep", "small", -3),
        ("small", "eaved", -11),
    ] {
        for gap in [0, 3] {
            let out = unequal_pair(a_def, b_def, "north_of", gap);
            let a = out.placements.get("site::s::a").expect("placement a");
            let b = out.placements.get("site::s::b").expect("placement b");
            // The premise: without it, the gap check below holds for
            // either reading.
            assert_ne!(a.dims.z, b.dims.z, "a={a_def}, b={b_def}");
            assert_eq!(
                b.origin,
                (0, 0, b_z - gap),
                "a={a_def}, b={b_def}, gap={gap}"
            );
            let b_front = b.origin.2 + i32::try_from(b.dims.z).unwrap();
            assert_eq!(
                a.origin.2 - b_front,
                gap,
                "a={a_def} at z {}..{}, b={b_def} at z {}..{}, gap={gap}",
                a.origin.2,
                a.origin.2 + i32::try_from(a.dims.z).unwrap(),
                b.origin.2,
                b_front,
            );
        }
    }
}

#[test]
fn east_of_leaves_exactly_gap_columns_between_unequal_widths() {
    // `east_of` is not the mirror image of `north_of`, and this pins it
    // against being "symmetrised" onto the new body's dims to match. It
    // moves past the prior placement's width, so the step is the
    // prior's lowered `dims.x` — `eaved` is 5 wide, not its `size=` 3.
    //
    // `b_x` is `b`'s origin at gap=0: `prior.dims.x`, since `a` sits at 0.
    for (a_def, b_def, b_x) in [
        ("small", "wide", 3),
        ("wide", "small", 9),
        ("eaved", "small", 5),
    ] {
        for gap in [0, 3] {
            let out = unequal_pair(a_def, b_def, "east_of", gap);
            let a = out.placements.get("site::s::a").expect("placement a");
            let b = out.placements.get("site::s::b").expect("placement b");
            assert_ne!(a.dims.x, b.dims.x, "a={a_def}, b={b_def}");
            assert_eq!(
                b.origin,
                (b_x + gap, 0, 0),
                "a={a_def}, b={b_def}, gap={gap}"
            );
            let a_east = a.origin.0 + i32::try_from(a.dims.x).unwrap();
            assert_eq!(b.origin.0 - a_east, gap, "a={a_def}, b={b_def}, gap={gap}");
        }
    }
}

fn village_pair_source(extra_connects: &str) -> String {
    format!(
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=4\n",
            "{}",
        ),
        extra_connects,
    )
}

#[test]
fn duplicate_connect_emits_w_duplicate_walkway_and_lays_one_strip() {
    let src = village_pair_source(
        "  connect a.entry to b.entry path=@gravel\n  connect a.entry to b.entry path=@gravel\n",
    );
    let out = lowered(&src);
    let dup_count = findings(&out, DiagnosticCode::DuplicateWalkway).len();
    assert_eq!(dup_count, 1, "expected exactly one W_DUPLICATE_WALKWAY");
    assert_eq!(out.walkways.len(), 1, "second row must not lay a strip");
}

#[test]
fn reverse_connect_dedupes_against_first_row() {
    // `a.entry → b.entry` and `b.entry → a.entry` are one walkway.
    let src = village_pair_source(
        "  connect a.entry to b.entry path=@gravel\n  connect b.entry to a.entry path=@gravel\n",
    );
    let out = lowered(&src);
    let dup_count = findings(&out, DiagnosticCode::DuplicateWalkway).len();
    assert_eq!(
        dup_count, 1,
        "expected exactly one W_DUPLICATE_WALKWAY on the reversed row",
    );
    assert_eq!(
        out.walkways.len(),
        1,
        "reversed row must not lay a second strip"
    );
}

#[test]
fn a_row_refused_after_the_dedup_check_leaves_its_pair_to_the_next_row() {
    // The first row's `path=` is an abstract token, which lowers to
    // nothing without a registry pack, so that row lays no strip. The
    // second row names the same pair and a concrete block. It is not
    // a duplicate of a walkway that was never laid: it lays the strip,
    // and the only finding is the first row's own deferral.
    let src = village_pair_source(
        "  connect a.entry to b.entry path=@path.gravel\n  connect b.entry to a.entry path=@gravel\n",
    );
    let out = lowered(&src);
    let codes: Vec<DiagnosticCode> = out.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        [DiagnosticCode::AbstractTokenDeferred],
        "{:#?}",
        out.diagnostics
    );
    let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
    assert_eq!(laid, ["walkway::s::b.entry__a.entry"]);
    assert_eq!(out.walkways[0].path_material, "minecraft:gravel");
}

/// The `W_INVALID_WALKWAY_IDENT` findings on `out`, as `(primary, notes)`.
fn invalid_walkway_ident_findings(out: &BlockArrayIr) -> Vec<(String, Vec<String>)> {
    findings(out, DiagnosticCode::InvalidWalkwayIdent)
        .into_iter()
        .map(|d| {
            (
                d.primary.clone(),
                d.notes.iter().map(|n| n.message.clone()).collect(),
            )
        })
        .collect()
}

#[test]
fn walkway_ids_with_an_edge_underscore_are_named_rather_than_aliased() {
    // `a.p_ → b.p` and `a.p → _b.p` differ as `(from, to)` pairs, so
    // `W_DUPLICATE_WALKWAY` has nothing to say, but a `_` at the edge
    // of `p_` or `_b` merges into the `__` separator: both would
    // encode to `walkway::s::a.p___b.p`, and the second row would
    // replace the first in the structure map with no finding. Each
    // row must instead be dropped with a finding naming its own
    // segment. The third row is sound and must still lay, so the
    // test cannot pass by dropping every walkway.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def hut size=5x5:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=p  side=front at=center\n",
        "  door  id=p_ side=back  at=center\n",
        "\n",
        "site s:\n",
        "  place id=a  use=hut theme=t at=origin\n",
        "  place id=b  use=hut theme=t east_of=a gap=4\n",
        "  place id=_b use=hut theme=t north_of=a gap=4\n",
        "  connect a.p_ to b.p path=@gravel\n",
        "  connect a.p to _b.p path=@gravel\n",
        "  connect a.p to b.p path=@gravel\n",
    );
    let out = lowered(src);
    let findings = invalid_walkway_ident_findings(&out);
    let primaries: Vec<&str> = findings.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        primaries,
        [
            "walkway `a.p_ ↔ b.p` was dropped because the port id `p_` starts or ends \
             with `_`",
            "walkway `a.p ↔ _b.p` was dropped because the place id `_b` starts or ends \
             with `_`",
        ],
    );
    let notes: Vec<&[String]> = findings.iter().map(|(_, n)| n.as_slice()).collect();
    let rule = "a walkway's place and port ids may not start or end with `_` in either \
                position, so the rule does not depend on which way the row is written";
    assert_eq!(
        notes,
        [
            [format!(
                "{rule}; rename the port so it neither starts nor ends with `_`"
            )],
            [format!(
                "{rule}; rename the place so it neither starts nor ends with `_`"
            )],
        ],
    );
    let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
    assert_eq!(laid, ["walkway::s::a.p__b.p"]);
    let walkway_structures: Vec<&str> = out
        .structures
        .keys()
        .filter(|k| k.starts_with("walkway::"))
        .map(String::as_str)
        .collect();
    assert_eq!(walkway_structures, ["walkway::s::a.p__b.p"]);
}

#[test]
fn a_port_named_underscore_is_named_rather_than_lowered_to_an_unparseable_key() {
    // A port called `_` gives `walkway::s::a.___b._`, which splits
    // back at the first `__` into an empty port, a key the artifact
    // namer cannot name. The row must be dropped at lowering with a
    // finding naming `_`, beside a sound row that still lays.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def hut size=5x5:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=_ side=front at=center\n",
        "  door  id=q side=back  at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t east_of=a gap=4\n",
        "  connect a._ to b._ path=@gravel\n",
        "  connect a.q to b.q path=@gravel\n",
    );
    let out = lowered(src);
    let findings = invalid_walkway_ident_findings(&out);
    let primaries: Vec<&str> = findings.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        primaries,
        [
            "walkway `a._ ↔ b._` was dropped because the port id `_` starts or ends \
          with `_`"
        ],
    );
    let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
    assert_eq!(laid, ["walkway::s::a.q__b.q"]);
    for key in out.walkways.keys() {
        assert_eq!(
            WalkwayScopeKey::parse(key.as_str()).as_ref(),
            Ok(key),
            "every laid walkway's key must parse back to itself",
        );
    }
}

#[test]
fn a_walkway_id_containing_dunder_is_named_with_the_dunder_repair() {
    // The `__` arm of the same finding:
    // its note names the `__` repair, not the edge-`_` one.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def hut size=5x5:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=b__c side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t east_of=a gap=4\n",
        "  connect a.b__c to b.b__c path=@gravel\n",
    );
    let out = lowered(src);
    let findings = invalid_walkway_ident_findings(&out);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let (primary, notes) = &findings[0];
    assert_eq!(
        primary,
        "walkway `a.b__c ↔ b.b__c` was dropped because the port id `b__c` contains \
         `__`, the separator between the walkway scope key's `from` and `to` halves",
    );
    assert_eq!(
        notes,
        &[
            "a walkway's site, place and port ids may not contain `__`, so no id can be \
             mistaken for the separator; rename the port so it has no `__`, e.g. by \
             replacing `__` with `_`"
        ],
    );
    assert!(out.walkways.is_empty());
}

fn walkway_with_blocked_l_path_source() -> &'static str {
    // Two `home` placements wired back-to-back so the straight L-path
    // between their ports threads through `b`'s floor. `a` exposes a
    // `side=back` door (port sits at world z = -1), `b` exposes
    // `side=front` (port sits at world z = 3). With `east_of=a gap=2`
    // `b` lands at origin (5, 0, 0) with interior 3x3, so the L-path
    // `(1, -1) -> (6, -1) -> (6, 3)` would pass through (6, 0),
    // (6, 1), (6, 2) — the middle column of `b`'s 3×3 floor.
    // A detour through the open x∈[3,4] gap between the two floors
    // exists, so the router must find it.
    concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def back_home size=3x3:\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=back side=back at=center\n",
        "\n",
        "def front_home size=3x3:\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=front side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=back_home theme=t at=origin\n",
        "  place id=b use=front_home theme=t east_of=a gap=2\n",
        "  connect a.back to b.front path=@gravel\n",
    )
}

#[test]
fn walkway_routes_around_obstructed_l_path_without_warning() {
    // The straight L crosses `b`'s floor but a detour exists, so the
    // strip is laid whole and nothing is reported.
    let out = lowered(walkway_with_blocked_l_path_source());
    assert!(
        findings(&out, DiagnosticCode::WalkwayBlocked).is_empty(),
        "detourable row must not warn, got {:?}",
        out.diagnostics,
    );
    let walkway = out
        .walkways
        .get("walkway::s::a.back__b.front")
        .expect("walkway IR present");
    // Both minimal detours (via the x=3 or x=4 column) share the
    // L-path's bounding box, so origin/footprint are pinned exactly
    // even though the tie-break picks one column.
    assert_eq!(walkway.origin, (1, 0, -1));
    assert_eq!(walkway.footprint, Footprint { x: 6, z: 5 });
    let ba = out
        .structures
        .get("walkway::s::a.back__b.front")
        .expect("walkway block array present");
    // The detour is a shortest route: Manhattan distance 9 → 10
    // cells, all gravel, none skipped.
    let gravel_count = (0..ba.dims.volume())
        .filter(|&i| ba.palette.entries[usize::from(ba.voxels[i].0)].id == "minecraft:gravel")
        .count();
    assert_eq!(gravel_count, 10, "shortest detour lays 10 gravel cells");
    // Endpoints are gravel; the cells the L would have crossed on
    // `b`'s floor edge (world (6, 0..=2) → local (5, 1..=3)) stay
    // air because the route went around them.
    assert_eq!(block_id(ba, 0, 0, 0), "minecraft:gravel");
    assert_eq!(block_id(ba, 5, 0, 4), "minecraft:gravel");
    for dz in 1..=3 {
        assert_eq!(
            block_id(ba, 5, 0, dz),
            BlockState::AIR_ID,
            "floor cell at local (5, 0, {dz}) must stay untouched",
        );
    }
}

#[test]
fn walkway_detour_is_deterministic_across_lowerings() {
    // The router breaks ties by fixed expansion order, never by hash
    // iteration order — two lowerings of the same source must produce
    // the identical voxel grid (the lockfile pins walkway placement,
    // so a wobbling tie-break would break reproducible builds).
    let first = lowered(walkway_with_blocked_l_path_source());
    let second = lowered(walkway_with_blocked_l_path_source());
    assert_eq!(
        first.structures.get("walkway::s::a.back__b.front"),
        second.structures.get("walkway::s::a.back__b.front"),
    );
}

fn walkway_with_unroutable_port_source() -> &'static str {
    // Same back-to-back pair as
    // `walkway_with_blocked_l_path_source`, plus a third placement
    // `c` stacked directly north of `a` (`gap=0` → origin
    // (0, 0, -3)) whose floor covers `a`'s back port cell (1, -1).
    // A route cannot leave a buried port, so the row must fall back
    // to the L-path skip-and-warn lay. The L crosses `c`'s floor at
    // (1, -1) and (2, -1) and `b`'s floor at (6, 0..=2) — 5 skipped
    // cells.
    concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def back_home size=3x3:\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=back side=back at=center\n",
        "\n",
        "def front_home size=3x3:\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=front side=front at=center\n",
        "\n",
        "site s:\n",
        "  place id=a use=back_home theme=t at=origin\n",
        "  place id=b use=front_home theme=t east_of=a gap=2\n",
        "  place id=c use=front_home theme=t north_of=a gap=0\n",
        "  connect a.back to b.front path=@gravel\n",
    )
}

#[test]
fn walkway_blocked_cells_skip_with_w_walkway_blocked_count() {
    // `a`'s port is buried under `c`'s floor, so no route exists and the
    // row lays the L, skipping cells. The count matters: an obstruction
    // swallowed silently would leave the lockfile claiming voxels the
    // `.nbt` does not carry.
    let out = lowered(walkway_with_unroutable_port_source());
    let blocked = findings(&out, DiagnosticCode::WalkwayBlocked);
    assert_eq!(
        blocked.len(),
        1,
        "expected one W_WALKWAY_BLOCKED, got {:?}",
        out.diagnostics,
    );
    assert_eq!(
        blocked[0].data,
        Some(DiagnosticData::WalkwayBlocked { skipped: 5 }),
        "expected the structured payload to report five skipped cells, got data={:?} primary={}",
        blocked[0].data,
        blocked[0].primary,
    );
    // The note must name the actual cause — `a.back` is buried
    // under `c`'s floor — not fall back to the generic
    // widen-the-gap suggestion, which cannot fix a buried port.
    let note = &blocked[0].notes[0].message;
    assert!(
        note.contains("port `a.back` is buried"),
        "note must point at the buried port, got {note}",
    );
    // The primary is the text-format contract humans and text-matching
    // harnesses read; `data` augments it rather than replacing it, so
    // both carry the count.
    assert!(
        blocked[0].primary.contains("skipped 5 cells"),
        "primary text contract must still name the skip count, got {}",
        blocked[0].primary,
    );
    // The rendered JSON is what downstream tooling (an LSP quick-fix, a
    // CI annotator) reads; a caller that wraps `lower_to_block_array` in
    // the JSON formatter gets this shape.
    let source = walkway_with_unroutable_port_source();
    let lines = crate::lines::LineStarts::new(source);
    let rendered = blocked[0].render(source, &lines);
    let value = serde_json::to_value(&rendered).expect("rendered serialises");
    assert_eq!(
        value["data"],
        serde_json::json!({"kind": "walkway_blocked", "skipped": 5}),
        "JSON `data` payload must match the structured contract, got {value}",
    );
    // Every other finding leaves `data` unset; the checks above look at
    // the `W_WALKWAY_BLOCKED` entry alone.
    for d in &out.diagnostics {
        if d.code != DiagnosticCode::WalkwayBlocked {
            assert!(
                d.data.is_none(),
                "code {:?} unexpectedly carries a payload: {:?}",
                d.code,
                d.data,
            );
        }
    }
    // Walkway IR still emitted — the row survives, only the colliding
    // cells stay air. Bounding box covers x∈[1,6], z∈[-1,3].
    let walkway = out
        .walkways
        .get("walkway::s::a.back__b.front")
        .expect("walkway IR present despite collisions");
    assert_eq!(walkway.origin, (1, 0, -1));
    assert_eq!(walkway.footprint, Footprint { x: 6, z: 5 });
    let ba = out
        .structures
        .get("walkway::s::a.back__b.front")
        .expect("walkway block array present despite collisions");
    // Path corner at (6, -1) is still gravel.
    assert_eq!(block_id(ba, 5, 0, 0), "minecraft:gravel");
    // The buried port cell and its neighbour under `c`'s floor
    // (world (1, -1), (2, -1) → local (0, 0), (1, 0)) stay air.
    for dx in 0..=1 {
        assert_eq!(
            block_id(ba, dx, 0, 0),
            BlockState::AIR_ID,
            "blocked cell at local ({dx}, 0, 0) should stay air",
        );
    }
    // Cells colliding with `b`'s floor (world (6,0), (6,1), (6,2))
    // map to local (5, 1), (5, 2), (5, 3) — they must stay air.
    for dz in 1..=3 {
        assert_eq!(
            block_id(ba, 5, 0, dz),
            BlockState::AIR_ID,
            "blocked cell at local (5, 0, {dz}) should stay air",
        );
    }
    // Endpoint at b's port (world (6, 3) → local (5, 4)) remains
    // gravel — the port itself sits outside `b`'s floor.
    assert_eq!(block_id(ba, 5, 0, 4), "minecraft:gravel");
}

/// The note of the one `W_WALKWAY_BLOCKED` `source` lowers to.
fn walkway_blocked_note_of(source: &str) -> String {
    let out = lowered(source);
    let blocked = findings(&out, DiagnosticCode::WalkwayBlocked);
    assert_eq!(blocked.len(), 1, "diagnostics={:?}", out.diagnostics);
    assert_eq!(blocked[0].notes.len(), 1, "{:?}", blocked[0]);
    blocked[0].notes[0].message.clone()
}

#[test]
fn a_port_on_its_own_placements_pressure_plate_is_not_blamed_on_another_floor() {
    // Each hut lays a pressure plate on the cell in front of its own
    // door, which is that door's port cell. No other placement covers
    // either port, so pulling the placements apart cannot help.
    let note = walkway_blocked_note_of(concat!(
        "def hut size=5x5:\n",
        "  floor id=f mat_slot=wall\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  roof  kind=flat mat_slot=wall overhang=1\n",
        "  door  id=e side=front at=center\n",
        "  pressure_plate id=pp at=front.outside offset=2 y=0\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t east_of=a gap=4\n",
        "  connect a.e to b.e path=@gravel\n",
    ));
    assert_eq!(
        note,
        "port `a.e` sits on the `minecraft:oak_pressure_plate` its own placement `a` lays \
         on that cell; move that block or the door/window; port `b.e` sits on the \
         `minecraft:oak_pressure_plate` its own placement `b` lays on that cell; move that \
         block or the door/window",
    );
}

#[test]
fn a_port_buried_by_another_sites_placement_names_that_site() {
    // `u`'s `c` sits where `walkway_with_unroutable_port_source`'s
    // `c` does — over `a`'s back port — but in another site, so its
    // bare id would name nothing in site `s`.
    let source = walkway_with_unroutable_port_source()
        .replace("  place id=c use=front_home theme=t north_of=a gap=0\n", "")
        + concat!(
            "\n",
            "site u:\n",
            "  place id=o use=front_home theme=t at=origin\n",
            "  place id=c use=front_home theme=t north_of=o gap=0\n",
        );
    let note = walkway_blocked_note_of(&source);
    assert_eq!(
        note,
        "port `a.back` is buried inside the floor of placement `c` in site `u`; move that \
         door/window to an unobstructed wall or pull the placements apart",
    );
}

#[test]
fn a_router_past_the_area_cap_names_the_placements_that_stretch_its_box() {
    // `c` and `d` are four blocks apart; the router's box is wide
    // because `a` and `b` sit two thousand blocks off. `e1` reaches
    // the box's west edge, `e2` sits inside it and sets no edge, and
    // `e3`, north of everything, alone sets its north edge.
    let note = walkway_blocked_note_of(concat!(
        "def hut size=3x3:\n",
        "  floor id=f mat_slot=wall\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=front side=front at=center\n",
        "  door  id=back  side=back  at=center\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t north_of=a gap=2000\n",
        "  place id=c use=hut theme=t east_of=b gap=2000\n",
        "  place id=d use=hut theme=t north_of=c gap=4\n",
        "  place id=e1 use=hut theme=t north_of=a gap=500\n",
        "  place id=e2 use=hut theme=t east_of=e1 gap=500\n",
        "  place id=e3 use=hut theme=t north_of=e2 gap=1600\n",
        "  connect c.front to d.back path=@gravel\n",
    ));
    assert_eq!(
        note,
        format!(
            "the router searches the box spanning both ports and every placement floor on \
             the walk plane, here 4238888 cells, past its cap of {ROUTE_AREA_CAP} cells; \
             the two ports span only 42 of those cells, and the floors of placements `a`, \
             `b`, `c`, `d`, `e1`, and `e3` stretch the box past them, so bring those \
             placements closer to the two ports",
        ),
    );
}

#[test]
fn edge_pushers_names_each_floor_that_sets_an_edge_beyond_both_ports() {
    // Ports at x=0..=10, z=0..=10. Each of `w`, `e`, `n` and `s`
    // alone sets one edge of the rectangle, beyond the ports; `in`
    // sets none; `tie` reaches the east edge only where a port does,
    // so it stretches nothing; `other_plane` meets the east edge on
    // another walk plane.
    let extent = |min_x, max_x, min_z, max_z| FloorExtent {
        y: 0,
        min_x,
        max_x,
        min_z,
        max_z,
    };
    let extents: IndexMap<String, FloorExtent> = [
        ("w", extent(-20, -18, 4, 6)),
        ("e", extent(28, 30, 4, 6)),
        ("n", extent(4, 6, -40, -38)),
        ("s", extent(4, 6, 48, 50)),
        ("in", extent(2, 4, 2, 4)),
        ("tie", extent(8, 10, 4, 6)),
        ("other_plane", extent(28, 30, 20, 22)),
    ]
    .into_iter()
    .map(|(k, e)| {
        let e = if k == "other_plane" {
            FloorExtent { y: 7, ..e }
        } else {
            e
        };
        (k.to_owned(), e)
    })
    .collect();
    let rect = SearchRect {
        min_x: -21,
        max_x: 31,
        min_z: -41,
        max_z: 51,
    };
    assert_eq!(
        edge_pushers(rect, (0, 0, 0), (10, 0, 10), &extents),
        vec!["w", "e", "n", "s"],
    );
    // An edge a port sets is not stretched by a floor that only meets
    // it there.
    let to_tie = SearchRect { max_x: 11, ..rect };
    assert_eq!(
        edge_pushers(to_tie, (0, 0, 0), (10, 0, 10), &extents),
        vec!["w", "n", "s"],
    );
}

#[test]
fn a_router_past_the_area_cap_on_distant_ports_blames_the_ports() {
    // The floor plan is `p`'s nine cells; the two ports are 1999
    // apart on each axis, so the straight L is exactly at the cap and
    // the router's margin takes it past. Moving `p` cannot help.
    let note = walkway_blocked_note_of(concat!(
        "def hut size=3x3:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=front side=front at=center\n",
        "  door  id=back  side=back  at=center\n",
        "\n",
        "def pad size=3x3:\n",
        "  floor id=f mat_slot=wall\n",
        "  walls id=w mat_slot=wall height=3\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=q use=hut theme=t east_of=a gap=1996\n",
        "  place id=p use=pad theme=t north_of=q gap=500\n",
        "  place id=b use=hut theme=t north_of=q gap=1992\n",
        "  connect a.front to b.back path=@gravel\n",
    ));
    assert_eq!(
        note,
        format!(
            "the router searches the box spanning both ports and every placement floor on \
             the walk plane, here 4010006 cells, past its cap of {ROUTE_AREA_CAP} cells; \
             the two ports alone span 4008004 of those cells, so place the two structures \
             closer together",
        ),
    );
}

#[test]
fn a_straight_l_past_the_area_cap_is_not_laid_and_says_why() {
    // The two ports alone span more than the cap, so the row is
    // refused before the router or the voxel buffer is reached.
    let out = lowered(concat!(
        "def hut size=3x3:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=front side=front at=center\n",
        "  door  id=back  side=back  at=center\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=q use=hut theme=t east_of=a gap=2100\n",
        "  place id=b use=hut theme=t north_of=q gap=2100\n",
        "  connect a.front to b.back path=@gravel\n",
    ));
    let blocked = findings(&out, DiagnosticCode::WalkwayBlocked);
    assert_eq!(blocked.len(), 1, "{:?}", out.diagnostics);
    let area = l_path_area((1, 0, 3), (2104, 0, -2104));
    assert_eq!(
        blocked[0].primary,
        format!(
            "walkway `a.front ↔ b.back` spans {area} cells, past the {ROUTE_AREA_CAP}-cell \
             router cap, and was not laid"
        ),
    );
    let notes: Vec<&str> = blocked[0]
        .notes
        .iter()
        .map(|n| n.message.as_str())
        .collect();
    assert_eq!(
        notes,
        [format!(
            "the walkway search area ({area} cells) exceeds the router's cap of \
             {ROUTE_AREA_CAP} cells; place the two structures closer together"
        )
        .as_str()],
    );
    assert_eq!(blocked[0].data, None);
    assert!(out.walkways.is_empty());
}

#[test]
fn a_port_on_its_own_plate_and_another_floor_names_both() {
    // `o`, in another site, lays a floor over `a`'s port cell, where
    // `a`'s own plate already sits. Pulling the placements apart
    // leaves the plate, and moving the plate leaves the floor, so the
    // note names both.
    let note = walkway_blocked_note_of(concat!(
        "def hut size=5x5:\n",
        "  floor id=f mat_slot=wall\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  roof  kind=flat mat_slot=wall overhang=1\n",
        "  door  id=e side=front at=center\n",
        "  pressure_plate id=pp at=front.outside offset=2 y=0\n",
        "\n",
        "def slab size=7x7:\n",
        "  floor id=f mat_slot=wall\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut theme=t at=origin\n",
        "  place id=b use=hut theme=t east_of=a gap=4\n",
        "  connect a.e to b.e path=@gravel\n",
        "\n",
        "site u:\n",
        "  place id=o use=slab theme=t at=origin\n",
    ));
    assert_eq!(
        note,
        "port `a.e` is buried inside the floor of placement `o` in site `u` and sits on the \
         `minecraft:oak_pressure_plate` its own placement `a` lays on that cell; move that \
         block, and move that door/window to an unobstructed wall or pull the placements \
         apart; port `b.e` sits on the `minecraft:oak_pressure_plate` its own placement `b` \
         lays on that cell; move that block or the door/window",
    );
}

fn walkway_pair_source(path_token: &str) -> String {
    // No floors, so the path never collides.
    format!(
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=2\n",
            "  connect a.entry to b.entry path={token}\n",
        ),
        token = path_token,
    )
}

#[test]
fn walkway_abstract_path_lifts_through_registry_pack() {
    let resolver = FakeResolver {
        entries: vec![("walkway.gravel", "gravel")],
    };
    let src = walkway_pair_source("@walkway.gravel");
    let out = lowered_with_resolver(&src, &resolver);
    assert!(
        findings(&out, DiagnosticCode::AbstractTokenDeferred).is_empty()
            && findings(&out, DiagnosticCode::UnknownAbstractToken).is_empty(),
        "no abstract-token diagnostics expected, got {:?}",
        out.diagnostics,
    );
    let walkway = out
        .walkways
        .get("walkway::s::a.entry__b.entry")
        .expect("walkway lowered");
    assert_eq!(walkway.path_material, "minecraft:gravel");
    // Pinned here too, so a coordinate swap on the abstract-token path
    // fails on its own. a's front port is at (1, 0, 3); b east_of=a
    // gap=2 with cottage dims 3×3 → origin (5, 0, 0), front port
    // (6, 0, 3). The L-path collapses to a pure x-axis run.
    assert_eq!(walkway.origin, (1, 0, 3));
    assert_eq!(walkway.footprint, Footprint { x: 6, z: 1 });
}

#[test]
fn walkway_abstract_path_without_pack_emits_w_abstract_token_deferred() {
    // The row is dropped so the lockfile does not pin a strip with no
    // material.
    let src = walkway_pair_source("@walkway.gravel");
    let out = lowered(&src);
    let diag = *findings(&out, DiagnosticCode::AbstractTokenDeferred)
        .first()
        .unwrap_or_else(|| {
            panic!(
                "expected W_ABSTRACT_TOKEN_DEFERRED on the connect row, got {:?}",
                out.diagnostics,
            )
        });
    assert_eq!(diag.severity(), Severity::Warning);
    assert!(
        diag.primary.contains("@walkway.gravel"),
        "expected the abstract token to be named, got {}",
        diag.primary,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "no walkway should land without a material",
    );
}

#[test]
fn walkway_unknown_abstract_path_emits_e_unknown_abstract_token() {
    // A token the pack does not declare fails loud
    // (`spec/materials-themes` "Canonical vocabulary").
    let resolver = FakeResolver {
        entries: vec![("walkway.gravel", "gravel")],
    };
    let src = walkway_pair_source("@walkway.grvl");
    let out = lowered_with_resolver(&src, &resolver);
    let diag = *findings(&out, DiagnosticCode::UnknownAbstractToken)
        .first()
        .unwrap_or_else(|| {
            panic!(
                "expected E_UNKNOWN_ABSTRACT_TOKEN on the connect row, got {:?}",
                out.diagnostics,
            )
        });
    assert_eq!(diag.severity(), Severity::Error);
    assert!(
        diag.notes
            .iter()
            .any(|n| n.message.contains("walkway.gravel")),
        "expected nearest-match note pointing at walkway.gravel, got {:?}",
        diag.notes,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "no walkway should land for an unknown abstract token",
    );
}

fn endpoint_cascade_source(a_def: &str, b_def: &str, defs: &str, connect_line: &str) -> String {
    format!(
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "{defs}",
            "\n",
            "site s:\n",
            "  place id=a use={a_def} theme=t at=origin\n",
            "  place id=b use={b_def} theme=t at=origin\n",
            "  {connect_line}\n",
        ),
        defs = defs,
        a_def = a_def,
        b_def = b_def,
        connect_line = connect_line,
    )
}

fn sized_then_sizeless_defs() -> &'static str {
    concat!(
        "def sized size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "def sizeless:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
    )
}

/// The `W_DEFERRED_MEMBER` a walkway against a placement that did not
/// lower earns.
fn cascade_warning(out: &BlockArrayIr) -> &Diagnostic {
    deferrals(out)
        .into_iter()
        .find(|d| d.primary.contains("walkway") && d.primary.contains("did not lower"))
        .unwrap_or_else(|| {
            panic!(
                "expected a cascade W_DEFERRED_MEMBER, got {:?}",
                out.diagnostics,
            )
        })
}

#[test]
fn walkway_endpoint_skipped_to_side_cascades_w_deferred_member() {
    // `b` is sizeless, so the `to` placement is missing.
    let src = endpoint_cascade_source(
        "sized",
        "sizeless",
        sized_then_sizeless_defs(),
        "connect a.entry to b.entry path=@gravel",
    );
    let out = lowered(&src);
    let cascade = cascade_warning(&out);
    assert!(
        cascade.primary.contains("`b.entry` placement") && !cascade.primary.contains("`a.entry`"),
        "expected the cascade to single out `b.entry`, got {}",
        cascade.primary,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "walkway must not lay against a placement that did not lower",
    );
}

#[test]
fn walkway_endpoint_skipped_from_side_cascades_w_deferred_member() {
    // `a` is sizeless, so the `from` placement is missing.
    let src = endpoint_cascade_source(
        "sizeless",
        "sized",
        sized_then_sizeless_defs(),
        "connect a.entry to b.entry path=@gravel",
    );
    let out = lowered(&src);
    let cascade = cascade_warning(&out);
    assert!(
        cascade.primary.contains("`a.entry` placement") && !cascade.primary.contains("`b.entry`"),
        "expected the cascade to single out `a.entry`, got {}",
        cascade.primary,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "walkway must not lay against a placement that did not lower",
    );
}

#[test]
fn walkway_endpoint_skipped_both_sides_cascades_w_deferred_member() {
    let defs = concat!(
        "def sizeless_a:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
        "def sizeless_b:\n",
        "  walls mat_slot=wall height=2\n",
        "  door id=entry side=front at=center\n",
        "\n",
    );
    let src = endpoint_cascade_source(
        "sizeless_a",
        "sizeless_b",
        defs,
        "connect a.entry to b.entry path=@gravel",
    );
    let out = lowered(&src);
    let cascade = cascade_warning(&out);
    assert!(
        cascade.primary.contains("`a.entry`") && cascade.primary.contains("`b.entry`"),
        "expected the cascade to name both endpoints, got {}",
        cascade.primary,
    );
    assert!(
        !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
        "walkway must not lay against a pair of skipped placements",
    );
}

#[test]
fn east_of_skipped_prior_does_not_silently_stack_at_origin() {
    // `a` is sizeless, so it earns `W_DEF_NO_SIZE` and never lands in
    // `placements`. Falling back to `(0, 0, 0)` for `east_of=a` would
    // stack the two buildings, so `b` is skipped instead.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def sized size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "def sizeless:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=a use=sizeless theme=t at=origin\n",
        "  place id=b use=sized theme=t east_of=a gap=2\n",
    );
    let out = lowered(src);
    assert!(
        !out.placements.contains_key("site::s::b"),
        "placement b must be skipped, not silently stacked at origin",
    );
    let cascade = deferrals(&out)
        .iter()
        .any(|d| d.primary.contains("east_of") && d.primary.contains("origin cannot be resolved"));
    assert!(
        cascade,
        "expected a cascade W_DEFERRED_MEMBER mentioning the unresolvable origin, got {:?}",
        out.diagnostics,
    );
}

// --- level flattening and its y offset ----------------------------------

#[test]
fn level_without_y_defers_and_drops_its_children() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=floor2\n    walls id=upper mat_slot=w height=2\n"
    );
    let out = lowered(src);
    let defers = deferred_lines(src, &out);
    assert_eq!(
        defers.len(),
        1,
        "level with missing y= should defer once, got {defers:?}",
    );
    assert_eq!(
        defers[0].0, "level id=floor2",
        "the defer sits on the level"
    );
    assert!(
        defers[0].1.contains("level requires `y="),
        "defer reason should mention required y=; got {}",
        defers[0].1,
    );
}

#[test]
fn level_with_non_integer_y_defers_with_generic_reason() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=floor2 y=top\n    walls id=upper mat_slot=w height=2\n"
    );
    let out = lowered(src);
    assert_defers_on(src, &out, "level id=floor2 y=top", "`y=`");
}

#[test]
fn nested_level_defers_per_inner_level_child() {
    // One deferral per inner level, so the count says how many
    // subtrees were dropped.
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=outer y=0\n    level id=inner1 y=1\n      walls id=a mat_slot=w height=1\n    level id=inner2 y=2\n      walls id=b mat_slot=w height=1\n"
    );
    let out = lowered(src);
    let nested: Vec<(&str, &str)> = deferred_lines(src, &out)
        .into_iter()
        .filter(|(_, primary)| primary.contains("nested"))
        .collect();
    assert_eq!(
        nested.len(),
        2,
        "one defer per inner level expected, got {nested:?}",
    );
    assert_eq!(
        nested.iter().map(|&(line, _)| line).collect::<Vec<_>>(),
        ["level id=inner1 y=1", "level id=inner2 y=2"],
        "each defer sits on its inner level",
    );
}

#[test]
fn max_wall_top_aggregates_across_level_walls() {
    // struct walls height=5 + level y=5 walls height=4 → tower top
    // at y=9. `dims.y = 1 + 9 + gable_extra` with roof_w=5, roof_h=5
    // (no overhang) so ridge span=5 and gable_extra = ceil(5/2) = 3.
    // dims.y = 1 + 9 + 3 = 13.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=5\n  roof kind=gable mat_slot=r\n  level id=floor2 y=5\n    walls id=upper mat_slot=w height=4\n"
    );
    let out = lowered(src);
    let ba = out.structures.get("struct::s").expect("structure lowered");
    assert_eq!(ba.dims.y, 13);
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected defers: {:?}",
        out.diagnostics
    );
    // Second-floor SW-most (low-x, low-z) corner at y=6..=9 is
    // cobblestone from the level walls, not air.
    for y in 6..=9 {
        assert_eq!(block_id(ba, 0, y, 0), "minecraft:cobblestone");
    }
}

// --- fill_stair unhappy paths -------------------------------------------

/// A 5x5 struct under a gable with `overhang=1`, so an eave `stair` has
/// a band to sit in, followed by `stair`.
fn eave_stair_source(stair: &str) -> String {
    format!(
        "{}  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  \
         roof kind=gable mat_slot=r overhang=1\n  {stair}\n",
        theme_t!("w", ""),
    )
}

#[test]
fn stair_without_kind_defers() {
    let stair = "stair id=e mat_slot=r side=front half=top facing=out";
    let src = eave_stair_source(stair);
    assert_defers_on(&src, &lowered(&src), stair, "stair without `kind=`");
}

#[test]
fn stair_with_unknown_kind_defers() {
    let stair = "stair id=e kind=spiral mat_slot=r side=front half=top";
    let src = eave_stair_source(stair);
    assert_defers_on(&src, &lowered(&src), stair, "stair `kind=spiral`");
}

#[test]
fn stair_with_unknown_half_defers() {
    let stair = "stair id=e kind=stairs mat_slot=r side=front half=middle";
    let src = eave_stair_source(stair);
    assert_defers_on(&src, &lowered(&src), stair, "stair `half=middle`");
}

#[test]
fn stair_with_unknown_facing_defers() {
    let stair = "stair id=e kind=stairs mat_slot=r side=front half=top facing=north";
    let src = eave_stair_source(stair);
    assert_defers_on(&src, &lowered(&src), stair, "stair `facing=north`");
}

#[test]
fn stair_with_unknown_shape_defers() {
    let stair = "stair id=e kind=stairs mat_slot=r side=front half=top facing=out shape=inner_left";
    let src = eave_stair_source(stair);
    assert_defers_on(&src, &lowered(&src), stair, "stair `shape=inner_left`");
}

#[test]
fn stair_without_overhang_defers() {
    // Two ways to arrive at an overhang of zero. The first is the
    // author writing none. The second is a roof that will not draw,
    // which contributes none however large its `overhang=` — and an
    // author who wrote `overhang=2` must not be told the key is
    // missing.
    let stair = "stair id=e kind=stairs mat_slot=r side=front half=top facing=out";
    for roof in ["roof kind=flat mat_slot=r", "roof mat_slot=r overhang=2"] {
        let src = format!(
            "{}  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  \
             {roof}\n  {stair}\n",
            theme_t!("w", ""),
        );
        let out = lowered(&src);
        let reason = deferred_lines(&src, &out)
            .into_iter()
            .find(|&(line, primary)| line == stair && primary.contains("eave `stair`"))
            .unwrap_or_else(|| {
                panic!(
                    "expected the eave to defer under `{roof}`, got {:?}",
                    out.diagnostics
                )
            })
            .1
            .to_owned();
        assert!(
            reason.contains("draws an overhang"),
            "the reason must name the overhang a roof draws, not the key the author may well have written: {reason}",
        );
    }
}

#[test]
fn stair_with_y_out_of_bounds_defers() {
    // struct walls height=3, roof kind=flat → dims.y = 1 + 3 + 1 = 5.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r overhang=1\n  stair id=e kind=stairs mat_slot=r side=front half=top facing=out y=99\n"
    );
    let out = lowered(src);
    assert_defers_on(
        src,
        &out,
        "stair id=e kind=stairs mat_slot=r side=front half=top facing=out y=99",
        "y=99",
    );
}

#[test]
fn stair_facing_in_paints_the_inward_cardinal() {
    // side=front + facing=in → the stair riser points -z (north).
    let out = lowered(&eave_stair_source(
        "stair id=e kind=stairs mat_slot=r side=front half=top facing=in",
    ));
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected defers: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    // Overhang row outside front wall is z = overhang + interior_h = 6.
    // Local y=0 → world y=0. Interior x∈[1, 5]. Grab column x=3.
    let state = &ba.palette.entries[usize::from(ba.voxels[ba.dims.index(3, 0, 6).unwrap()].0)];
    assert_eq!(
        state.properties.get("facing").map(String::as_str),
        Some("north")
    );
}

// --- fill_window unhappy paths ------------------------------------------

/// A 6x5 struct walled three high, followed by `window`.
fn walled_6x5_source(window: &str) -> String {
    format!(
        "{}\nstruct s size=6x5\n  walls mat_slot=w height=3\n  {window}\n",
        theme_t!("w", ""),
    )
}

#[test]
fn window_repeat_zero_defers() {
    let window = "window side=front y=1 size=1x1 repeat=0";
    let src = walled_6x5_source(window);
    assert_defers_on(&src, &lowered(&src), window, "repeat=0");
}

#[test]
fn window_repeat_without_step_defers() {
    let window = "window side=front y=1 size=1x1 repeat=3";
    let src = walled_6x5_source(window);
    assert_defers_on(&src, &lowered(&src), window, "requires a positive `step=`");
}

#[test]
fn window_repeat_with_sym_defers() {
    let window = "window side=front y=1 size=1x1 repeat=2 step=2 sym=true";
    let src = walled_6x5_source(window);
    assert_defers_on(&src, &lowered(&src), window, "`repeat=` and `sym=true`");
}

#[test]
fn window_span_beyond_wall_defers() {
    // wall length 6, size=2, repeat=4 step=2 → span_end = 0 + 3*2 + 2 = 8 > 6.
    let window = "window side=front y=1 size=2x1 repeat=4 step=2";
    let src = walled_6x5_source(window);
    assert_defers_on(&src, &lowered(&src), window, "extends beyond");
}

#[test]
fn window_repeat_step_leaves_columns_between_stamps_alone() {
    // repeat=2 step=3 size=1x1 offset=0 on a 6-wide front wall:
    // stamps at u=0 and u=3. Wall x mapping: overhang=0, interior_w=6,
    // z=interior_h - 1 = 4. Column at u=1 must remain cobblestone.
    let out = lowered(&walled_6x5_source(
        "window side=front y=1 size=1x1 repeat=2 step=3",
    ));
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected defers: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    // Air carve (no mat_slot=) → voxel is air (palette index 0).
    assert_eq!(block_id(ba, 0, 1, 4), BlockState::AIR_ID);
    assert_eq!(block_id(ba, 3, 1, 4), BlockState::AIR_ID);
    // Between stamps at u=1 and u=2 the wall stays.
    assert_eq!(block_id(ba, 1, 1, 4), "minecraft:cobblestone");
    assert_eq!(block_id(ba, 2, 1, 4), "minecraft:cobblestone");
}

#[test]
fn a_window_without_mat_slot_carves_one_stamp_to_air() {
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  window side=front y=1 offset=2 size=1x1\n"
    );
    let out = lowered(src);
    assert_eq!(
        deferred_count(&out),
        0,
        "unexpected defers: {:?}",
        out.diagnostics
    );
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
}

#[test]
fn window_carve_stops_at_the_top_of_the_wall_not_the_top_of_the_volume() {
    // walls height=3 fills rows 1..=3, roof kind=flat puts a deck at
    // y=4 and takes dims.y to 5. A mat_slot-less window at y=3 size=1x2
    // carves to air, so gating on the volume rather than on the wall
    // would punch a hole through the deck. It must defer.
    let src = theme_t!(
        "w",
        "  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n  window side=front y=3 size=1x2\n"
    );
    let out = lowered(src);
    assert!(
        deferred_lines(src, &out).contains(&(
            "window side=front y=3 size=1x2",
            "window rows y=3..=4 are not all inside one wall course (size=1x2; the walls \
             occupy y=1..=3)",
        )),
        "expected wall-column defer, got {:?}",
        out.diagnostics,
    );
}

// --- carve_door course cap ----------------------------------------------

#[test]
fn door_defers_when_its_level_sits_at_or_above_the_wall_it_would_carve() {
    // struct walls height=3 (rows 1..=3) with a door inside `level
    // y=3`: the door opens at world y=4, which is above the course
    // and inside no other. The defer names the row and the rows the
    // walls do occupy instead of silently painting AIR over air.
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=roofline y=3\n    door id=hole side=front at=center\n"
    );
    let out = lowered(src);
    assert!(
        deferred_lines(src, &out).contains(&(
            "door id=hole side=front at=center",
            "door opens at y=4, which is not inside any wall course (the walls occupy y=1..=3)",
        )),
        "expected a wall-course defer, got {:?}",
        out.diagnostics,
    );
}

// --- pressure_plate lowering --------------------------------------------

/// A 3x3 struct walled two high in `@oak_pressure_plate`, followed by
/// `member`.
fn plate_walled_3x3(member: &str) -> String {
    format!(
        "{}\nstruct s size=3x3\n  walls mat_slot=p height=2\n  {member}\n",
        theme_t!("p" -> "oak_pressure_plate", ""),
    )
}

#[test]
fn pressure_plate_outside_with_overhang_paints_in_the_overhang_column() {
    // With `overhang=1` on the roof the struct inflates by one voxel
    // on every horizontal axis (dims.x = 3+2 = 5, dims.z = 5). The
    // front wall sits at z=overhang+ih-1=3, and the exterior
    // overhang column is at z=4. `at=front.outside offset=0 y=0`
    // must land in that exterior column, not fall back to the wall
    // row.
    let out = lowered(&plate_walled_3x3(
        "roof kind=flat mat_slot=p overhang=1\n  pressure_plate at=front.outside offset=0 y=0",
    ));
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.x, 5);
    assert_eq!(ba.dims.z, 5);
    assert_eq!(block_id(ba, 1, 0, 4), "minecraft:oak_pressure_plate");
    // The wall's own foundation cell at z=3 must NOT hold a plate.
    assert_ne!(block_id(ba, 1, 0, 3), "minecraft:oak_pressure_plate");
}

#[test]
fn pressure_plate_outside_at_y_zero_without_overhang_falls_back_to_foundation() {
    // No roof → dims stay at the authored 3x3 footprint (overhang=0).
    // `at=front.outside offset=0 y=0` cannot reach an exterior cell,
    // so the foundation fallback paints on the wall's own y=0 column
    // (still floor material at that row).
    let out = lowered(&plate_walled_3x3(
        "pressure_plate at=front.outside offset=0 y=0",
    ));
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(ba.dims.x, 3);
    assert_eq!(ba.dims.z, 3);
    assert_eq!(block_id(ba, 0, 0, 2), "minecraft:oak_pressure_plate");
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn pressure_plate_outside_above_ground_without_overhang_defers() {
    // A plate at `y=1` with no overhang would clobber the wall block
    // the massing phase painted directly above the foundation. The
    // fallback is restricted to y=0 for that reason and higher plates
    // must defer, with an exterior-voxel reason.
    let plate = "pressure_plate at=front.outside offset=0 y=1";
    let src = plate_walled_3x3(plate);
    assert_defers_on(&src, &lowered(&src), plate, "has no exterior voxel");
}

#[test]
fn pressure_plate_outside_saturating_shift_defers_above_ground() {
    // Left wall at overhang=0 sits at x=0. `shift_outward` saturates
    // back to x=0, which is a valid dims cell but *not* an exterior
    // voxel. At y=1 the saturating shift must NOT silently overwrite
    // the wall block — the anchor defers instead.
    let plate = "pressure_plate at=left.outside offset=0 y=1";
    let src = plate_walled_3x3(plate);
    let out = lowered(&src);
    let ba = out.structures.get("struct::s").unwrap();
    assert_defers_on(&src, &out, plate, "has no exterior voxel");
    // And the wall block above the foundation must survive intact.
    assert_eq!(block_id(ba, 0, 1, 1), "minecraft:oak_pressure_plate");
}

#[test]
fn pressure_plate_inside_paints_one_voxel_toward_the_interior() {
    // The front wall's middle cell is (1, 0, 2); one step in is the
    // struct's only interior cell.
    let out = lowered(&plate_walled_3x3(
        "pressure_plate at=inside.front offset=1 y=0",
    ));
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_pressure_plate");
    assert_eq!(deferred_count(&out), 0);
}

#[test]
fn pressure_plate_mat_slot_resolves_into_the_palette() {
    // A `mat_slot=` bound to a canonical id must land in the palette
    // verbatim — the default `oak_pressure_plate` fallback only
    // fires when no binding resolves.
    let src = "theme t:\n  slot fixture -> @spruce_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=fixture height=2\n  pressure_plate mat_slot=fixture at=inside.front offset=1 y=0\n";
    let out = lowered(src);
    let ba = out.structures.get("struct::s").unwrap();
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert!(
        ids.contains(&"minecraft:spruce_pressure_plate"),
        "palette should carry the resolved id, got {ids:?}",
    );
    assert_eq!(block_id(ba, 1, 0, 1), "minecraft:spruce_pressure_plate");
}

/// Lower `size=<size>` with `members` (each line indented, cobblestone
/// on the `wall` slot) and one plate line, and return the output with
/// the `W_DEFERRED_MEMBER` primaries.
fn lowered_plate(size: &str, members: &str, plate: &str) -> (BlockArrayIr, Vec<String>) {
    let src = format!(
        "theme t:\n  slot wall -> @cobblestone\n\nstruct s size={size}\n{members}  \
         pressure_plate {plate}\n",
    );
    let out = lowered(&src);
    let reasons = primaries_of(&out, DiagnosticCode::DeferredMember)
        .into_iter()
        .map(str::to_owned)
        .collect();
    (out, reasons)
}

/// [`lowered_plate`] with `walls height=3` ahead of `extra`.
fn lowered_inside_plate(size: &str, extra: &str, plate: &str) -> (BlockArrayIr, Vec<String>) {
    lowered_plate(
        size,
        &format!("  walls mat_slot=wall height=3\n{extra}"),
        plate,
    )
}

fn plate_count(ba: &BlockArray) -> usize {
    ba.voxels
        .iter()
        .filter(|v| ba.palette.entries[usize::from(v.0)].id == PRESSURE_PLATE_BASE_ID)
        .count()
}

/// The wall rings the inside-plate tests run against: the footprint's
/// own edge, and the ring a roof overhang of 1 or 2 moves inward.
const OVERHANGS: [u32; 3] = [0, 1, 2];

/// The member line that gives a struct `overhang`, empty for none.
fn overhang_roof(overhang: u32) -> String {
    if overhang == 0 {
        String::new()
    } else {
        format!("  roof kind=flat mat_slot=wall overhang={overhang}\n")
    }
}

#[test]
fn pressure_plate_inside_at_a_corner_defers_and_keeps_the_side_wall() {
    // The front wall of a 5x5 struct is z=4; one step in from its
    // offset=0 end is (0, 1, 3), a block of the left wall.
    let (out, reasons) =
        lowered_inside_plate("5x5", "", "id=p at=inside.front offset=0 y=1 -> sig.a");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        reasons,
        vec![
            "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so \
             the voxel inside it belongs to the neighbouring wall; use an `offset=` from 1 to \
             3 to reach an interior voxel. Its signal binding still reaches the netlist, with \
             no plate placed to drive it"
                .to_owned()
        ],
    );
    assert_eq!(block_id(ba, 0, 1, 3), "minecraft:cobblestone");
    assert_eq!(plate_count(ba), 0);
}

#[test]
fn pressure_plate_inside_at_a_corner_defers_at_the_floor_row_too() {
    // At y=0 the corner cell is floor under the side wall. The
    // `<side>.outside` foundation fallback is y=0-only; inside has no
    // such exemption. No wall is painted at row 0, so the reason does
    // not claim one.
    let (out, reasons) = lowered_inside_plate("5x5", "", "at=inside.front offset=0 y=0");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        reasons,
        vec![
            "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so \
             the voxel inside it is not an interior cell of this struct; use an `offset=` \
             from 1 to 3 to reach an interior voxel"
                .to_owned()
        ],
    );
    assert_eq!(plate_count(ba), 0);
}

#[test]
fn pressure_plate_inside_at_a_corner_claims_no_wall_where_none_is_painted() {
    // The ring is decided from the footprint, so a corner is refused
    // at every row — but only a row the walls paint has a wall there
    // to name: not a floor-only struct, and not the air between two
    // courses (rows 1 and 5 here).
    let cases = [
        ("  floor mat_slot=wall\n", 0),
        (
            "  walls mat_slot=wall height=1\n  level y=4\n    walls mat_slot=wall height=1\n",
            3,
        ),
    ];
    for (members, y) in cases {
        let (out, reasons) =
            lowered_plate("5x5", members, &format!("at=inside.front offset=0 y={y}"));
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            reasons,
            vec![
                "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, \
                 so the voxel inside it is not an interior cell of this struct; use an \
                 `offset=` from 1 to 3 to reach an interior voxel"
                    .to_owned()
            ],
            "{members:?} y={y}",
        );
        assert_eq!(plate_count(ba), 0);
    }
}

#[test]
fn pressure_plate_inside_at_a_corner_of_a_long_left_wall_names_that_wall() {
    // On a 4x6 footprint the left wall runs along `size.h`, so both
    // its name and its offset range differ from the front wall's.
    let (out, reasons) = lowered_inside_plate("4x6", "", "at=inside.left offset=5 y=1");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        reasons,
        vec![
            "pressure_plate `at=inside.left offset=5` is at a corner of the left wall, so \
             the voxel inside it belongs to the neighbouring wall; use an `offset=` from 1 to \
             4 to reach an interior voxel"
                .to_owned()
        ],
    );
    assert_eq!(plate_count(ba), 0);
}

/// The four sides of a 4x5 footprint with the length of each wall:
/// front and back run along `size.w`, left and right along `size.h`.
const SIDES_4X5: [(&str, u32); 4] = [("front", 4), ("back", 4), ("left", 5), ("right", 5)];

#[test]
fn pressure_plate_inside_defers_at_both_corners_of_every_side() {
    // Each side mirrors `offset=` differently, so both ends of every
    // wall are checked: a refusal keyed on one axis or one end would
    // let the other three walls or the far corner through. Under an
    // overhang the wall ring moves in, so a bound measured from the
    // volume's edge rather than the ring would let the corner in.
    for overhang in OVERHANGS {
        for (side, length) in SIDES_4X5 {
            for offset in [0, length - 1] {
                let (out, reasons) = lowered_inside_plate(
                    "4x5",
                    &overhang_roof(overhang),
                    &format!("at=inside.{side} offset={offset} y=1"),
                );
                let ba = out.structures.get("struct::s").unwrap();
                let case = format!("overhang={overhang} inside.{side} offset={offset}");
                assert_eq!(
                    reasons,
                    vec![format!(
                        "pressure_plate `at=inside.{side} offset={offset}` is at a corner of \
                         the {side} wall, so the voxel inside it belongs to the neighbouring \
                         wall; use an `offset=` from 1 to {} to reach an interior voxel",
                        length - 2,
                    )],
                    "{case}",
                );
                assert_eq!(plate_count(ba), 0, "{case} painted");
            }
        }
    }
}

#[test]
fn pressure_plate_inside_paints_at_every_non_corner_offset_of_every_side() {
    // The other half of the corner rule: every offset between the two
    // corners reaches a genuine interior cell and paints without a
    // word. A 4x5 footprint keeps the two axes' lengths distinct, and
    // the cells are those of the footprint with no overhang.
    let cells: [&[(u32, u32)]; 4] = [
        &[(1, 3), (2, 3)],
        &[(2, 1), (1, 1)],
        &[(1, 1), (1, 2), (1, 3)],
        &[(2, 3), (2, 2), (2, 1)],
    ];
    for overhang in OVERHANGS {
        for ((side, length), cells) in SIDES_4X5.into_iter().zip(cells) {
            assert_eq!(
                cells.len(),
                usize::try_from(length - 2).unwrap(),
                "{side} has one cell per non-corner offset",
            );
            for (offset, &(x, z)) in (1..length - 1).zip(cells) {
                let (out, reasons) = lowered_inside_plate(
                    "4x5",
                    &overhang_roof(overhang),
                    &format!("at=inside.{side} offset={offset} y=1"),
                );
                let ba = out.structures.get("struct::s").unwrap();
                let case = format!("overhang={overhang} inside.{side} offset={offset}");
                assert_eq!(reasons, Vec::<String>::new(), "{case}");
                let (x, z) = (x + overhang, z + overhang);
                assert_eq!(
                    block_id(ba, x, 1, z),
                    PRESSURE_PLATE_BASE_ID,
                    "{case} should land on ({x}, 1, {z})",
                );
            }
        }
    }
}

#[test]
fn pressure_plate_inside_a_two_deep_struct_defers_and_keeps_the_back_wall() {
    // With size.h = 2 the cell one step in from the front wall is the
    // back wall. With no overhang `front.outside` would bury the plate
    // under the wall at y=0 and find no cell above it, so the reason
    // does not offer it.
    let (out, reasons) =
        lowered_inside_plate("5x2", "", "id=p at=inside.front offset=2 y=1 -> sig.a");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        reasons,
        vec![
            "pressure_plate `at=inside.front offset=2`: the struct's size.h is 2, so it has \
             no interior voxel; an interior needs a size of at least 3 on both axes. Grow \
             `size.h` to at least 3. Its signal binding still reaches the netlist, with no \
             plate placed to drive it"
                .to_owned()
        ],
    );
    assert_eq!(block_id(ba, 2, 1, 0), "minecraft:cobblestone");
    assert_eq!(plate_count(ba), 0);
}

#[test]
fn pressure_plate_inside_a_one_deep_struct_with_an_overhang_defers() {
    // With size.h = 1 the front and back walls are one row (z=1 under
    // `overhang=1`), and the inward step lands in the overhang ring
    // behind the building at z=0.
    let roof = overhang_roof(1);
    let (out, reasons) = lowered_inside_plate("5x1", &roof, "at=inside.front offset=2 y=1");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        reasons,
        vec![
            "pressure_plate `at=inside.front offset=2`: the struct's size.h is 1, so it has \
             no interior voxel; an interior needs a size of at least 3 on both axes. Grow \
             `size.h` to at least 3, or anchor the plate with `at=front.outside`"
                .to_owned()
        ],
    );
    assert_eq!(block_id(ba, 3, 1, 0), "minecraft:air");
    assert_eq!(plate_count(ba), 0);
    // The remedy the reason offers works on this very struct.
    let (out, reasons) = lowered_inside_plate("5x1", &roof, "at=front.outside offset=2 y=1");
    assert_eq!(reasons, Vec::<String>::new());
    assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 1);
}

#[test]
fn pressure_plate_inside_a_narrow_struct_names_the_narrow_axis() {
    // The front wall of a 2-wide struct is two corners with nothing
    // between them, so no offset can help until the width grows.
    let (out, reasons) = lowered_inside_plate("2x5", "", "at=inside.front offset=1 y=1");
    let ba = out.structures.get("struct::s").unwrap();
    assert_eq!(
        reasons,
        vec![
            "pressure_plate `at=inside.front offset=1`: the struct's size.w is 2, so it has \
             no interior voxel, and `offset=1` is at a corner of the front wall besides; an \
             interior needs a size of at least 3 on both axes. Grow `size.w` to at least 3 \
             and use an `offset=` away from both ends of the wall"
                .to_owned()
        ],
    );
    assert_eq!(plate_count(ba), 0);
}

#[test]
fn pressure_plate_inside_reports_a_thin_axis_and_a_corner_in_one_pass() {
    // `2x9 at=inside.left offset=0` is both too narrow and a corner:
    // one reason names both, so widening alone is not the whole fix.
    // With both axes thin, both are named.
    let cases = [
        (
            "2x9",
            "at=inside.left offset=0 y=1",
            "pressure_plate `at=inside.left offset=0`: the struct's size.w is 2, so it has \
             no interior voxel, and `offset=0` is at a corner of the left wall besides; an \
             interior needs a size of at least 3 on both axes. Grow `size.w` to at least 3 \
             and use an `offset=` from 1 to 7",
        ),
        (
            "2x2",
            "at=inside.front offset=0 y=1",
            "pressure_plate `at=inside.front offset=0`: the struct's size.w is 2 and size.h \
             is 2, so it has no interior voxel, and `offset=0` is at a corner of the front \
             wall besides; an interior needs a size of at least 3 on both axes. Grow \
             `size.w` to at least 3 and `size.h` to at least 3 and use an `offset=` away \
             from both ends of the wall",
        ),
    ];
    for (size, plate, expected) in cases {
        let (out, reasons) = lowered_inside_plate(size, "", plate);
        assert_eq!(reasons, vec![expected.to_owned()], "{size} {plate}");
        assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 0);
    }
}

#[test]
fn pressure_plate_rejects_missing_and_malformed_anchors() {
    for (plate, needle) in [
        ("pressure_plate offset=0 y=0", "without `at=`"),
        (
            "pressure_plate at=center offset=0 y=0",
            "must be `<side>.outside`",
        ),
        (
            "pressure_plate at=up.outside offset=0 y=0",
            "is not one of front, back, left, right",
        ),
    ] {
        let src = plate_walled_3x3(plate);
        assert_defers_on(&src, &lowered(&src), plate, needle);
    }
}

#[test]
fn pressure_plate_out_of_range_offset_or_y_defers() {
    for (plate, needle) in [
        // offset=99 past a 3-length front wall.
        (
            "pressure_plate at=inside.front offset=99 y=0",
            "runs past the front wall",
        ),
        // offset=3 is the first column past a 3-length wall: the
        // boundary `wall_local_to_grid` refuses on, so it has to be
        // refused here first — the arm after the helper call asserts
        // rather than reports (`INVARIANT(wall-grid-validated)`).
        (
            "pressure_plate at=inside.front offset=3 y=0",
            "`offset=3` runs past the front wall",
        ),
        // y=99 past the struct's dims.y.
        (
            "pressure_plate at=inside.front offset=0 y=99",
            "does not fit in the struct",
        ),
    ] {
        let src = plate_walled_3x3(plate);
        assert_defers_on(&src, &lowered(&src), plate, needle);
    }
}

#[test]
fn nonneg_int_rejects_values_that_do_not_fit_in_u32() {
    // 5_000_000_000 exceeds u32::MAX (~4.29 * 10^9); it defers via
    // `nonneg_int_or_defer` at the level's `y=`.
    let src = theme_t!(
        "w",
        "\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=huge y=5000000000\n    walls id=upper mat_slot=w height=1\n"
    );
    let out = lowered(src);
    assert_defers_on(src, &out, "level id=huge y=5000000000", "fits in u32");
}
