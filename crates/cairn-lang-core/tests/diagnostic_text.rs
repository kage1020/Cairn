//! Diagnostic prose has to read as prose.
//!
//! Multi-line string literals in this crate are joined with a trailing `\`,
//! which swallows the newline and the next line's indentation. Drop the
//! backslash and the literal keeps both — the message still compiles, still
//! says the right thing, and renders with a twenty-space gap in the middle
//! of a sentence. `rustfmt` does not touch string contents and `clippy` has
//! no lint for it, so CI stayed green through three of these at once.
//!
//! Checking the rendered text is the only place the mistake is visible.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::{Diagnostic, check};
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{lower, parse};

const THEME: &str = "theme t:\n\
\x20\x20slot floor -> @oak_planks\n\
\x20\x20slot wall  -> @cobblestone\n\
\x20\x20slot path  -> @gravel\n\n";

const HUT: &str = "def hut size=3x3:\n\
\x20\x20floor id=floor mat_slot=floor\n\
\x20\x20walls id=walls mat_slot=wall height=3\n\
\x20\x20door  id=entry side=front at=center\n\n";

/// Sources chosen to light up as much of the diagnostic surface as one
/// pass can, since only a code that actually fires gets its text checked.
fn noisy_sources() -> Vec<String> {
    vec![
        // Extent, saturating reads, and roof deferral.
        format!("{THEME}struct huge size=100000x100000\n  walls mat_slot=wall height=3\n"),
        format!("{THEME}struct t size=3x3\n  walls mat_slot=wall height=5000000000\n"),
        format!(
            "{THEME}struct t size=9x7\n  walls mat_slot=wall height=3\n\
             \x20\x20roof kind=flat mat_slot=wall overhang=4294967296\n"
        ),
        // Place ids, unresolved refs, unused defs.
        format!("{THEME}{HUT}site s:\n  place id=\"home.1\" use=hut theme=t at=origin\n"),
        format!("{THEME}{HUT}site s:\n  place id=a use=nosuchdef theme=t at=origin\n"),
        format!("{THEME}{HUT}"),
        // Walkway refusals.
        format!(
            "{THEME}{HUT}site duo:\n  place id=a use=hut theme=t at=origin\n\
             \x20\x20place id=b use=hut theme=t east_of=a gap=30000\n\
             \x20\x20place id=c use=hut theme=t north_of=b gap=30000\n\
             \x20\x20connect a.entry to c.entry path=@path\n"
        ),
        // Syntactic codes.
        format!("{THEME}struct s size=5x5 size=6x6\n  floor mat_slot=floor\n"),
        format!(
            "{THEME}struct s size=5x5\n  floor id=x mat_slot=floor\n  walls id=x mat_slot=wall height=3\n"
        ),
        format!("{THEME}struct s size=5x5\n  torch mat_slot=wall\n"),
        format!("{THEME}struct s size=5x5\n  floor mat_slot=nosuchslot\n"),
        // Duplicate names and headers. Each kind gets its own source
        // because the repair note is written per kind, so one fixture
        // would leave the other branch's prose unchecked.
        format!("{THEME}{HUT}def hut size=5x5:\n  floor id=floor mat_slot=floor\n"),
        format!("{THEME}struct s size=5x5\n  floor\n\nstruct s size=6x6\n  floor\n"),
        format!(
            "{THEME}{HUT}site s:\n  place id=a use=hut theme=t at=origin\n\nsite s:\n  place id=b use=hut theme=t at=origin\n"
        ),
        format!("@cairn 2026.06\n@cairn 2026.07\n\n{THEME}struct s size=5x5\n  floor\n"),
        // Unsupported nesting, one source per branch of the message:
        // the two scopes, and the three reasons a `level` loses its
        // body. Singular and plural both appear — the verb agrees with
        // the count, and only a rendered string shows that.
        format!(
            "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n    door id=d side=front at=center\n"
        ),
        format!(
            "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n    door id=d side=front at=center\n    window id=w side=front y=1 offset=1 size=1x1\n"
        ),
        format!(
            "{THEME}{HUT}site s:\n  place id=a use=hut theme=t at=origin\n    place id=b use=hut theme=t east_of=a gap=4\n"
        ),
        format!("{THEME}{HUT}site s:\n  level y=0\n    place id=a use=hut theme=t at=origin\n"),
        format!(
            "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  level y=0\n    level y=1\n      walls mat_slot=wall height=3\n"
        ),
        format!(
            "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  level\n    walls mat_slot=wall height=3\n"
        ),
    ]
}

fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).expect("fixture parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let mut all = check(&module, &ir, None);
    all.extend(lower_to_block_array(&ir, &resolution, None).diagnostics);
    all
}

/// Every string a diagnostic renders, tagged with where it came from.
fn rendered_strings() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for source in noisy_sources() {
        for d in diagnostics_for(&source) {
            let code = d.code.as_str();
            out.push((format!("{code} primary"), d.primary.clone()));
            for (i, note) in d.notes.iter().enumerate() {
                out.push((format!("{code} note {i}"), note.message.clone()));
            }
        }
    }
    assert!(
        out.len() > 20,
        "the fixtures should light up a broad slice of the surface, got {} strings",
        out.len(),
    );
    out
}

#[test]
fn no_diagnostic_text_carries_a_run_of_spaces() {
    for (origin, text) in rendered_strings() {
        assert!(
            !text.contains("  "),
            "{origin} renders a run of spaces, which is a dropped `\\` line \
             continuation in the literal: {text:?}",
        );
    }
}

#[test]
fn no_diagnostic_text_carries_a_raw_newline_or_tab() {
    // The renderer puts each diagnostic on its own line and indents notes,
    // so a literal newline inside the text breaks that shape — and a tab
    // lands at a different column in every consumer.
    for (origin, text) in rendered_strings() {
        assert!(
            !text.contains('\n') && !text.contains('\t'),
            "{origin} embeds its own line break: {text:?}",
        );
    }
}

#[test]
fn no_diagnostic_text_is_empty_or_padded() {
    for (origin, text) in rendered_strings() {
        assert!(!text.trim().is_empty(), "{origin} renders nothing");
        assert_eq!(text.trim(), text, "{origin} has leading or trailing space");
    }
}
