//! `W_IGNORED_ARGUMENT` on the keys of one form of `place` written on the other, and on the
//! stand keys of a body's `place` that its `at=` does not read (`spec/components-editing-sites`
//! "A `def` placed in a body").

use cairn_lang_core::DiagnosticCode;

use crate::common::{PRELUDE, diagnose, slice};

/// Each `W_IGNORED_ARGUMENT` of `src` as the text it points at, its primary and its one note.
fn ignored(src: &str) -> Vec<(&str, String, String)> {
    diagnose(src)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::IgnoredArgument)
        .map(|d| {
            let note = d
                .notes
                .first()
                .map(|n| n.message.clone())
                .unwrap_or_default();
            (slice(src, &d), d.primary, note)
        })
        .collect()
}

fn in_a_body(row: &str) -> String {
    format!("{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  {row}\n")
}

const SITE_KEY: &str = "is a key of a site's `place`, and nothing reads it on a `place` in a body; \
                        the value was ignored";

const STANDS_BY_AT: &str = "a def placed in a body stands where its `at=` says, by a wall as \
                            `at=inside.<side>` or at a cell as `at=X,Y,Z` — remove the argument";

#[test]
fn a_site_key_on_a_body_place_is_ignored() {
    let src = in_a_body("place use=table at=1,0,1 theme=plain east_of=a north_of=a gap=2");
    assert_eq!(
        ignored(&src),
        [
            (
                "plain",
                "`theme=` is a key of a site's `place`, and nothing reads it on a `place` in a \
                 body; the value was ignored"
                    .to_owned(),
                "the def is built with the theme of the body it stands in — remove the argument"
                    .to_owned(),
            ),
            (
                "a",
                format!("`east_of=` {SITE_KEY}"),
                STANDS_BY_AT.to_owned()
            ),
            (
                "a",
                format!("`north_of=` {SITE_KEY}"),
                STANDS_BY_AT.to_owned()
            ),
            ("2", format!("`gap=` {SITE_KEY}"), STANDS_BY_AT.to_owned()),
        ],
    );
}

#[test]
fn a_stand_key_the_at_does_not_read_is_ignored() {
    let by_the_wall = in_a_body("place use=chair at=inside.back offset=1 y=0 facing=east");
    assert_eq!(
        ignored(&by_the_wall),
        [(
            "east",
            "`facing=` is read on a `place` at a coordinate triple, and this one stands by a \
             wall; the value was ignored"
                .to_owned(),
            "a def placed by a wall faces away from it; to turn it another way, stand it at a \
             cell with `at=X,Y,Z facing=...`"
                .to_owned(),
        )],
    );
    let at_a_cell = in_a_body("place use=chair at=1,0,1 offset=1 y=2 facing=east");
    let row = "is read on a `place` by a wall, and this one stands at a coordinate triple; the \
               value was ignored";
    let note = "the triple names the cell it stands at, its row included — remove the argument";
    assert_eq!(
        ignored(&at_a_cell),
        [
            ("1", format!("`offset=` {row}"), note.to_owned()),
            ("2", format!("`y=` {row}"), note.to_owned()),
        ],
    );
}

#[test]
fn a_body_key_on_a_site_place_is_ignored() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=a use=hut theme=plain at=origin offset=1 y=2 facing=east\n"
    );
    let row = "is a key of a `place` in a body, and nothing reads it on a site's `place`; the \
               value was ignored";
    let note = "a site's `place` stands where its `at=`, `east_of=` or `north_of=` puts it, \
                facing as its def is written — remove the argument";
    assert_eq!(
        ignored(&src),
        [
            ("1", format!("`offset=` {row}"), note.to_owned()),
            ("2", format!("`y=` {row}"), note.to_owned()),
            ("east", format!("`facing=` {row}"), note.to_owned()),
        ],
    );
}
