//! Where a placement's or a walkway's array sits in its site.

use cairn_lang_core::SiteName;
use cairn_lang_core::block_array::BlockArrayIr;

/// The site `scope`'s array belongs to and the origin it sits at there, or `None` for a struct's.
pub(crate) fn site_origin<'a>(
    ir: &'a BlockArrayIr,
    scope: &str,
) -> Option<(&'a SiteName, (i32, i32, i32))> {
    ir.placements
        .get(scope)
        .map(|placement| (&placement.site, placement.origin))
        .or_else(|| {
            ir.walkways
                .get(scope)
                .map(|walkway| (&walkway.site, walkway.origin))
        })
}
