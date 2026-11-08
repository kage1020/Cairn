//! `E_NO_PORTABLE_IMPL`: a component of the built struct that reads another through a block
//! update (`spec/redstone` "Edition differences").

use cairn_lang_core::block_array::{
    BlockArrayIr, BlockIdSet, BlockState, TargetRegistry, lower_to_block_array,
};
use cairn_lang_core::{Edition, IntentModule, lower, parse, resolve};
use cairn_lang_redstone::verify_portable;

/// A target that spells every token as the block of its name, so the walls stand as written.
struct Named;

impl TargetRegistry for Named {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        let name = token.split('.').next()?;
        Some(BlockState::bare(format!("minecraft:{name}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }
}

/// `src` lowered, and built for `edition`.
fn built(src: &str, edition: Edition) -> (IntentModule, BlockArrayIr) {
    let intent = lower(&parse(src).expect("parse"));
    let built = lower_to_block_array(&intent, &resolve(&intent, Some(edition)), Some(&Named));
    (intent, built)
}

/// A 7x5 struct of 4-row walls under a flat roof one voxel past them, then `members`; the
/// overhang puts the back wall at `z=1`, and along it `offset=2` is `x=5`.
fn room(members: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n\n\
         struct s size=7x5\n  walls mat_slot=wall height=4\n  \
         roof kind=flat mat_slot=wall overhang=1\n{members}  circuit region=floor void=2\n"
    )
}

/// Each finding on `edition` as the text it points at, its primary and its notes.
fn found(src: &str, edition: Edition) -> Vec<(String, String, Vec<String>)> {
    let (intent, built) = built(src, edition);
    verify_portable(&intent, &built, edition)
        .into_iter()
        .map(|d| {
            assert_eq!(d.code.as_str(), "E_NO_PORTABLE_IMPL");
            (
                src[d.span.clone()].to_owned(),
                d.primary,
                d.notes.into_iter().map(|note| note.message).collect(),
            )
        })
        .collect()
}

/// The finding a piston or dispenser line gets on Java when `lever` powers the cell above it.
fn quasi_connected(actuator: &str, keyword: &str) -> (String, String, Vec<String>) {
    let named = format!("the {keyword} `p`");
    (
        actuator.to_owned(),
        format!(
            "struct=s: on java, {named} also takes power from the lever `sw` through the cell \
             above it, and acts on it only once a block update reaches it: this circuit requires \
             update-order (quasi-connectivity) semantics"
        ),
        vec![
            format!("the lever `sw` powers the cell above {named} from here"),
            "bedrock has no quasi-connectivity, so this is no dependence on order there".to_owned(),
            format!(
                "Fix: move the lever `sw` so that it powers no block beside the cell above \
                 {named}, or redesign the logic to be order-independent; or drop to Tier 0 with \
                 an `@edition java` guard"
            ),
        ],
    )
}

#[test]
fn a_java_piston_or_dispenser_a_sensor_powers_only_through_the_cell_above_it_is_refused() {
    for (actuator, keyword) in [
        (
            "piston id=p at=inside.back offset=2 y=1 powered_by=sig.p",
            "piston",
        ),
        (
            "dispenser id=p at=inside.back offset=2 y=1 fired_by=sig.p",
            "dispenser",
        ),
    ] {
        // Beside the cell above, two rows up, and diagonally above, by the block it hangs on.
        for lever in [
            "lever id=sw at=inside.back offset=2 y=3 -> sig.a",
            "lever id=sw at=inside.back offset=3 y=2 -> sig.a",
            "lever id=sw at=back.outside offset=2 y=2 -> sig.a",
        ] {
            let src = room(&format!(
                "  {actuator}\n  {lever}\n  logic sig.p = not sig.a\n"
            ));
            assert_eq!(
                found(&src, Edition::Java),
                [quasi_connected(actuator, keyword)],
                "{lever}"
            );
            assert_eq!(found(&src, Edition::Bedrock), [], "{lever}");
        }
    }
}

#[test]
fn a_piston_a_sensor_powers_directly_or_not_at_all_is_left_alone() {
    for (actuator, lever) in [
        // The lever above the piston powers it on both editions.
        (
            "piston id=p at=inside.back offset=2 y=1 powered_by=sig.p",
            "lever id=sw at=inside.back offset=2 y=2 -> sig.a",
        ),
        // Too far to reach the cell above.
        (
            "piston id=p at=inside.back offset=2 y=1 powered_by=sig.p",
            "lever id=sw at=inside.back offset=4 y=2 -> sig.a",
        ),
        // A piston no binding drives is no part of the circuit.
        (
            "piston id=p at=inside.back offset=2 y=1",
            "lever id=sw at=inside.back offset=2 y=3 -> sig.a",
        ),
    ] {
        let src = room(&format!(
            "  {actuator}\n  {lever}\n  lamp id=lp at=inside.front offset=1 y=1 lit_by=sig.a\n  \
             logic sig.p = not sig.a\n"
        ));
        assert_eq!(found(&src, Edition::Java), [], "{actuator}\n{lever}");
    }
}

#[test]
fn a_tier0_part_beside_the_cell_above_a_piston_is_the_authors() {
    let src = room(
        "  piston id=p at=inside.back offset=2 y=1 powered_by=sig.p\n  \
         redstone_torch at=inside.back offset=2 y=3\n  \
         lever id=sw at=inside.front offset=1 y=1 -> sig.a\n  logic sig.p = sig.a\n",
    );
    assert_eq!(found(&src, Edition::Java), []);
}

#[test]
fn an_observer_watching_what_the_circuit_drives_is_refused_on_every_edition() {
    let watches_the_lamp = (
        "  lamp id=lp at=inside.back offset=2 y=2 lit_by=sig.l\n",
        "observer id=ob at=inside.back offset=3 y=2 facing=left -> sig.o",
        "the lamp `lp`",
        "lamp id=lp at=inside.back offset=2 y=2 lit_by=sig.l",
        "sig.l",
    );
    let watches_the_head = (
        "  piston id=pi at=inside.back offset=2 y=1 powered_by=sig.l\n",
        "observer id=ob at=inside.front offset=4 y=1 facing=in -> sig.o",
        "the piston `pi`",
        "piston id=pi at=inside.back offset=2 y=1 powered_by=sig.l",
        "sig.l",
    );
    for (actuator, observer, named, _, signal) in [watches_the_lamp, watches_the_head] {
        let src = room(&format!(
            "{actuator}  {observer}\n  lever id=sw at=inside.front offset=1 y=2 -> sig.a\n  \
             logic sig.l = sig.a and not sig.o\n"
        ));
        for edition in [Edition::Java, Edition::Bedrock] {
            assert_eq!(
                found(&src, edition),
                [(
                    observer.to_owned(),
                    format!(
                        "struct=s: this observer watches {named}, which the circuit drives, so \
                         its signal is the circuit's own change read back through a block \
                         update: this circuit requires update-order (BUD) semantics"
                    ),
                    vec![
                        format!("{named} is driven by {signal} here"),
                        format!(
                            "Fix: redesign the logic to be order-independent: an observer \
                             pulses on each change, as `edge_rising({signal}) or \
                             edge_falling({signal})` does in `logic`; or drop to Tier 0 with an \
                             `@edition` guard"
                        ),
                    ],
                )],
                "{observer} on {edition:?}",
            );
        }
    }
}

#[test]
fn the_note_points_at_the_binding_that_drives_the_actuator() {
    let src = room(
        "  door id=d side=front at=center\n  door[id=d] opened_by=sig.l\n  \
         observer id=ob at=inside.front offset=3 y=1 facing=out -> sig.o\n  \
         lever id=sw at=inside.back offset=1 y=1 -> sig.a\n  logic sig.l = sig.a or sig.o\n",
    );
    let (intent, built) = built(&src, Edition::Java);
    let pointed: Vec<(&str, Vec<Option<&str>>)> = verify_portable(&intent, &built, Edition::Java)
        .iter()
        .map(|d| {
            (
                &src[d.span.clone()],
                d.notes
                    .iter()
                    .map(|note| note.span.clone().map(|span| &src[span]))
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        pointed,
        [(
            "observer id=ob at=inside.front offset=3 y=1 facing=out -> sig.o",
            vec![Some("sig.l"), None],
        )],
    );
}

#[test]
fn an_observer_with_no_tail_or_watching_what_nothing_drives_is_left_alone() {
    for (lamp, observer) in [
        (
            "lamp id=lp at=inside.back offset=2 y=2",
            "observer id=ob at=inside.back offset=3 y=2 facing=left -> sig.o",
        ),
        (
            "lamp id=lp at=inside.back offset=2 y=2 lit_by=sig.o",
            "observer id=ob at=inside.back offset=3 y=2 facing=left",
        ),
        (
            "lamp id=lp at=inside.back offset=2 y=2 lit_by=sig.o",
            "observer id=ob at=inside.back offset=3 y=2 facing=right -> sig.o",
        ),
    ] {
        let src = room(&format!(
            "  {lamp}\n  {observer}\n  lever id=sw at=inside.front offset=1 y=2 -> sig.a\n  \
             logic sig.x = sig.a\n  lamp id=l2 at=inside.front offset=2 y=2 lit_by=sig.x\n"
        ));
        for edition in [Edition::Java, Edition::Bedrock] {
            assert_eq!(found(&src, edition), [], "{lamp}\n{observer}");
        }
    }
}

#[test]
fn a_def_placed_twice_is_reported_once_under_the_def() {
    let src = "theme t:\n  slot wall -> @cobblestone\n\n\
               def hut size=7x5:\n  walls mat_slot=wall height=4\n  \
               roof kind=flat mat_slot=wall overhang=1\n  \
               dispenser id=p at=inside.back offset=2 y=1 fired_by=sig.p\n  \
               lever id=sw at=inside.back offset=2 y=3 -> sig.a\n  logic sig.p = not sig.a\n  \
               circuit region=floor void=2\n\n\
               site v:\n  place id=a use=hut theme=t at=origin\n  \
               place id=b use=hut theme=t east_of=a gap=2\n";
    let (actuator, primary, notes) = quasi_connected(
        "dispenser id=p at=inside.back offset=2 y=1 fired_by=sig.p",
        "dispenser",
    );
    assert_eq!(
        found(src, Edition::Java),
        [(actuator, primary.replace("struct=s:", "def=hut:"), notes)],
    );
}
