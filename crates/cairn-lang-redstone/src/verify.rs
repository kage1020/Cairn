//! `spec/redstone` "Verification": each assertion against the legalized circuit of its scope, a
//! latency bound by the ticks of its slowest path, a truth table and a temporal assertion by
//! running the circuit.

use indexmap::IndexMap;
use indexmap::map::Entry;

use cairn_lang_core::intent::{AssertIr, IntentModule, Member};

use crate::logic_ir::ScopeKind;

mod latency;
mod truth_and_temporal;

pub use latency::verify_latency;
pub use truth_and_temporal::{MAX_ASSERTION_SENSORS, verify_truth_and_temporal};

/// Each scope's `assert` lines in source order, those under its members included, keyed as a
/// [`crate::ScopedCircuitLayout`] entry names its scope, in the module's order of scopes; a name
/// given twice keeps the first scope.
fn scope_asserts(module: &IntentModule) -> IndexMap<(ScopeKind, &str), Vec<&AssertIr>> {
    fn collect<'a>(asserts: &'a [AssertIr], members: &'a [Member], out: &mut Vec<&'a AssertIr>) {
        out.extend(asserts);
        for member in members {
            collect(&member.children.asserts, &member.children.members, out);
        }
    }
    let scopes = module
        .structs
        .iter()
        .map(|s| (ScopeKind::Struct, s.name.as_str(), &s.asserts, &s.members))
        .chain(
            module
                .defs
                .iter()
                .map(|d| (ScopeKind::Def, d.name.as_str(), &d.asserts, &d.members)),
        )
        .chain(
            module
                .sites
                .iter()
                .map(|s| (ScopeKind::Site, s.name.as_str(), &s.asserts, &s.placements)),
        );
    let mut out: IndexMap<(ScopeKind, &str), Vec<&AssertIr>> = IndexMap::new();
    for (kind, name, asserts, members) in scopes {
        let Entry::Vacant(slot) = out.entry((kind, name)) else {
            continue;
        };
        let mut found = Vec::new();
        collect(asserts, members, &mut found);
        found.sort_by_key(|a| (a.span().start, a.span().end));
        slot.insert(found);
    }
    out
}
