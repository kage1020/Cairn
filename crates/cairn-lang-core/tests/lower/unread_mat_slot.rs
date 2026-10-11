//! A role whose `MemberRole::unread_arguments` lists `mat_slot` builds the same under two slot
//! names bound to different blocks, even with every `slot_value` filled in by hand.

use std::collections::HashMap;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::intent::{Member, known_keywords, role_of};
use cairn_lang_core::resolve::Resolution;
use cairn_lang_core::{BlockArrayIr, IntentModule, lower, parse, resolve};

/// Where a fixture's slot name goes.
const SLOT: &str = "%SLOT%";

/// Two slots bound to blocks no other member paints, and the slot the rest of each body paints.
const THEME: &str = "theme t:\n  slot first -> @oak_planks\n  slot second -> @cobblestone\n  slot wall -> @stone_bricks\n\n";

/// The two names each fixture is built under.
const NAMES: (&str, &str) = ("first", "second");

/// A `def` a site fixture can place, with a door a `connect` can name.
const HUT: &str = "def hut size=3x3:\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=entry side=front at=center\n\n";

/// The body after [`THEME`] for each keyword under test, with [`SLOT`] where the name goes.
const FIXTURES: &[(&str, &str)] = &[
    (
        "door",
        "struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door side=front at=center mat_slot=%SLOT%\n",
    ),
    (
        "level",
        "struct s size=7x5\n  floor mat_slot=wall\n  level y=0 mat_slot=%SLOT%\n    walls mat_slot=wall height=3\n",
    ),
    (
        "circuit",
        "struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  circuit region=floor void=2 mat_slot=%SLOT%\n",
    ),
    (
        "place",
        "def d size=5x5:\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n\nsite v:\n  place id=a use=d theme=t at=origin mat_slot=%SLOT%\n",
    ),
    (
        "connect",
        "site v:\n  place id=a use=hut theme=t at=origin\n  place id=b use=hut theme=t east_of=a gap=4\n  connect a.entry to b.entry path=@gravel mat_slot=%SLOT%\n",
    ),
    // The control: a `window` reads its `mat_slot=`, so it builds differently under the two names.
    (
        "window",
        "struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  window side=front y=1 offset=1 size=2x1 mat_slot=%SLOT%\n",
    ),
];

fn source(body: &str, name: &str) -> String {
    let hut = if body.contains("use=hut") { HUT } else { "" };
    format!("{THEME}{hut}{}", body.replace(SLOT, name))
}

/// Every member of every `struct` and `def` body, keyed by span start as the resolver keys them.
fn members_by_start(ir: &IntentModule) -> HashMap<usize, &Member> {
    fn walk<'a>(members: &'a [Member], out: &mut HashMap<usize, &'a Member>) {
        for member in members {
            out.insert(member.span.start, member);
            walk(&member.children.members, out);
        }
    }
    let mut out = HashMap::new();
    for s in &ir.structs {
        walk(&s.members, &mut out);
    }
    for d in &ir.defs {
        walk(&d.members, &mut out);
    }
    out
}

/// Gives every member in every bound scope the `slot_value` its theme declares, whatever its role.
fn look_up_every_slot(ir: &IntentModule, resolution: &mut Resolution) {
    let members = members_by_start(ir);
    for scope in resolution.scopes.values_mut() {
        let Some(theme) = scope
            .bound_theme
            .as_ref()
            .and_then(|name| resolution.themes.get(name))
        else {
            continue;
        };
        for (start, binding) in &mut scope.members {
            if let Some(slot) = members.get(start).and_then(|m| m.mat_slot.as_ref())
                && let Some(value) = theme.slots.get(&slot.name)
            {
                binding.slot_value = Some(value.clone());
            }
        }
    }
}

/// One source built as the pipeline runs it, and again with every slot looked up.
fn builds(source: &str) -> (BlockArrayIr, BlockArrayIr) {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    assert_eq!(
        resolution
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        Vec::<&str>::new(),
        "the resolver raises nothing on the fixture:\n{source}",
    );
    let as_run = lower_to_block_array(&ir, &resolution, None);
    let mut looked_up = resolution.clone();
    look_up_every_slot(&ir, &mut looked_up);
    let with_every_slot = lower_to_block_array(&ir, &looked_up, None);
    (as_run, with_every_slot)
}

/// Whether two builds match in blocks, placements and walkways; diagnostics carry spans.
fn same_build(left: &BlockArrayIr, right: &BlockArrayIr) -> bool {
    left.structures == right.structures
        && left.placements == right.placements
        && left.walkways == right.walkways
}

/// Voxels the build paints, across every structure and walkway.
fn painted(ir: &BlockArrayIr) -> usize {
    let structures: usize = ir
        .structures
        .values()
        .map(|ba| ba.voxels.iter().filter(|i| i.0 != 0).count())
        .sum();
    structures + ir.walkways.len()
}

/// The block ids each structure's palette holds, for a failure message.
fn palettes(ir: &BlockArrayIr) -> String {
    ir.structures
        .iter()
        .map(|(key, ba)| {
            let ids: Vec<&str> = ba.palette.entries.iter().map(|e| e.id.as_str()).collect();
            format!("{key}: {}", ids.join(" "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_mat_slot_nothing_reads_leaves_the_build_as_it_was() {
    let (one, other) = NAMES;
    for (keyword, body) in FIXTURES {
        let (first, first_looked_up) = builds(&source(body, one));
        let (second, second_looked_up) = builds(&source(body, other));
        for (how, ir) in [
            (one, &first),
            (other, &second),
            ("first, looked up", &first_looked_up),
            ("second, looked up", &second_looked_up),
        ] {
            assert!(
                painted(ir) > 0,
                "`{keyword}` ({how}) paints nothing, so the comparison has no evidence in it",
            );
            assert_eq!(
                ir.diagnostics
                    .iter()
                    .map(|d| d.code.as_str())
                    .collect::<Vec<_>>(),
                Vec::<&str>::new(),
                "`{keyword}` ({how}) does not lower cleanly, so the builds differ for a reason \
                 that is not the slot",
            );
        }
        if role_of(keyword).unread_argument("mat_slot").is_some() {
            assert!(
                same_build(&first, &second),
                "`{keyword}`'s `mat_slot=` is listed as unread, and the build moves between \
                 `{one}` and `{other}`:\nfirst:\n{}\nsecond:\n{}",
                palettes(&first),
                palettes(&second),
            );
            assert!(
                same_build(&first_looked_up, &second_looked_up),
                "`{keyword}`'s `mat_slot=` is listed as unread, and a lowering rule reads the \
                 slot it names once it is looked up:\nfirst:\n{}\nsecond:\n{}",
                palettes(&first_looked_up),
                palettes(&second_looked_up),
            );
        } else {
            assert!(
                !same_build(&first, &second) && !same_build(&first_looked_up, &second_looked_up),
                "`{keyword}` reads its `mat_slot=`, and `{one}` builds what `{other}` builds, so \
                 the two names cannot tell a reader from a non-reader:\n{}",
                palettes(&first),
            );
        }
    }
}

#[test]
fn every_role_whose_mat_slot_is_unread_is_built_here() {
    let mut from_table: Vec<&str> = known_keywords()
        .iter()
        .copied()
        .filter(|keyword| role_of(keyword).unread_argument("mat_slot").is_some())
        .collect();
    from_table.sort_unstable();
    let mut from_fixtures: Vec<&str> = FIXTURES
        .iter()
        .map(|(keyword, _)| *keyword)
        .filter(|keyword| role_of(keyword).unread_argument("mat_slot").is_some())
        .collect();
    from_fixtures.sort_unstable();
    assert_eq!(from_fixtures, from_table);
}
