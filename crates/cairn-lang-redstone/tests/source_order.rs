//! The synth walk has to come out in the order the file reads.
//!
//! Two readers depend on it and say so. `LogicIr`'s node list is
//! documented as reading top-to-bottom the way the source does, and
//! `ScopedLogicIr` as holding its scopes "in source order across the whole
//! module". Neither held: `IntentModule` files scopes into three vectors by
//! kind, and a body into a members vector and a logic vector, so the walk
//! recovered a grouping rather than an order.
//!
//! The visible cost was not the dump. `E_LOGIC_MULTIPLE_DRIVERS` reports at
//! the redefinition and notes "first declared here" at the original — so
//! when a `level`-nested binding was collected ahead of a top-level one
//! written above it, the finding accused the earlier line of redefining
//! what the later line declared, and told the author to look down the file
//! for the definition that was above them all along.

use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{DiagnosticCode, ScopeKind, SignalRef, SynthOutput, synthesize};

fn synth_source(source: &str) -> SynthOutput {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    synthesize(&intent)
}

/// 1-based line of a byte offset, counted the way the source reads.
fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

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
        "the binding on line 6 is the first declaration and the one on \
         line 8, inside the `level`, is the redefinition",
    );
}

#[test]
fn a_nested_binding_above_a_top_level_one_is_the_first_declaration() {
    // The mirror. Without it the test above would also pass under a rule
    // that always blames whichever binding is nested.
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
    // Two independent signals, so nothing is a redefinition and the only
    // thing left to observe is the node order. `sig.x` is written first at
    // the top level and `sig.y` second inside a `level`; the dump must
    // agree with the file rather than with the nesting.
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
    // `signal_defs` is the map documented as reading "top-to-bottom the
    // same way the source does", and its values are the node indices, so
    // it pins both the listing order and the numbering at once.
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
        "the walk follows the file, not the three vectors the module files \
         scopes into",
    );
}

#[test]
fn a_struct_written_above_a_def_is_still_walked_first() {
    // The mirror of the test above: the old walk emitted structs first, so
    // that one alone would pass under "always structs last" just as badly.
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
    // The dump order is one reader; the diagnostics are the other, and they
    // are what an author reads first. A `def` above a `struct` must have
    // its finding reported above the struct's.
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
        "the def's unused-signal warning is on line 5 and the struct's on \
         line 9: {:?}",
        out.diagnostics,
    );
}

/// A module with all three scope kinds, written in an order none of the
/// three vectors reproduces: `def`, then `site`, then `struct`. The old
/// walk emitted struct, then def, then site, so this puts every pair the
/// wrong way round at once.
///
/// Each scope carries exactly one finding, on a line of its own. The two
/// body scopes leave a signal nothing consumes; the site names one nothing
/// declares, which is all a site body can do — it has no sensor to bind
/// one.
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

/// The mirror: the same three scopes, reversed.
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
    // A `site` body carries no sensor, so its `logic` line can only name
    // an unbound signal — which is exactly what makes it observable here:
    // one finding per scope, and the site's has to land between the other
    // two. The scope list cannot show this, because a scope whose
    // collection failed is elided from it by design.
    assert_eq!(
        finding_scopes(DEF_SITE_STRUCT),
        ["def=hut@8", "site=plaza@12", "struct=tower@16"],
    );
}

#[test]
fn reversing_the_three_scopes_reverses_the_walk() {
    // Without the mirror, a rule that happened to emit def, then site,
    // then struct would satisfy the test above.
    assert_eq!(
        finding_scopes(STRUCT_SITE_DEF),
        ["struct=tower@8", "site=plaza@12", "def=hut@16"],
    );
}
