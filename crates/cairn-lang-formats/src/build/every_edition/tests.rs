use cairn_lang_core::{lower, parse};

use super::*;

/// An `item_display` with no `item=`, which Java refuses for the missing item and Bedrock for the
/// display, then `extra` on its line.
fn trophy(extra: &str) -> String {
    format!(
        "theme t:\n  slot floor -> @oak_planks\n  slot wall -> @stone_bricks\n\n\
         struct room size=5x5:\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n  \
         item_display id=a at=1,1,1{extra}\n"
    )
}

/// A dispenser Java refuses for its lever's quasi-connectivity and Bedrock for the lever no
/// route reaches.
const DISPENSER: &str = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=7x5\n  \
                         floor mat_slot=wall\n  walls mat_slot=wall height=4\n  \
                         roof kind=flat mat_slot=wall overhang=1\n  \
                         dispenser id=p at=inside.back offset=2 y=1 fired_by=sig.p\n  \
                         lever id=sw at=inside.back offset=2 y=3 -> sig.a\n  \
                         logic sig.p = not sig.a\n  circuit region=floor void=4\n";

/// Each error [`refused_by_every_edition`] reports on `source`, as its code, the line its span
/// starts on and its last note.
fn refused(source: &str, circuits: bool) -> Vec<(&'static str, usize, String)> {
    let module = parse(source).expect("the source parses");
    refused_by_every_edition(&module, &lower(&module), circuits)
        .iter()
        .map(|d| {
            let line = source[..d.span_start()].matches('\n').count() + 1;
            let note = d.notes().last().expect("a note").message.clone();
            (d.code_str(), line, note)
        })
        .collect()
}

fn raised_for(edition: &str) -> String {
    format!(
        "raised for `--edition {edition}`; every edition refuses this source, each with errors \
         of its own"
    )
}

#[test]
fn a_line_each_edition_refuses_with_its_own_code_reports_each_editions_error() {
    assert_eq!(
        refused(&trophy(""), true),
        [
            ("E_INVALID_MEMBER", 8, raised_for("java")),
            ("E_PARITY_UNSUPPORTED", 8, raised_for("bedrock")),
        ],
    );
}

#[test]
fn a_source_each_edition_refuses_at_its_own_line_reports_each_editions_circuit_error() {
    assert_eq!(
        refused(DISPENSER, true),
        [
            ("E_NO_PORTABLE_IMPL", 8, raised_for("java")),
            ("E_ROUTE_UNREACHABLE_COMPONENT", 9, raised_for("bedrock")),
        ],
    );
    assert_eq!(refused(DISPENSER, false), []);
}

#[test]
fn a_source_one_edition_builds_reports_nothing() {
    assert_eq!(refused(&trophy(" item=diamond_sword"), true), []);
}
