//! The `binding` pass: a `-> value` tail on a member that cannot emit a signal. Whether the value
//! names a signal needs a Logic IR, so `cairn-lang-redstone` checks that.

use crate::ast::SIGNAL_HEAD;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole, SENSOR_HOSTS};
use crate::prose::or_list;

use super::DiagnosticSink;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for body in ir.bodies() {
        walk(body.members, sink);
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
    // An unknown keyword's own finding carries the repair.
    if matches!(member.role, MemberRole::Other(_)) {
        return;
    }
    let keyword = member.role.keyword();
    if !SENSOR_HOSTS.contains(&keyword) {
        sink.push(misplaced_binding(keyword, &binding.span));
    } else if member.selector.is_some() {
        sink.push(tail_on_selector(keyword, &binding.span));
    }
}

/// A tail on a sensor keyword written in the selector form (`spec/redstone` "Signal binding").
fn tail_on_selector(keyword: &str, span: &Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::MisplacedBinding,
        span: span.clone(),
        primary: format!(
            "`{keyword}[...]` is the selector form, which carries no sensor binding; only a \
             sensor's own line carries a `-> {SIGNAL_HEAD}.<name>` tail",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "write the tail on the `{keyword}` line that declares the sensor, without \
                 brackets; an actuator patch such as `door[id=<label>] opened_by=` is the only \
                 binding a selector line carries",
            ),
        }],
        data: None,
    }
}

/// A tail on a member that is not a sensor.
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
            message: format!(
                "move the tail onto a sensor, a {} line",
                or_list(
                    &SENSOR_HOSTS
                        .iter()
                        .map(|host| format!("`{host}`"))
                        .collect::<Vec<_>>()
                )
                .unwrap_or_default(),
            ),
        }],
        data: None,
    }
}
