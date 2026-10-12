//! Theme names, their per-edition variants, and which of them a build binds
//! (`spec/versioning-editions` "Java / Bedrock portability"). Shared by the resolver and
//! [`super::version_axes`], so both weigh the variant the build actually binds.

use crate::edition::Edition;

/// A theme name's logical part, and the edition its `_java` or `_bedrock` suffix names.
pub(crate) fn strip_edition_suffix(name: &str) -> (&str, Option<Edition>) {
    if let Some(base) = name.strip_suffix("_java") {
        (base, Some(Edition::Java))
    } else if let Some(base) = name.strip_suffix("_bedrock") {
        (base, Some(Edition::Bedrock))
    } else {
        (name, None)
    }
}

/// The sole logical theme name among `names`, ignoring per-edition variant suffixes.
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

/// The theme name to bind for `logical` under `edition`, or `None`.
///
/// A pin falls back to the unsuffixed variant but never to the other edition's, whose slot values
/// would leak into this edition's output.
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
/// Unpinned, a declared name binds verbatim and an undeclared suffixed name binds nothing.
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
