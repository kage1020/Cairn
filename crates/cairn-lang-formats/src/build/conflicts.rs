//! The ids a build paints that a carried version lacks, and the `@requires` floors that admit only
//! such versions (`spec/versioning-editions` "A floor is weighed against the registry").

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::DiagnosticData;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{FloorVerdict, VersionFloor, VersionOrder};
use cairn_lang_core::{Diagnostic, Resolution, Span};

use crate::registry::RegistryPack;

/// An id a build paints, at the first place it paints it.
#[derive(Debug, Clone)]
pub(crate) struct Painted {
    pub(crate) id: String,
    pub(crate) span: Span,
}

/// A painted id with the carried version that bounds it.
#[derive(Debug, Clone)]
pub(crate) struct Bound {
    pub(crate) painted: Painted,
    pub(crate) version: String,
    pub(crate) key: i64,
}

/// A floor no version declaring every painted id clears, with the ids last declared below it.
#[derive(Debug, Clone)]
pub struct Conflict {
    pub(crate) floor: VersionFloor,
    /// The earliest `version` first, then in source order; never empty.
    pub(crate) ids: Vec<Bound>,
}

/// One id a pinned lowering found its version lacks.
pub(crate) struct Lacked {
    pub(crate) painted: Painted,
    pub(crate) since: Option<String>,
    pub(crate) until: Option<String>,
}

/// The floors among `floors` above the last version declaring an id the newest of `considered`
/// lacks.
#[must_use]
pub fn requires_conflicts(
    ir: &IntentModule,
    resolution: &Resolution,
    pack: &RegistryPack,
    considered: &[String],
    floors: &[VersionFloor],
    order: &VersionOrder,
) -> Vec<Conflict> {
    let Some(newest) = considered.last() else {
        return Vec::new();
    };
    let (last_declared, _) = bounds_at(ir, resolution, pack, newest, order, |l| l.until.clone());
    conflicts(&last_declared, floors, order)
}

/// The floors among `floors` that admit only versions after one of `last_declared` ends.
pub(crate) fn conflicts(
    last_declared: &[Bound],
    floors: &[VersionFloor],
    order: &VersionOrder,
) -> Vec<Conflict> {
    floors
        .iter()
        .filter_map(|floor| {
            // An unplaceable floor is `E_REQUIRES_UNORDERABLE`'s, and its verdict is never `Below`.
            let mut ids: Vec<Bound> = last_declared
                .iter()
                .filter(|b| order.verdict(&floor.version, b.key) == FloorVerdict::Below)
                .cloned()
                .collect();
            ids.sort_by_key(|b| (b.key, b.painted.span.start));
            (!ids.is_empty()).then(|| Conflict {
                floor: floor.clone(),
                ids,
            })
        })
        .collect()
}

/// The ids a lowering pinned at `version` refuses as undeclared: those `side` finds another
/// version declaring, each bounded there and once, then those no version declares.
pub(crate) fn bounds_at(
    ir: &IntentModule,
    resolution: &Resolution,
    pack: &RegistryPack,
    version: &str,
    order: &VersionOrder,
    side: impl Fn(&Lacked) -> Option<String>,
) -> (Vec<Bound>, Vec<Painted>) {
    let pinned = lower_to_block_array(ir, resolution, Some(&pack.view(Some(version))));
    let mut bounds = Vec::new();
    let mut undeclared = Vec::new();
    for lacked in pinned.diagnostics.iter().filter_map(lacked) {
        match side(&lacked) {
            Some(declaring) => bounds.push(bound(order, lacked.painted, declaring)),
            None => undeclared.push(lacked.painted),
        }
    }
    (first_of_each(bounds, |b| &b.painted), undeclared)
}

/// The id `d` refuses as undeclared at its pin, with the versions nearest it that declare it.
fn lacked(d: &Diagnostic) -> Option<Lacked> {
    let (id, since, until) = match d.data.as_ref()? {
        DiagnosticData::UnknownId {
            id, since, until, ..
        } => (id, since.clone(), until.clone()),
        DiagnosticData::ParityUnsupported { id, .. } => (id, None, None),
        _ => return None,
    };
    Some(Lacked {
        painted: Painted {
            id: id.clone(),
            span: d.span.clone(),
        },
        since,
        until,
    })
}

/// `painted`, bounded at `version`.
///
/// # Panics
///
/// If `order` does not carry `version`.
pub(crate) fn bound(order: &VersionOrder, painted: Painted, version: String) -> Bound {
    let key = order
        .key_of(&version)
        .expect("a declaring version is one the pack carries");
    Bound {
        painted,
        version,
        key,
    }
}

/// Each id once, at the first place the build paints it, in source order.
pub(crate) fn first_of_each<T>(mut items: Vec<T>, painted: impl Fn(&T) -> &Painted) -> Vec<T> {
    items.sort_by_key(|item| painted(item).span.start);
    let mut seen: Vec<String> = Vec::new();
    items.retain(|item| {
        let id = &painted(item).id;
        if seen.contains(id) {
            return false;
        }
        seen.push(id.clone());
        true
    });
    items
}
