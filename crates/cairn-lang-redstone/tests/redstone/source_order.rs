//! The synth walk follows the file's order, not the vectors `IntentModule` files scopes into.
//! Each test has a mirror, so no fixed-order rule passes both.

use cairn_lang_redstone::{DiagnosticCode, ScopeKind, SignalRef};

use crate::common::{line_of, synth_source};

const NESTED_AFTER_TOP_LEVEL: &str = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.dup = sig.a and sig.b
  level y=0
    logic sig.dup = sig.a or sig.b
  door id=front side=front at=center
  door[id=front] opened_by=sig.dup
";

#[test]
fn a_nested_binding_below_a_top_level_one_is_the_redefinition() {
    let out = synth_source(NESTED_AFTER_TOP_LEVEL);
    let duplicates: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicMultipleDrivers)
        .collect();
    assert_eq!(
        duplicates.len(),
        1,
        "expected one E_LOGIC_MULTIPLE_DRIVERS, got: {:?}",
        out.diagnostics,
    );
    let d = duplicates[0];
    let note = d
        .notes
        .iter()
        .find(|n| n.message == "first declared here")
        .expect("the duplicate names the first declaration");
    let first = note.span.as_ref().expect("the note carries a span");
    assert_eq!(
        (
            line_of(NESTED_AFTER_TOP_LEVEL, first.start),
            line_of(NESTED_AFTER_TOP_LEVEL, d.span.start),
        ),
        (6, 8),
        "line 6 is the first declaration and line 8 the redefinition",
    );
}

#[test]
fn a_nested_binding_above_a_top_level_one_is_the_first_declaration() {
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  level y=0
    logic sig.dup = sig.a or sig.b
  logic sig.dup = sig.a and sig.b
  door id=front side=front at=center
  door[id=front] opened_by=sig.dup
";
    let out = synth_source(source);
    let d = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::LogicMultipleDrivers)
        .expect("one E_LOGIC_MULTIPLE_DRIVERS");
    let first = d
        .notes
        .iter()
        .find(|n| n.message == "first declared here")
        .and_then(|n| n.span.as_ref())
        .expect("the note carries a span");
    assert_eq!(
        (line_of(source, first.start), line_of(source, d.span.start)),
        (7, 8),
    );
}

#[test]
fn permuting_a_nested_and_a_top_level_binding_swaps_which_one_is_reported() {
    // No redefinition here, so only the node order is observed.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.x = sig.a and sig.b
  level y=0
    logic sig.y = sig.x or sig.a
  door id=front side=front at=center
  door[id=front] opened_by=sig.y
";
    let out = synth_source(source);
    let entry = out.scoped.scopes.first().expect("one scope");
    let defs: Vec<(String, SignalRef)> = entry
        .ir
        .signal_defs
        .iter()
        .map(|(name, def)| (name.tail().join("."), *def))
        .collect();
    let gate_index = |signal: &str| {
        defs.iter().find(|(name, _)| name == signal).map_or_else(
            || panic!("`sig.{signal}` must reach the IR, got {defs:?}"),
            |(_, def)| *def,
        )
    };
    assert_eq!(
        (gate_index("x"), gate_index("y")),
        (SignalRef::Gate(0), SignalRef::Gate(1)),
        "`sig.x` is written above `sig.y`, so it is the earlier node: {defs:?}",
    );
}

#[test]
fn a_def_written_above_a_struct_is_walked_first() {
    let source = "\
@cairn 2026.06

def hut size=1x1:
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.d
  door id=front side=front at=center
  door[id=front] opened_by=sig.d

struct tower size=1x1
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.s
  door id=front side=front at=center
  door[id=front] opened_by=sig.s
";
    let out = synth_source(source);
    let order: Vec<(ScopeKind, &str)> = out
        .scoped
        .scopes
        .iter()
        .map(|s| (s.kind, s.name.as_str()))
        .collect();
    assert_eq!(
        order,
        [(ScopeKind::Def, "hut"), (ScopeKind::Struct, "tower")],
        "the walk follows the file",
    );
}

#[test]
fn a_struct_written_above_a_def_is_still_walked_first() {
    let source = "\
@cairn 2026.06

struct tower size=1x1
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.s
  door id=front side=front at=center
  door[id=front] opened_by=sig.s

def hut size=1x1:
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.d
  door id=front side=front at=center
  door[id=front] opened_by=sig.d
";
    let out = synth_source(source);
    let order: Vec<(ScopeKind, &str)> = out
        .scoped
        .scopes
        .iter()
        .map(|s| (s.kind, s.name.as_str()))
        .collect();
    assert_eq!(
        order,
        [(ScopeKind::Struct, "tower"), (ScopeKind::Def, "hut")],
    );
}

#[test]
fn findings_from_two_scopes_come_out_in_line_order() {
    let source = "\
@cairn 2026.06

def hut size=1x1:
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.d
  logic sig.q = sig.d and sig.d

struct tower size=1x1
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.s
  logic sig.r = sig.s and sig.s
";
    let out = synth_source(source);
    let lines: Vec<usize> = out
        .diagnostics
        .iter()
        .map(|d| line_of(source, d.span.start))
        .collect();
    assert_eq!(
        lines,
        [5, 9],
        "one finding per scope, in line order: {:?}",
        out.diagnostics,
    );
}

/// One scope of each kind, in an order no walk over the per-kind vectors reproduces. The site's
/// `sig.q` is unbound on purpose: a site body has no sensor to bind it.
const DEF_SITE_STRUCT: &str = concat!(
    "@cairn 2026.06
",
    "
",
    "theme t:
",
    "  slot wall -> @cobblestone
",
    "
",
    "def hut size=1x1:
",
    "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.d
",
    "  logic sig.dd = sig.d and sig.d
",
    "
",
    "site plaza:
",
    "  place id=one use=hut theme=t at=origin
",
    "  logic sig.ss = sig.q and sig.q
",
    "
",
    "struct tower size=1x1
",
    "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.t
",
    "  logic sig.tt = sig.t and sig.t
",
);

const STRUCT_SITE_DEF: &str = concat!(
    "@cairn 2026.06
",
    "
",
    "theme t:
",
    "  slot wall -> @cobblestone
",
    "
",
    "struct tower size=1x1
",
    "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.t
",
    "  logic sig.tt = sig.t and sig.t
",
    "
",
    "site plaza:
",
    "  place id=one use=hut theme=t at=origin
",
    "  logic sig.ss = sig.q and sig.q
",
    "
",
    "def hut size=1x1:
",
    "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.d
",
    "  logic sig.dd = sig.d and sig.d
",
);

fn finding_scopes(source: &str) -> Vec<String> {
    synth_source(source)
        .diagnostics
        .iter()
        .map(|d| {
            let label = d.primary.split(':').next().expect("a scope label prefix");
            format!("{}@{}", label, line_of(source, d.span.start))
        })
        .collect()
}

#[test]
fn three_scope_kinds_are_walked_in_the_order_the_file_writes_them() {
    assert_eq!(
        finding_scopes(DEF_SITE_STRUCT),
        ["def=hut@8", "site=plaza@12", "struct=tower@16"],
    );
}

#[test]
fn reversing_the_three_scopes_reverses_the_walk() {
    assert_eq!(
        finding_scopes(STRUCT_SITE_DEF),
        ["struct=tower@8", "site=plaza@12", "def=hut@16"],
    );
}
