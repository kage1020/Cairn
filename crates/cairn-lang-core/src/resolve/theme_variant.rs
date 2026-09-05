//! Theme names, their per-edition variants, and which of them a build binds.
//!
//! `spec/versioning-editions.md` §10.7 lets one logical theme be written as
//! two per-edition variants — `shop_java` and `shop_bedrock` — and lets a
//! reference name the logical theme rather than either variant. Which variant
//! a build actually binds is therefore a question about a *set of names* and
//! the pinned edition, and nothing else.
//!
//! It lives here rather than inside [`super::resolver`] because two callers
//! now ask it. The resolver asks in order to walk a body under the right slot
//! map; [`super::version_axes`] asks in order to decide whose version floors
//! a build inherits, and it works from the surface AST, where no
//! `ThemeBinding` exists yet. One copy of the rule, so a reference cannot bind
//! one variant for the build and a different one for the floor it is held to.

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

/// Return the sole *logical* theme name among `names`, ignoring per-edition
/// variant suffixes.
///
/// A file with `theme shop_java` + `theme shop_bedrock` reports
/// `Some("shop")` because both are variants of one logical theme — this
/// keeps the auto-pick rule intact when the author uses spec §10.7
/// variants. A file with `theme cottage` + `theme keep` reports `None`
/// because the two names are genuinely distinct logical themes.
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
/// Under a `Some(edition)` compile the fallback deliberately **stops at
/// the unsuffixed variant** rather than cross over to the opposite
/// edition's variant. Binding, say, a `_bedrock` theme under
/// `--edition java` would silently route Bedrock-only slot values into a
/// Java `.nbt`. Returning `None` instead is reported as
/// `E_THEME_VARIANT_MISSING` by both callers — not as `E_UNRESOLVED_SLOT`,
/// which needs a bound theme to say the slot is missing from and would
/// blame a slot that is declared and spelled correctly.
///
/// The `None` case still tolerates a partial file (only one variant
/// declared): it prefers the unsuffixed theme, then Java, then Bedrock —
/// a deterministic order that avoids leaking source-order into
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
/// The name half of the resolver's `resolve_theme_reference`, which adds to
/// it the two things only a diagnostic needs: whether `None` means "nothing
/// of that name is declared" or "no variant of it fits the pin", and how the
/// reference was spelled.
///
/// A reference is read as naming the *logical* theme, which is the spelling
/// spec versioning-editions §10.7 asks the semantic layer to use, so
/// `theme=shop` binds in a module declaring only `shop_java` and
/// `shop_bedrock`. With no pin, nothing re-picks a variant the author named:
/// a declared name binds verbatim, and a *suffixed* name nothing declares
/// binds nothing rather than being swapped for a sibling.
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
        // Names that happen to end with a similar substring but are not
        // suffixed with the closed edition set remain unsuffixed.
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
