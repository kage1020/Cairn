//! `E_OUTPUT_NAME_COLLISION`: the names a build would write its artifacts under.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::ids::{WalkwayEndpoint, WalkwayScopeKey, artifact_stem};
use crate::intent::{IntentModule, MemberRole};

use super::resolution::{PortRef, ScopeResolution, ValidatedConnect, place_scope_key, struct_key};

/// Two scopes a build would write to one file, compared by [`artifact_stem`] folded to lower case
/// so `Hut` and `hut` collide on every host. A scope key seen twice is another code's finding.
pub(super) fn check_output_names(
    ir: &IntentModule,
    scopes: &IndexMap<String, ScopeResolution>,
    connects: &[ValidatedConnect],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut seen: IndexMap<String, (String, OutputArtifact)> = IndexMap::new();
    for artifact in output_artifacts(ir, scopes, connects) {
        if !seen_keys.insert(artifact.key.clone()) {
            continue;
        }
        let stem = artifact_stem(&artifact.key);
        let folded = stem.to_lowercase();
        let Some((first_stem, first)) = seen.get(&folded) else {
            seen.insert(folded, (stem, artifact));
            continue;
        };
        let why = if *first_stem == stem {
            format!(
                "both would be written to `{stem}` in the output directory, with the edition's \
                 extension, and if both are built, a build can keep only one; rename one of them"
            )
        } else {
            format!(
                "`{first_stem}` and `{stem}` differ only in case, which makes them one file on \
                 the case-insensitive file systems macOS and Windows use by default; rename one \
                 so the names differ by more than case"
            )
        };
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::OutputNameCollision,
            span: artifact.span,
            primary: format!(
                "{} would be written to the same file as {}",
                artifact.label, first.label
            ),
            notes: vec![
                DiagnosticNote {
                    span: Some(first.span.clone()),
                    message: format!("{} is declared here", first.label),
                },
                DiagnosticNote {
                    span: None,
                    message: why,
                },
            ],
            data: None,
        });
    }
}

struct OutputArtifact {
    key: String,
    span: Span,
    label: String,
}

fn output_artifacts(
    ir: &IntentModule,
    scopes: &IndexMap<String, ScopeResolution>,
    connects: &[ValidatedConnect],
) -> Vec<OutputArtifact> {
    let structs = ir
        .structs
        .iter()
        .filter(|s| s.size.is_some())
        .map(|s| OutputArtifact {
            key: struct_key(s),
            span: s.span.clone(),
            label: format!("`struct {}`", s.name),
        });
    let places: Vec<OutputArtifact> = ir
        .sites
        .iter()
        .flat_map(|site| {
            site.placements
                .iter()
                .filter(|m| matches!(m.role, MemberRole::Place))
                .filter_map(move |m| {
                    let id = m.id.as_deref()?;
                    let key = place_scope_key(&site.name, id);
                    // The lookup lowering makes before it skips the placement with `W_DEF_NO_SIZE`.
                    let sized = m
                        .intent_state
                        .get("use")
                        .and_then(|v| v.value.as_label_str())
                        .and_then(|name| ir.defs.iter().find(|d| d.name == name))
                        .is_some_and(|d| d.size.is_some());
                    (sized && scopes.contains_key(&key)).then(|| OutputArtifact {
                        key,
                        span: m.span.clone(),
                        label: format!("`place id={id}` in site `{}`", site.name),
                    })
                })
        })
        .collect();
    let counted_places: HashSet<String> = places.iter().map(|p| p.key.clone()).collect();
    let mut seen_pairs = HashSet::new();
    let walkways = connects.iter().filter_map(|c| {
        let lowers = |end: &PortRef| {
            counted_places.contains(&place_scope_key(c.site.as_str(), end.place.as_str()))
        };
        if !lowers(&c.from) || !lowers(&c.to) {
            return None;
        }
        let endpoint = |end: &PortRef| WalkwayEndpoint {
            place: end.place.clone(),
            port: end.port.clone(),
        };
        let Ok(key) = WalkwayScopeKey::from_parts(&c.site, &endpoint(&c.from), &endpoint(&c.to))
        else {
            // Lowering refuses this key with `W_INVALID_WALKWAY_IDENT` and writes no file.
            return None;
        };
        // Recorded only here, as lowering records a pair only once it lays it.
        seen_pairs.insert(c.walkway_pair()).then(|| OutputArtifact {
            key: key.as_str().to_owned(),
            span: c.span.clone(),
            label: format!("the walkway `{} ↔ {}` in site `{}`", c.from, c.to, c.site),
        })
    });

    structs.chain(places).chain(walkways).collect()
}
