//! A source corpus chosen to light up as much of the diagnostic surface
//! as one pass can.
//!
//! Only a code that actually fires gets checked, so the properties every
//! finding has to satisfy — readable prose, a severity matching the
//! ledger — are only as good as the set of codes these sources reach.
//! Shared by `diagnostic_text.rs` and `diagnostic_severity.rs` rather
//! than duplicated, so a fixture added for one property covers the other.

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

pub fn noisy_sources() -> Vec<String> {
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
        // the two scopes, and the two reasons a `level` loses its body.
        // Singular and plural both appear — the verb agrees with the
        // count, and only a rendered string shows that.
        format!(
            "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n    door id=d side=front at=center\n"
        ),
        format!(
            "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n    door id=d side=front at=center\n    window id=w side=front y=1 offset=1 size=1x1\n"
        ),
        format!(
            "{THEME}{HUT}site s:\n  place id=a use=hut theme=t at=origin\n    place id=b use=hut theme=t east_of=a gap=4\n"
        ),
        format!(
            "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  level y=0\n    level y=1\n      walls mat_slot=wall height=3\n"
        ),
        format!(
            "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  level\n    walls mat_slot=wall height=3\n"
        ),
        // Misplaced members: both bodies, the `level`-in-a-site advice
        // branch, and the note that counts what an indented body loses
        // with its root.
        format!("{THEME}{HUT}struct s size=5x5\n  place id=a use=hut theme=t at=origin\n"),
        format!(
            "{THEME}{HUT}site s:\n  place id=a use=hut theme=t at=origin\n  floor mat_slot=floor\n"
        ),
        format!("{THEME}{HUT}site s:\n  level y=0\n    place id=a use=hut theme=t at=origin\n"),
        format!(
            "{THEME}{HUT}site s:\n  place id=a use=hut theme=t at=origin\n  walls mat_slot=wall height=3\n    floor mat_slot=floor\n    door id=d side=front at=center\n"
        ),
        // Positional values, singular and plural.
        format!("{THEME}struct s size=5x5\n  roof flat mat_slot=wall\n"),
        format!("{THEME}struct s size=5x5\n  window front G 2 2 2x2 mat_slot=wall\n"),
        // A slot bound to something that is not a material token, and a
        // selector that matches nothing.
        format!("theme t:\n  slot floor -> 42\n\nstruct s size=3x3\n  floor mat_slot=floor\n"),
        "theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\
         \x20\x20walls[class=nosuchclass] -> mat_slot=floor\n\n\
         struct s size=3x3\n\
         \x20\x20floor mat_slot=floor\n"
            .to_owned(),
    ]
}

pub fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).expect("fixture parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let mut all = check(&module, &ir, None);
    all.extend(lower_to_block_array(&ir, &resolution, None).diagnostics);
    all
}
