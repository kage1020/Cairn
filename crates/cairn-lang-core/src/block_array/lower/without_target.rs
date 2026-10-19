//! Lowering with no target pinned, for a check that reports only the findings every target that
//! reads their line raises (`spec/lint` "Error vs warning").

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, sort_by_position};
use crate::edition::Edition;
use crate::error::Span;
use crate::intent::IntentModule;
use crate::resolve::{Resolution, resolve};

use crate::block_array::PALETTE_CAPACITY;
use crate::block_array::material::NoTarget;

use super::lower_to_block_array_within;

/// The findings every target that reads their line raises on `intent`, under `edition`'s theme
/// variants or each edition's when it is `None` (`spec/lint` "Error vs warning"); `declared`
/// answers whether some version of an edition declares a block id.
#[must_use]
pub fn findings_without_target(
    intent: &IntentModule,
    edition: Option<Edition>,
    declared: &dyn Fn(Edition, &str) -> bool,
) -> Vec<Diagnostic> {
    let editions = edition.map_or_else(|| vec![Edition::Java, Edition::Bedrock], |e| vec![e]);
    let mut shared: Vec<(Vec<Edition>, Resolution)> = Vec::new();
    for edition in editions {
        let resolution = resolve(intent, Some(edition));
        match shared
            .iter_mut()
            .find(|(_, seen)| lowers_alike(seen, &resolution))
        {
            Some((editions, _)) => editions.push(edition),
            None => shared.push((vec![edition], resolution)),
        }
    }
    let lowerings: Vec<(Resolution, Vec<Diagnostic>)> = shared
        .into_iter()
        .map(|(editions, resolution)| {
            let in_each = |id: &str| editions.iter().all(|edition| declared(*edition, id));
            let registry = NoTarget { declared: &in_each };
            let found =
                lower_to_block_array_within(intent, &resolution, Some(&registry), PALETTE_CAPACITY)
                    .diagnostics;
            (resolution, found)
        })
        .collect();
    agreed(intent, lowerings)
}

/// Whether lowering reads the same input from both: it reads only the scopes and the connects.
fn lowers_alike(a: &Resolution, b: &Resolution) -> bool {
    a.scopes == b.scopes && a.connects == b.connects
}

/// The findings of `lowerings` that every target reading their line raises: one in a theme only
/// its own lowering binds, or one each lowering raises with the same code at the same span.
fn agreed(
    intent: &IntentModule,
    mut lowerings: Vec<(Resolution, Vec<Diagnostic>)>,
) -> Vec<Diagnostic> {
    if lowerings.len() == 1 {
        return lowerings.pop().map(|(_, found)| found).unwrap_or_default();
    }
    let bound: Vec<HashSet<&str>> = lowerings
        .iter()
        .map(|(resolution, _)| bound_themes(resolution))
        .collect();
    let mut seen: HashSet<&Diagnostic> = HashSet::new();
    let mut kept: Vec<Diagnostic> = Vec::new();
    for (i, (_, found)) in lowerings.iter().enumerate() {
        let others = || lowerings.iter().enumerate().filter(move |(j, _)| *j != i);
        let own: Vec<&Span> = intent
            .themes
            .iter()
            .filter(|theme| {
                bound[i].contains(theme.name.as_str())
                    && others().all(|(j, _)| !bound[j].contains(theme.name.as_str()))
            })
            .map(|theme| &theme.span)
            .collect();
        for finding in found {
            let read_alone = own
                .iter()
                .any(|theme| theme.start <= finding.span.start && finding.span.end <= theme.end);
            let raised_by_each = others().all(|(_, (_, other))| {
                other
                    .iter()
                    .any(|o| o.code == finding.code && o.span == finding.span)
            });
            if (read_alone || raised_by_each) && seen.insert(finding) {
                kept.push(finding.clone());
            }
        }
    }
    sort_by_position(&mut kept);
    kept
}

/// The themes `resolution` binds to some scope, each by its own name.
fn bound_themes(resolution: &Resolution) -> HashSet<&str> {
    resolution
        .scopes
        .values()
        .filter_map(|scope| scope.bound_theme.as_deref())
        .collect()
}
