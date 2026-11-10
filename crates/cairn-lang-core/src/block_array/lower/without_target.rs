//! Lowering with no target pinned, for a check that reports only the findings every target that
//! reads their line raises (`spec/lint` "Error vs warning").

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, EditionFindings, agreed};
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
    let guarded = intent.has_edition_guards();
    let mut shared: Vec<(Vec<Edition>, Resolution)> = Vec::new();
    for edition in editions {
        let resolution = resolve(intent, Some(edition));
        match shared
            .iter_mut()
            .find(|(_, seen)| lowers_alike(seen, &resolution, guarded))
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
    agreed_lowerings(intent, lowerings)
}

/// Whether lowering reads the same input from both: the scopes, the connects and, when `guarded`
/// says some member stands under an `@edition` guard, the edition.
fn lowers_alike(a: &Resolution, b: &Resolution, guarded: bool) -> bool {
    a.scopes == b.scopes && a.connects == b.connects && (!guarded || a.edition == b.edition)
}

/// The findings [`agreed`] keeps of `lowerings`, where a theme only one lowering binds is that
/// lowering's own, like the lines under its edition's guards.
fn agreed_lowerings(
    intent: &IntentModule,
    lowerings: Vec<(Resolution, Vec<Diagnostic>)>,
) -> Vec<Diagnostic> {
    let (resolutions, found): (Vec<Resolution>, Vec<Vec<Diagnostic>>) =
        lowerings.into_iter().unzip();
    let bound: Vec<HashSet<&str>> = resolutions.iter().map(bound_themes).collect();
    let only_bound_by = |i: usize, name: &str| {
        bound[i].contains(name)
            && bound
                .iter()
                .enumerate()
                .all(|(j, other)| i == j || !other.contains(name))
    };
    let editions = found
        .into_iter()
        .enumerate()
        .map(|(i, found)| EditionFindings {
            found,
            guarded: resolutions[i]
                .edition
                .map(|edition| intent.lines_guarded_for(edition))
                .unwrap_or_default(),
            themes: intent
                .themes
                .iter()
                .filter(|theme| only_bound_by(i, &theme.name))
                .map(|theme| &theme.span)
                .collect(),
        })
        .collect();
    agreed(editions)
}

/// The findings each edition's lowering raises on the lines under its `@edition` guards, which a
/// lowering that pins no edition leaves out; `declared` is [`findings_without_target`]'s.
#[must_use]
pub fn findings_under_guards(
    intent: &IntentModule,
    declared: &dyn Fn(Edition, &str) -> bool,
) -> Vec<Diagnostic> {
    let guarded: Vec<&Span> = intent
        .guarded_members()
        .into_iter()
        .map(|(_, member)| &member.span)
        .collect();
    if guarded.is_empty() {
        return Vec::new();
    }
    findings_without_target(intent, None, declared)
        .into_iter()
        .filter(|finding| finding.touches(&guarded))
        .collect()
}

/// The themes `resolution` binds to some scope, each by its own name.
fn bound_themes(resolution: &Resolution) -> HashSet<&str> {
    resolution
        .scopes
        .values()
        .filter_map(|scope| scope.bound_theme.as_deref())
        .collect()
}
