//! `E_OUTPUT_NAME_COLLISION`: the names a build would write its artifacts
//! under, compared once every scope is resolved.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::ids::{WalkwayEndpoint, WalkwayScopeKey, artifact_stem};
use crate::intent::{IntentModule, MemberRole};

use super::resolution::{PortRef, ScopeResolution, ValidatedConnect};
use super::resolver::{place_scope_key, struct_key};

/// `E_OUTPUT_NAME_COLLISION`: two scopes the build would write to one
/// file.
///
/// The artifacts are the ones a build would write, as far as resolution
/// can tell:
///
/// - every `struct` with a `size=`. Lowering drops a sizeless one with
///   `W_STRUCT_NO_SIZE`.
/// - every `place` that resolved to a scope and whose `def` has a
///   `size=`. Lowering drops a placement of a sizeless `def` with
///   `W_DEF_NO_SIZE`.
/// - one walkway per endpoint pair, [`ValidatedConnect::walkway_pair`],
///   named after the first row for the pair whose scope key can be built,
///   when both of its places are counted above. Lowering lays a pair
///   once and reports a later row for it as `W_DUPLICATE_WALKWAY`, and
///   lays nothing for a row whose key it cannot build or whose endpoint
///   did not lower.
///
/// Some of what lowering decides cannot be known here, so the set is not
/// exactly what a build writes. It still counts a struct or placement
/// past the volume budget (`W_STRUCTURE_TOO_LARGE`) and a walkway past the
/// router's area cap (`W_WALKWAY_BLOCKED`), none of which lowering writes,
/// which is why the finding says the two *would* share a file. In the
/// other direction, a pair's walkway is named after its first row, and
/// when lowering refuses that row (its `path=` material does not
/// resolve, say) and lays a later one written the other way round, the
/// file it writes has a name this check did not count. `cairn compile`
/// compares the names lowering produced again before it writes any.
///
/// Each name is [`artifact_stem`] folded to lower case, so `Hut` and
/// `hut` collide on every host rather than only on the case-insensitive
/// file systems where they are one file. An artifact whose scope key was
/// already seen is one scope declared twice, which `E_DUPLICATE_ITEM` or
/// `E_DUPLICATE_PLACE_ID` already reports, so it is skipped here whichever
/// artifact holds its name.
///
/// The artifacts are compared in the order the list above gives them:
/// structs, then places, then walkways, each kind in declaration order.
/// The finding anchors on the artifact that comes second in that order,
/// with a note on the one that came first. That is not always the later
/// one in the source: a `place` in a site written above a `struct` of the
/// same name is still the one the finding anchors on.
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

/// One artifact [`check_output_names`] compares: its scope key, where it
/// is declared, and how the finding names it.
struct OutputArtifact {
    key: String,
    span: Span,
    label: String,
}

/// The artifacts [`check_output_names`] compares, in the order it
/// compares them; its doc says which are counted and why.
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
                    // The same lookup lowering makes before it reports
                    // `W_DEF_NO_SIZE` and skips the placement.
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
            // Lowering builds the key from the same row with the same call
            // and, when it is refused, reports `W_INVALID_WALKWAY_IDENT` and
            // lays nothing (`block_array::lower`, `lower_connects`). A row
            // with no key writes no file, so it has nothing to collide
            // with. If the two calls ever stop agreeing, this arm is where
            // walkways drop out of the check.
            return None;
        };
        // Recorded only for a row that gets this far, as lowering records
        // a pair only once it lays it.
        seen_pairs.insert(c.walkway_pair()).then(|| OutputArtifact {
            key: key.as_str().to_owned(),
            span: c.span.clone(),
            label: format!("the walkway `{} ↔ {}` in site `{}`", c.from, c.to, c.site),
        })
    });

    structs.chain(places).chain(walkways).collect()
}
