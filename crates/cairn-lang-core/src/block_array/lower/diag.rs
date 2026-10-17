//! Findings about a body or member lowering skips, and about a scope
//! with no theme to paint with.

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{DefIr, Member, MemberRole, StructIr};

use crate::block_array::args::diag_deferred_member_reason;

/// Which lowering entry point produced this body: what is voxelised, not what was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VoxelSource {
    Struct,
    Place,
}

pub(super) fn diag_struct_no_size(s: &StructIr) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::StructNoSize,
        span: s.span.clone(),
        primary: format!(
            "struct `{}` has no `size=WxH`; block-array lowering skipped it",
            s.name,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "add a `size=WxH` header to give the struct a voxel footprint".to_owned(),
        }],
        data: None,
    }
}

pub(super) fn diag_no_theme_bound_generic(
    kind: VoxelSource,
    label: &str,
    header_span: &Span,
) -> Diagnostic {
    let host = match kind {
        VoxelSource::Struct => "struct",
        VoxelSource::Place => "place",
    };
    Diagnostic {
        code: DiagnosticCode::NoThemeBound,
        span: header_span.clone(),
        primary: format!(
            "{host} `{label}` has no theme bound, so no `mat_slot=` in it resolves to a block",
        ),
        notes: vec![
            DiagnosticNote {
                span: None,
                message: "a `floor` is left air; no `walls` is built, so every `door` and \
                          `window` is refused for want of a wall; a `roof`, eave `stair` or \
                          `pressure_plate` is built from its default block"
                    .to_owned(),
            },
            DiagnosticNote {
                span: None,
                // Names what makes a theme bind, since this pass cannot tell which cause applies.
                message: "a scope binds a theme when the module declares exactly one logical \
                          theme, or the `place` names one with `theme=`; under a `--edition` pin \
                          that theme must also have a variant for that edition"
                    .to_owned(),
            },
        ],
        data: None,
    }
}

pub(super) fn diag_def_no_size(def: &DefIr) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DefNoSize,
        span: def.span.clone(),
        primary: format!(
            "def `{}` has no `size=WxH`; placements that `use={}` cannot derive a voxel footprint",
            def.name, def.name,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "add a `size=WxH` header to give the def a voxel footprint".to_owned(),
        }],
        data: None,
    }
}

pub(super) fn diag_deferred_member(member: &Member) -> Diagnostic {
    let role = MemberRole::keyword(&member.role);
    diag_deferred_member_reason(
        member,
        &format!("`{role}` is not yet handled by block-array lowering"),
    )
}
