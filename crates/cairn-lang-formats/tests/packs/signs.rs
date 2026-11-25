//! What the built-in packs make of a `sign`: the standing sign its slot binds and the wall sign it
//! hangs as, at every target, each written as that target spells it (`spec/blockstate` "Derive by
//! default, promote on override").

use std::path::Path;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};
use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::build::finding::Found;
use cairn_lang_formats::build::lowering::check_lowering;
use cairn_lang_formats::data_version::resolve_bedrock_target;
use cairn_lang_formats::info::{InfoRequest, InfoSink, ReportFormat, RequestedEditions, info};
use cairn_lang_formats::registry::builtin_pack;

use crate::common::{compile_source, note_fields, states_for, supported_versions, written_palette};

/// A room whose theme binds `board` to `@<token>`, standing a sign on its floor and hanging one on
/// its front wall.
fn signs_of(token: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot board -> @{token}\n\n\
         struct s size=5x5\n  walls mat_slot=wall height=3\n  \
         sign at=2,1,2 mat_slot=board\n  sign at=inside.front offset=1 y=1 mat_slot=board\n"
    )
}

/// The standing sign and the hung one [`signs_of`] sets, each as `id[properties]`.
fn placed(out: &BlockArrayIr) -> [String; 2] {
    let array: &BlockArray = &out.structures["struct::s"];
    [(2, 1, 2), (1, 1, 3)].map(|(x, y, z)| {
        let at = array.dims.index(x, y, z).expect("inside the room");
        let state = &array.palette.entries[usize::from(array.voxels[at].0)];
        format!("{}[{}]", state.id, state.properties_text())
    })
}

/// The woods whose signs every target has.
const WOODS: [&str; 11] = [
    "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "bamboo",
    "crimson", "warped",
];

/// The woods later targets add, each with the first Java and the first Bedrock target that has it.
const LATER_WOODS: [(&str, &str, &str); 2] = [
    ("pale_oak", "1.21.4", "1.21.60"),
    ("poplar", "26.3", "1.26.60"),
];

/// The woods whose signs `edition`'s target `version` has.
fn woods_at(edition: Edition, version: &str) -> Vec<&'static str> {
    let order = builtin_pack(edition).version_order();
    let key = |version: &str| order.key_of(version).expect("a version of the pack");
    let later = LATER_WOODS.into_iter().filter_map(|(wood, java, bedrock)| {
        let since = match edition {
            Edition::Java => java,
            Edition::Bedrock => bedrock,
        };
        (key(version) >= key(since)).then_some(wood)
    });
    WOODS.into_iter().chain(later).collect()
}

#[test]
fn bedrock_writes_an_oak_or_dark_oak_sign_as_its_own_standing_and_wall_sign() {
    for version in supported_versions(builtin_pack(Edition::Bedrock)) {
        for (wood, standing, wall) in [
            ("oak", "minecraft:standing_sign", "minecraft:wall_sign"),
            (
                "dark_oak",
                "minecraft:darkoak_standing_sign",
                "minecraft:darkoak_wall_sign",
            ),
        ] {
            let built = compile_source(
                &signs_of(&format!("{wood}_sign")),
                Edition::Bedrock,
                version,
            )
            .unwrap_or_else(|codes| panic!("{wood} at {version} refused: {codes:?}"));
            assert_eq!(
                placed(&built),
                [
                    format!("minecraft:{wood}_sign[rotation=0]"),
                    format!("minecraft:{wood}_wall_sign[facing=north]"),
                ],
                "{wood} at {version}",
            );
            let target = resolve_bedrock_target(version).expect("a shipped target");
            let (root, notes) = build_mcstructure_tag(
                &built.structures["struct::s"],
                &target,
                &states_for(&target),
            )
            .unwrap_or_else(|err| panic!("{wood} at {version} not written: {err}"));
            assert_eq!(note_fields(&notes), [], "{wood} at {version}");
            assert_eq!(
                written_palette(&root),
                [
                    "minecraft:air[]".to_owned(),
                    "minecraft:cobblestone[]".to_owned(),
                    format!("{standing}[ground_sign_direction=0]"),
                    format!("{wall}[facing_direction=2]"),
                ],
                "{wood} at {version}",
            );
        }
    }
}

#[test]
fn every_wood_stands_and_hangs_its_sign_at_every_target_that_has_it() {
    for edition in [Edition::Java, Edition::Bedrock] {
        for version in supported_versions(builtin_pack(edition)) {
            for wood in woods_at(edition, version) {
                let built = compile_source(&signs_of(&format!("{wood}_sign")), edition, version)
                    .unwrap_or_else(|codes| {
                        panic!("{wood} at {edition:?} {version} refused: {codes:?}")
                    });
                assert_eq!(
                    placed(&built),
                    [
                        format!("minecraft:{wood}_sign[rotation=0]"),
                        format!("minecraft:{wood}_wall_sign[facing=north]"),
                    ],
                    "{wood} at {edition:?} {version}",
                );
                if edition == Edition::Bedrock {
                    let target = resolve_bedrock_target(version).expect("a shipped target");
                    let (_, notes) = build_mcstructure_tag(
                        &built.structures["struct::s"],
                        &target,
                        &states_for(&target),
                    )
                    .unwrap_or_else(|err| panic!("{wood} at {version} not written: {err}"));
                    assert_eq!(note_fields(&notes), [], "{wood} at {version}");
                }
            }
        }
    }
}

#[test]
fn a_check_with_no_target_finds_nothing_on_a_sign_every_target_builds() {
    for wood in ["oak", "dark_oak"] {
        let source = signs_of(&format!("{wood}_sign"));
        let ir = lower(&parse(&source).expect("the source parses"));
        for edition in [None, Some(Edition::Java), Some(Edition::Bedrock)] {
            assert_eq!(
                check_lowering(&ir, edition, None).diagnostics,
                [],
                "{wood} under {edition:?}",
            );
        }
    }
}

/// An [`InfoSink`] that keeps what it is sent.
#[derive(Default)]
struct Kept {
    findings: Vec<String>,
}

impl InfoSink for Kept {
    fn finding(&mut self, finding: &Found) {
        self.findings.push(format!("{finding:?}"));
    }

    fn line(&mut self, _line: &str) {}
}

#[test]
fn info_finds_an_oak_or_dark_oak_sign_buildable_at_every_target() {
    for wood in ["oak", "dark_oak"] {
        let source = signs_of(&format!("{wood}_sign"));
        let editions = RequestedEditions::default();
        let mut sink = Kept::default();
        let document = info(
            &InfoRequest {
                file: Path::new("signs.crn"),
                source: &source,
                editions: &editions,
                lock: Some(Path::new("no/such/dir/signs.crn.lock")),
                format: ReportFormat::Text,
            },
            &mut sink,
        )
        .unwrap_or_else(|refusal| panic!("{wood}: {:?}", refusal.findings));
        assert_eq!(sink.findings, Vec::<String>::new(), "{wood}");
        let buildable: Vec<(Edition, Vec<String>)> = document
            .axes
            .buildable_targets
            .into_iter()
            .map(|row| (row.edition, row.buildable))
            .collect();
        let every = |edition: Edition| builtin_pack(edition).targetable_versions();
        assert_eq!(
            buildable,
            [
                (Edition::Java, every(Edition::Java)),
                (Edition::Bedrock, every(Edition::Bedrock)),
            ],
            "{wood}",
        );
    }
}
