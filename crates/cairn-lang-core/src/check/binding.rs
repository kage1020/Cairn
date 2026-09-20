//! `binding` pass — flags a `-> value` tail on a member that cannot emit a
//! signal.
//!
//! The fourth field of a member line, and the last one with no check of its
//! own. [`super::arguments`] judges the `key=value` list and the member's
//! own `[key=value]`; `check::positional` reads the bare values; and the
//! tail was left to `synth`'s Logic IR walk, which only
//! `cairn synth --experimental-logic-synth` reaches. So
//! `walls ... -> sig.w` was silent through `check` and `compile`, built a
//! wall with no trace of the binding, and left the signal it named driven
//! by nothing.
//!
//! What this pass asks is the host question alone: may a member of this
//! role carry a tail at all. [`crate::intent::SENSOR_HOSTS`] answers it,
//! and the answer needs no Logic IR — which is the whole reason the rule
//! can run here. The two questions that do need one stay in
//! `cairn-lang-redstone`: whether the value names a signal
//! (`E_LOGIC_INVALID_SIGNAL`), and whether the signal it names is driven
//! twice or by nobody.
//!
//! The host is asked before the value for the same reason it is in that
//! crate: no edit to the value makes a `walls` emit, so telling the author
//! about the `sig.` namespace would send them round the loop to be told
//! about the host next time. A tail on a `pressure_plate` passes here
//! whatever it names, and the value-side refusal is the redstone
//! pipeline's to make.
//!
//! A member whose keyword the role table does not know is left alone, the
//! same way [`super::arguments`] leaves one: `E_UNKNOWN_KEYWORD` owns the
//! line, and "this member cannot emit a signal" would be a second finding
//! about a component that does not exist. `lever ... -> sig.w` is that
//! case today — a sensor `spec/redstone` "Signal binding" lists and the
//! surface has not reached — and telling its author about the host would
//! be telling them the wrong thing.

use crate::ast::SIGNAL_HEAD;
use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole, SENSOR_HOSTS};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for s in &ir.structs {
        walk(&s.members, sink);
    }
    for d in &ir.defs {
        walk(&d.members, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, sink);
    }
}

fn walk(members: &[Member], sink: &mut DiagnosticSink) {
    for m in members {
        check_member(m, sink);
        walk(&m.children.members, sink);
    }
}

fn check_member(member: &Member, sink: &mut DiagnosticSink) {
    let Some(binding) = &member.binding else {
        return;
    };
    // The keyword is the repair; whether it may emit is a question about a
    // component that does not exist.
    if matches!(member.role, MemberRole::Other(_)) {
        return;
    }
    let keyword = member.role.keyword();
    if SENSOR_HOSTS.contains(&keyword) {
        return;
    }
    sink.push(misplaced_binding(keyword, &binding.span));
}

/// A tail on a member that is not a sensor.
///
/// The repair names the hosts that exist today and the ones the
/// specification has reserved, because an author who wrote a tail on a
/// `walls` may have meant either: to move it onto the plate they already
/// have, or to write the `lever` the surface has not reached yet.
fn misplaced_binding(keyword: &str, span: &Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::MisplacedBinding,
        span: span.clone(),
        primary: format!(
            "`{keyword}` cannot emit a signal; only a sensor carries a \
             `-> {SIGNAL_HEAD}.<name>` tail",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            // Word for word the repair `cairn synth` gave, because it is
            // the same finding moved and not a second opinion about the
            // same line.
            message: format!(
                "move the tail onto a sensor — `{hosts}` {verb} the sensor {noun} the surface \
                 accepts today; `spec/redstone` \"Signal binding\" also lists `lever`, \
                 `button`, `daylight`, and `observer`",
                hosts = SENSOR_HOSTS.join("`, `"),
                verb = if SENSOR_HOSTS.len() == 1 { "is" } else { "are" },
                noun = if SENSOR_HOSTS.len() == 1 {
                    "keyword"
                } else {
                    "keywords"
                },
            ),
        }],
        data: None,
    }
}
