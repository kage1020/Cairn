//! Theme names, their per-edition variants, and which of them a build binds.
//!
//! `spec/versioning-editions` "Java / Bedrock portability" lets one logical theme be written as
//! per-edition variants, `shop_java` and `shop_bedrock`, and lets a reference name the logical
//! theme, so which variant a build binds depends only on a set of names and the pinned edition.
//!
//! The rule lives outside [`super::resolver`] because two callers ask it: the resolver, to walk a
//! body under the right slot map, and [`super::version_axes`], to decide whose version floors a
//! build inherits, working from the surface AST where no `ThemeBinding` exists yet. One copy means
//! `E_VERSION_CAP` and `buildable targets` weigh the floor of the variant the build actually binds.
//!
//! `cairn info`'s edition-neutral row asks with no pin and reads a theme only when both editions
//! bind the same one; [`super::unscoped_version_floors`] owns that caveat.

use crate::edition::Edition;

/// Split a theme name into its logical part and the edition it names.
///
/// The suffix set is closed to the two editions, so `javanese` is an
/// unsuffixed name and not a Java variant of `javan`.
pub(crate) fn strip_edition_suffix(name: &str) -> (&str, Option<Edition>) {
    if let Some(base) = name.strip_suffix("_java") {
        (base, Some(Edition::Java))
    } else if let Some(base) = name.strip_suffix("_bedrock") {
        (base, Some(Edition::Bedrock))
    } else {
        (name, None)
    }
}

/// Return the sole *logical* theme name among `names`, ignoring per-edition variant suffixes.
///
/// `shop_java` and `shop_bedrock` give `Some("shop")`, so the auto-pick still applies to the
/// variants of `spec/versioning-editions` "Java / Bedrock portability"; `cottage` and `keep` give
/// `None`.
pub(crate) fn single_logical_theme<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut logical: Option<&str> = None;
    for name in names {
        let (l, _) = strip_edition_suffix(name);
        match logical {
            None => logical = Some(l),
            Some(seen) if seen == l => {}
            Some(_) => return None,
        }
    }
    logical
}

/// Pick the theme name to bind for `logical` under `edition`.
///
/// Order of preference:
///
/// - `Some(Java)`    → `<logical>_java` → unsuffixed `<logical>` → **unbound**
/// - `Some(Bedrock)` → `<logical>_bedrock` → unsuffixed `<logical>` → **unbound**
/// - `None`          → unsuffixed `<logical>` → `<logical>_java` → `<logical>_bedrock`
///
/// Under a pin the fallback stops at the unsuffixed variant rather than crossing to the other
/// edition's: binding a `_bedrock` theme under `--edition java` would route Bedrock-only slot
/// values into a Java `.nbt`. The resolver reports `None` as `E_THEME_VARIANT_MISSING`, not
/// `E_UNRESOLVED_SLOT`, which needs a bound theme and would blame a slot that is declared and
/// spelled correctly.
///
/// Unpinned, any declared variant binds, in a fixed order so source order does not leak into
/// diagnostics.
pub(crate) fn pick_variant<'a>(
    names: impl IntoIterator<Item = &'a str>,
    logical: &str,
    edition: Option<Edition>,
) -> Option<&'a str> {
    let mut unsuffixed: Option<&str> = None;
    let mut java: Option<&str> = None;
    let mut bedrock: Option<&str> = None;
    for name in names {
        let (l, variant) = strip_edition_suffix(name);
        if l != logical {
            continue;
        }
        match variant {
            None => unsuffixed = Some(name),
            Some(Edition::Java) => java = Some(name),
            Some(Edition::Bedrock) => bedrock = Some(name),
        }
    }
    match edition {
        Some(Edition::Java) => java.or(unsuffixed),
        Some(Edition::Bedrock) => bedrock.or(unsuffixed),
        None => unsuffixed.or(java).or(bedrock),
    }
}

/// Which declared theme a `theme=NAME` reference binds, or `None`.
///
/// The resolver's `resolve_theme_reference` adds what only a diagnostic needs: whether `None` means
/// nothing of that name is declared or no variant fits the pin, and how the reference was spelled.
///
/// A reference names the *logical* theme, the spelling `spec/versioning-editions` "Java / Bedrock
/// portability" asks the semantic layer to use, so `theme=shop` binds in a module declaring only
/// `shop_java` and `shop_bedrock`. With no pin, nothing re-picks a variant the author named: a
/// declared name binds verbatim, and a *suffixed* name nothing declares binds nothing rather than
/// being swapped for a sibling.
pub(crate) fn bound_theme_name<'a, I>(
    names: I,
    written: &str,
    edition: Option<Edition>,
) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
    I::IntoIter: Clone,
{
    let names = names.into_iter();
    let (logical, written_variant) = strip_edition_suffix(written);
    if !names
        .clone()
        .any(|name| strip_edition_suffix(name).0 == logical)
    {
        return None;
    }
    if edition.is_none() {
        if let Some(declared) = names.clone().find(|name| *name == written) {
            return Some(declared);
        }
        if written_variant.is_some() {
            return None;
        }
    }
    pick_variant(names, logical, edition)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_edition_suffix_recognises_both_editions() {
        assert_eq!(
            strip_edition_suffix("shop_java"),
            ("shop", Some(Edition::Java))
        );
        assert_eq!(
            strip_edition_suffix("shop_bedrock"),
            ("shop", Some(Edition::Bedrock)),
        );
        assert_eq!(strip_edition_suffix("medieval"), ("medieval", None));
        assert_eq!(strip_edition_suffix("javanese"), ("javanese", None));
    }

    #[test]
    fn one_logical_theme_across_two_variants() {
        assert_eq!(
            single_logical_theme(["shop_java", "shop_bedrock"]),
            Some("shop")
        );
        assert_eq!(single_logical_theme(["cottage", "keep"]), None);
        assert_eq!(single_logical_theme([]), None);
    }

    #[test]
    fn a_pin_stops_at_the_unsuffixed_variant() {
        let names = ["shop_bedrock", "shop"];
        assert_eq!(
            pick_variant(names, "shop", Some(Edition::Java)),
            Some("shop")
        );
        assert_eq!(
            pick_variant(names, "shop", Some(Edition::Bedrock)),
            Some("shop_bedrock")
        );
        assert_eq!(
            pick_variant(["shop_bedrock"], "shop", Some(Edition::Java)),
            None
        );
    }
}
