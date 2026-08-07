//! Sentence fragments shared by the diagnostic builders.
//!
//! One copy per fragment, because the arity branches are where these go
//! wrong and the middle arity is the one nobody writes a test for first: a
//! list joined with a single `join(", ")` renders two items as
//! `` `a`, and `b` ``, which is a serial comma with nothing to serialise.

/// Render `items` as an English list — `a`, `a and b`, `a, b, and c`.
///
/// `None` for an empty slice. Every caller's sentence asserts that
/// something was listed, so there is no message to build from nothing, and
/// returning the empty string would put that claim in front of the user.
pub(crate) fn and_list(items: &[String]) -> Option<String> {
    Some(match items.split_last()? {
        (last, []) => last.clone(),
        (last, [only]) => format!("{only} and {last}"),
        (last, head) => format!("{}, and {last}", head.join(", ")),
    })
}

#[cfg(test)]
mod tests {
    use super::and_list;

    fn of(items: &[&str]) -> Option<String> {
        let owned: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
        and_list(&owned)
    }

    #[test]
    fn every_arity_reads() {
        assert_eq!(of(&[]), None);
        assert_eq!(of(&["a"]).as_deref(), Some("a"));
        assert_eq!(of(&["a", "b"]).as_deref(), Some("a and b"));
        assert_eq!(of(&["a", "b", "c"]).as_deref(), Some("a, b, and c"));
        assert_eq!(of(&["a", "b", "c", "d"]).as_deref(), Some("a, b, c, and d"));
    }
}
