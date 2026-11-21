//! The canonical tokens an edition's versions write under another id, per
//! `spec/versioning-editions` "The `respelled` row".

use std::path::Path;

use cairn_lang_core::block_array::bound_canonical_tokens;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{
    RespelledToken, SpellingTable, VersionFloor, WrittenSpelling, versions_satisfying,
};
use cairn_lang_core::{Edition, Resolution};
use cairn_lang_formats::registry::{PackView, RegistryPack};

/// The tokens `resolution` binds that a version of `considered` the floors admit writes under
/// another id.
pub(crate) fn respelled_tokens(
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
                    let respelling = view.respelling(&token)?;
                    Some(WrittenSpelling {
                        version: (*version).to_owned(),
                        id: respelling.written,
                        through: respelling.through,
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

/// The `note:` lines saying how each of `tokens` is written: one per id written and table.
pub(crate) fn report_respelled_notes(file: &Path, edition: Edition, tokens: &[RespelledToken]) {
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
            eprintln!(
                "note: {}:{}: `{}` is written `{id}` at {} {}, per the registry pack's {}",
                file.display(),
                token.position,
                token.id,
                edition.as_str(),
                versions.join(", "),
                table_name(through),
            );
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
