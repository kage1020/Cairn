//! The canonical tokens an edition's versions write under another id, per
//! `spec/versioning-editions` "The `respelled` row".

use cairn_lang_core::block_array::{BlockState, TargetRegistry, bound_canonical_tokens};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{
    RespelledToken, SpellingTable, VersionFloor, WrittenSpelling, versions_satisfying,
};
use cairn_lang_core::{Edition, Resolution};

use super::Reporter;
use crate::registry::{PackView, RegistryPack};

/// The tokens `resolution` binds that a version of `considered` the floors admit writes under
/// another id.
pub(super) fn respelled_tokens(
    resolution: &Resolution,
    pack: &RegistryPack,
    floors: &[VersionFloor],
    considered: &[String],
    source: &str,
    lines: &LineStarts,
) -> Vec<RespelledToken> {
    let views: Vec<(&str, PackView<'_>)> =
        versions_satisfying(&pack.version_order(), floors, considered)
            .into_iter()
            .map(|version| (version, pack.view(Some(version))))
            .collect();
    bound_canonical_tokens(resolution)
        .into_iter()
        .filter_map(|(span, token)| {
            let written: Vec<WrittenSpelling> = views
                .iter()
                .filter_map(|(version, view)| {
                    let (id, through) = written_at(view, &token)?;
                    Some(WrittenSpelling {
                        version: (*version).to_owned(),
                        id,
                        through,
                    })
                })
                .collect();
            (!written.is_empty()).then(|| RespelledToken {
                id: token.id,
                position: lines.position(source, span.start),
                written,
            })
        })
        .collect()
}

/// The id `view` writes for `token` when it is not the token's own, and the table that maps it;
/// `None` also when the version cannot write the token at all, which `E_UNKNOWN_ID` answers.
fn written_at(view: &PackView<'_>, token: &BlockState) -> Option<(String, SpellingTable)> {
    let meant = view.meant(token);
    let state = meant.as_ref().unwrap_or(token);
    let declared = view.block_ids().is_some_and(|ids| ids.contains(&state.id));
    let (id, through) = match (declared, &meant) {
        (true, Some(_)) => (state.id.clone(), SpellingTable::Meanings),
        (true, None) => return None,
        (false, _) => {
            let respelling = view.respelling(state)?;
            (respelling.written, respelling.through)
        }
    };
    (id != token.id).then_some((id, through))
}

/// The `note:` lines saying how each of `tokens` is written: one per id written and table.
pub(super) fn report_respelled_notes(
    out: &mut Reporter<'_>,
    edition: Edition,
    tokens: &[RespelledToken],
) {
    for token in tokens {
        let mut spellings: Vec<(&str, SpellingTable, Vec<&str>)> = Vec::new();
        for written in &token.written {
            match spellings
                .iter_mut()
                .find(|(id, through, _)| *id == written.id && *through == written.through)
            {
                Some((_, _, versions)) => versions.push(&written.version),
                None => spellings.push((&written.id, written.through, vec![&written.version])),
            }
        }
        for (id, through, versions) in spellings {
            let line = format!(
                "note: {}:{}: `{}` is written `{id}` at {} {}, per the registry pack's {}",
                out.file.display(),
                token.position,
                token.id,
                edition.as_str(),
                versions.join(", "),
                table_name(through),
            );
            out.line(&line);
        }
    }
}

fn table_name(table: SpellingTable) -> &'static str {
    match table {
        SpellingTable::Aliases => "alias table",
        SpellingTable::StateMap => "state map",
        SpellingTable::Meanings => "meaning table",
    }
}
