//! Helpers shared by the place-and-route integration test binaries.
//!
//! Kept deliberately small: the per-pass `load_example` /
//! `*_from_source` fixture builders stay in the binary that uses them,
//! because each pins a different prefix of the pipeline and reads
//! better next to its assertions. What lives here is the machinery
//! whose *correctness* is shared — a helper that has to agree with the
//! JSON wire form across every stage, and would rot silently if each
//! binary kept its own copy.

/// Rewrite every `"stage": "<name>"` value to a fixed placeholder so
/// two adjacent stages' dumps can be byte-compared on everything
/// *except* the tag that distinguishes them.
///
/// The routing, delay, and crossing binaries each assert that their
/// pass perturbs nothing but the field it writes. Since every cell
/// carries a `stage` tag whose value moves from stage to stage, that
/// assertion has to neutralise the tag first — and it has to do so
/// identically in all three, or a change to the key name would be
/// caught in one binary and papered over in another.
///
/// Handles both the compact and the pretty spelling: the separator
/// between the key and the value is copied through verbatim rather
/// than assumed.
pub fn normalize_stage_tags(json: &str) -> String {
    const KEY: &str = "\"stage\":";
    let mut out = String::with_capacity(json.len());
    let mut rest = json;
    while let Some(idx) = rest.find(KEY) {
        let (before, after) = rest.split_at(idx + KEY.len());
        out.push_str(before);
        let open = after.find('"').expect("stage tag value is a string");
        let close = after[open + 1..]
            .find('"')
            .expect("stage tag value is a closed string");
        out.push_str(&after[..open]);
        out.push_str("\"<stage>\"");
        rest = &after[open + close + 2..];
    }
    out.push_str(rest);
    out
}
