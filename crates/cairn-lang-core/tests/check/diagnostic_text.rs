//! Diagnostic prose reads as prose: no primary or note the corpus below reaches
//! carries a run of spaces, a raw line break or tab, or padding. A multi-line
//! literal that loses its trailing `\` still compiles and says the right thing,
//! so the rendered text is the only place it shows, and the corpus's reach is
//! asserted so the checks cannot quietly shrink.

use std::collections::BTreeSet;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::{Diagnostic, check};
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{lower, parse};

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

/// The codes are a floor and not the whole story: one code can render
/// several different sentences, and a fixture that stops reaching one of
/// them takes its prose out of the checks above without changing the code
/// set at all.
///
/// `W_IGNORED_ARGUMENT` is the code with the most: a primary per shape of
/// finding, and for an unreadable value a note per member and per outcome.
/// The two-repair-site branch is the one this pins: it is the only finding
/// in the set that names a second argument, so its sentence is the one a
/// subset check over codes cannot see go missing. The notes an unreadable
/// value can carry are pinned below.
#[test]
fn the_corpus_reaches_the_two_repair_site_branch_of_the_ignored_argument_prose() {
    let reached = rendered_strings().into_iter().any(|(origin, text)| {
        origin.starts_with("W_IGNORED_ARGUMENT")
            && text.contains("either argument may be the repair")
    });
    assert!(
        reached,
        "no fixture reaches the branch of `W_IGNORED_ARGUMENT` that names both repair sites",
    );
}

/// An unreadable value's note says what the default did, that the member
/// is not built either way, or that it was refused instead of given the
/// default, in words written per member. Each is its own literal, and so
/// its own chance at a dropped continuation; the two `i32` range refusals
/// beside them are assembled from a piece per end of the body, one of them
/// continued.
#[test]
fn the_corpus_reaches_every_note_an_unreadable_value_carries() {
    let rendered = rendered_strings();
    for (code, sentence) in [
        (
            "W_IGNORED_ARGUMENT",
            "the row is placed as `gap=0` places it",
        ),
        ("W_IGNORED_ARGUMENT", "this row is not placed either way"),
        (
            "W_IGNORED_ARGUMENT",
            "this row is not placed at `gap=0`, the value its origin was worked out with",
        ),
        ("W_IGNORED_ARGUMENT", "the stair is built with the default"),
        ("W_IGNORED_ARGUMENT", "this stair is not built either way"),
        (
            "W_IGNORED_ARGUMENT",
            "the window is drawn without its mirror",
        ),
        ("W_IGNORED_ARGUMENT", "this window is not cut either way"),
        (
            "W_IGNORED_ARGUMENT",
            "this window has `repeat=` greater than 1, which `sym=true` does not yet support, \
             so it is not cut on the `sym=false` default while `sym=` is unreadable",
        ),
        ("W_DEFERRED_MEMBER", "this placement's origin works out to"),
        ("W_DEFERRED_MEMBER", "this placement's body reaches"),
        (
            "W_DEFERRED_MEMBER",
            "; shrink the body with its `def`'s `size=` or a roof's `overhang=`, or shorten the \
             `gap=` on this row or on a row it is placed relative to",
        ),
    ] {
        assert!(
            rendered
                .iter()
                .any(|(origin, text)| origin.starts_with(code) && text.contains(sentence)),
            "no fixture reaches the `{code}` text `{sentence}`",
        );
    }
}

/// A `struct` / `def` header finding is worded apart from a member's:
/// `E_UNKNOWN_ARGUMENT` names the header rather than a keyword's arguments,
/// and the header's unreached `class=` has a primary and a note of its own.
/// Both codes are reached by member findings too, so the code set above
/// would stay whole if the header fixtures stopped reaching these.
#[test]
fn the_corpus_reaches_the_header_vocabulary_prose() {
    let rendered = rendered_strings();
    for (origin, sentence) in [
        (
            "E_UNKNOWN_ARGUMENT primary",
            "`siz=` is not an argument a `struct` header reads",
        ),
        (
            "W_IGNORED_ARGUMENT primary",
            "`class=` is an argument a `def` header takes and no pass reads yet",
        ),
        (
            "W_IGNORED_ARGUMENT note 0",
            "the `def` is built without it — remove the argument",
        ),
    ] {
        assert!(
            rendered
                .iter()
                .any(|(o, text)| o == origin && text.contains(sentence)),
            "no fixture reaches the {origin} `{sentence}`",
        );
    }
}

/// The three assertions above are only as good as the corpus: a code no
/// fixture reaches has its prose unchecked. The reach is asserted rather
/// than assumed, and named, so a fixture deleted for another reason shows
/// up as a shrunken list here instead of as silent coverage loss.
///
/// This is a floor, not the full code set: the block-array codes that need
/// a registry pack, and the resolver codes that need a multi-theme file,
/// are not all represented. Severity is not among the properties guarded
/// here: `Diagnostic::severity` reads the ledger, so there is no
/// per-diagnostic value left for a fixture to catch.
#[test]
fn the_corpus_reaches_the_codes_its_prose_assertions_are_written_for() {
    let mut seen: BTreeSet<&'static str> = BTreeSet::new();
    for source in noisy_sources() {
        for d in diagnostics_for(&source) {
            seen.insert(d.code.as_str());
        }
    }
    let expected: BTreeSet<&'static str> = [
        "E_DUPLICATE_HEADER",
        "E_DUPLICATE_ID",
        "E_DUPLICATE_ITEM",
        "E_DUPLICATE_SELECTOR",
        "E_DUPLICATE_SIZE",
        "E_INCOMPLETE_PLACE",
        "E_INVALID_PLACE_ID",
        "E_MISPLACED_MEMBER",
        "E_THEME_SELECTOR_UNMATCHED",
        "E_TRUTH_TABLE_CONFLICT",
        "E_TRUTH_TABLE_DUPLICATE_INPUT",
        "E_TRUTH_TABLE_EMPTY",
        "E_UNEXPECTED_POSITIONAL",
        "E_UNKNOWN_ARGUMENT",
        "E_UNKNOWN_KEYWORD",
        "E_UNKNOWN_SLOT_TARGET",
        "E_UNRESOLVED_PLACE_REF",
        "E_UNRESOLVED_SLOT",
        "E_UNSUPPORTED_NESTING",
        "W_DEFERRED_MEMBER",
        "W_IGNORED_ARGUMENT",
        "W_STRUCTURE_TOO_LARGE",
        "W_STRUCT_NO_SIZE",
        "W_TRUTH_TABLE_DUPLICATE_ROW",
        "W_TRUTH_TABLE_PARTIAL",
        "W_UNUSED_DEF",
    ]
    .into_iter()
    .collect();
    let missing: Vec<&str> = expected.difference(&seen).copied().collect();
    assert!(
        missing.is_empty(),
        "the corpus stopped reaching {missing:?}; it reached {seen:?}",
    );
}

const THEME: &str = "theme t:\n\
\x20\x20slot floor -> @oak_planks\n\
\x20\x20slot wall  -> @cobblestone\n\
\x20\x20slot path  -> @gravel\n\n";

const HUT: &str = "def hut size=3x3:\n\
\x20\x20floor id=floor mat_slot=floor\n\
\x20\x20walls id=walls mat_slot=wall height=3\n\
\x20\x20door  id=entry side=front at=center\n\n";

/// A source corpus chosen to light up as much of the diagnostic surface as
/// one pass can. Only a code that actually fires gets its prose checked, so
/// the properties every finding has to satisfy are only as good as the set
/// of codes these sources reach; the tests above assert that reach.
fn noisy_sources() -> Vec<String> {
    let mut sources = vec![
        // Extent, saturating reads, and roof deferral.
        format!("{THEME}struct huge size=100000x100000\n  walls mat_slot=wall height=3\n"),
        format!("{THEME}struct t size=3x3\n  walls mat_slot=wall height=5000000000\n"),
        format!(
            "{THEME}struct t size=9x7\n  walls mat_slot=wall height=3\n\
             \x20\x20roof kind=flat mat_slot=wall overhang=4294967296\n"
        ),
        // A key the roof's `kind=` routed past: in the vocabulary, on a
        // member that builds, and read by nobody on this line. The
        // ignored-argument prose has a branch per shape, and this one names
        // two arguments where the others name one. `flat` rather than
        // `gable` because a sloped roof needs a stair-family material, and
        // `wall` binds cobblestone — the `E_INCOMPATIBLE_MATERIAL` that
        // earns would be a second finding this fixture is not about.
        format!(
            "{THEME}struct t size=9x7\n  walls mat_slot=wall height=3\n\
             \x20\x20roof kind=flat mat_slot=wall slope_to=front\n"
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
        // Two selector rows that coincide, over a member both of them
        // select — so the prose under test is about the pair of rows and
        // not about a rule that reached nothing. Written out rather than
        // built from `THEME` because the rows belong inside it.
        "theme t:\n  slot wall -> @cobblestone\n  \
         window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n\n\
         struct s size=5x5\n  \
         window class=small side=front offset=1 y=1 size=1x1 mat_slot=wall\n"
            .to_owned(),
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
        // A misplaced row that carries a signal: the message softens its
        // claim rather than telling the author it reaches nothing, and
        // that softening is its own sentence to check.
        format!(
            "{THEME}{HUT}site s:
  place id=a use=hut theme=t at=origin
  pressure_plate id=gate at=front.outside -> sig.step
"
        ),
        // A `place` row short of one key, and one short of all three:
        // the message lists them in a sentence, so singular and plural
        // both have to read.
        format!("{THEME}{HUT}site s:\n  place id=a theme=t at=origin\n"),
        format!("{THEME}{HUT}site s:\n  place at=origin\n"),
        // The two-key arity of the missing-key list, which joins without a
        // serial comma, and an id-less row whose origin selector also
        // fails — the branch that has no name to quote.
        format!(
            "{THEME}{HUT}site s:
  place id=b at=origin
"
        ),
        format!(
            "{THEME}{HUT}site s:
  place use=hut theme=t at=middle
"
        ),
        // Positional values, singular and plural.
        format!("{THEME}struct s size=5x5\n  roof flat mat_slot=wall\n"),
        format!("{THEME}struct s size=5x5\n  window front G 2 2 2x2 mat_slot=wall\n"),
        // A slot bound to something that is not a material token, and a
        // selector that matches nothing.
        "theme t:\n  slot floor -> 42\n\nstruct s size=3x3\n  floor mat_slot=floor\n".to_owned(),
        "theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\
         \x20\x20walls[class=nosuchclass] -> mat_slot=floor\n\n\
         struct s size=3x3\n\
         \x20\x20floor mat_slot=floor\n"
            .to_owned(),
    ];
    sources.extend(unreadable_value_sources());
    sources.extend(header_sources());
    sources.extend(truth_table_sources());
    sources
}

/// The sources for the truth-table findings, kept apart like
/// `unreadable_value_sources` only so no list outgrows a screen.
fn truth_table_sources() -> Vec<String> {
    vec![
        // Truth tables that verify less than they look like they do: no
        // rows, a pattern assigned twice each way, and combinations left
        // out. One source per shape — the four codes word their repair
        // differently and a single table would leave three unrendered.
        format!("{THEME}struct s size=3x3\n  assert truth(sig.a, sig.b -> sig.o) {{ }}\n"),
        format!(
            "{THEME}struct s size=3x3\n  assert truth(sig.a, sig.b -> sig.o) {{ 00->0; 00->1; 01->0; 10->0; 11->0 }}\n"
        ),
        format!(
            "{THEME}struct s size=3x3\n  assert truth(sig.a, sig.b -> sig.o) {{ 00->0; 00->0; 01->0; 10->0; 11->0 }}\n"
        ),
        format!("{THEME}struct s size=3x3\n  assert truth(sig.a, sig.b -> sig.o) {{ 00->0 }}\n"),
        // And a header that lists a signal more than once, which is refused
        // alone. Two signals, so both shapes of the position list render:
        // `sig.a` in full, and `sig.b` past the cap with a count of the rest.
        format!(
            "{THEME}struct s size=3x3\n  assert truth(sig.a, sig.a, sig.b, sig.b, sig.b, sig.b, sig.b -> sig.o) {{ }}\n"
        ),
    ]
}

/// The sources for the `struct` / `def` header line's own vocabulary, kept
/// apart like `unreadable_value_sources` only so no list outgrows a screen:
/// a misspelled `size=`, which also leaves the struct with no size, and the
/// `class=` the header takes and no pass reads. Each finding has a header
/// sentence of its own, distinct from the member one.
fn header_sources() -> Vec<String> {
    vec![
        format!("{THEME}struct s siz=7x7\n  floor mat_slot=floor\n"),
        format!("{THEME}def cottage class=house size=5x5:\n  floor mat_slot=floor\n"),
    ]
}

/// The sources for an unreadable value's findings, kept apart only so
/// neither list outgrows a screen.
fn unreadable_value_sources() -> Vec<String> {
    vec![
        // Unreadable values, on a member that is built and on one that is
        // not: the note is worded per member and per outcome, and only a
        // rendered string shows each. `b`'s `gap=` is ignored and the row
        // placed; `c`'s origin leaves the `i32` range, a message joined from
        // continued lines; `d` is anchored on `c`, so it is not placed
        // and its unreadable `gap=` carries a second note. `e`'s far edge
        // sits exactly on `i32::MAX`, so `f` leaves the range at the `gap=0`
        // its unreadable `gap=` falls back to, and carries the third. `g`'s
        // origin is in range and its far edge is not.
        format!(
            "{THEME}{HUT}site s:\n  place id=a use=hut theme=t at=origin\n\
             \x20\x20place id=b use=hut theme=t east_of=a gap=wide\n\
             \x20\x20place id=c use=hut theme=t east_of=a gap=3000000000\n\
             \x20\x20place id=d use=hut theme=t east_of=c gap=wide\n\
             \x20\x20place id=e use=hut theme=t east_of=a gap=2147483642\n\
             \x20\x20place id=f use=hut theme=t east_of=e gap=wide\n\
             \x20\x20place id=g use=hut theme=t east_of=a gap=2147483644\n"
        ),
        // Its own theme, because an eave needs a stair material and
        // `THEME` binds none.
        "theme t:\n  slot wall -> @cobblestone\n  slot eave -> @oak_stairs\n\n\
         struct t size=7x5\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         stair kind=stairs side=front facing=1 mat_slot=eave\n  \
         stair kind=stairs side=back half=sideways shape=\"straight\" mat_slot=eave\n  \
         window side=front offset=1 y=1 size=1x1 sym=yes mat_slot=wall\n  \
         window side=back offset=9 y=1 size=1x1 sym=yes mat_slot=wall\n  \
         window side=left offset=0 y=1 size=1x1 repeat=2 step=2 sym=yes mat_slot=wall\n"
            .to_owned(),
    ]
}

/// Every finding `check` and the block-array pass raise over `source`.
fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).expect("fixture parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let mut all = check(&module, &ir, None);
    all.extend(lower_to_block_array(&ir, &resolution, None).diagnostics);
    all
}
