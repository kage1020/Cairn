use super::*;

const PACK_TEXT: &str = r#"{ "extracted": { "states": [
    { "mc_version": "1.21.0", "java": "1.21" },
    { "mc_version": "1.21.60", "java": "1.21.5" }
] } }"#;

/// A state map naming stone, Java's oak slab as Bedrock's `oak_double_slab`, and the mud and
/// mangrove roots it adds at 1.21.60.
const STATES_TEXT: &str = r#"{ "families": [
    { "mc_versions": ["1.21.0", "1.21.60"], "ids": ["stone", "oak_double_slab=oak_slab"] },
    { "mc_versions": ["1.21.60"], "ids": ["mud", "mangrove_roots"] }
] }"#;

/// Java conductor tables at 1.21 and 1.21.4, the second adding mud and the vault no map names.
const JAVA_TEXT: &str = r#"{ "rows": [
    { "mc_versions": ["1.21", "1.21.4"], "ids": ["stone", "oak_slab", "mangrove_roots"] },
    { "mc_versions": ["1.21.4"], "ids": ["mud", "vault"] }
] }"#;

fn texts(bedrock: &str) -> Texts<'_> {
    Texts {
        pack: PACK_TEXT,
        states: STATES_TEXT,
        java: JAVA_TEXT,
        bedrock,
    }
}

const ROOTS: Insulating = Insulating {
    id: "mangrove_roots",
    mc_versions: &["1.21.60"],
    why: "a source says so",
};

fn rendered(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| finding.render("bedrock-conductors"))
        .collect()
}

fn unclassified(id: &str, java: &str, version: &str) -> String {
    Finding::new(
        BEDROCK,
        None,
        format!(
            "`{id}` conducts in Java {java}, and the Bedrock {version} state map names it, but no \
             row names it at {version}: give it a row, conducting or `unsettled`, or record in \
             {RULE_FILE} the source that shows Bedrock makes it pass no power"
        ),
    )
    .render("bedrock-conductors")
}

#[test]
fn a_table_that_classifies_every_java_conductor_its_map_names_passes() {
    let bedrock = r#"{ "rows": [
        { "mc_versions": ["1.21.0", "1.21.60"], "ids": ["minecraft:stone", "oak_slab"] },
        { "mc_versions": ["1.21.60"], "ids": ["mud"], "unsettled": true }
    ] }"#;
    assert_eq!(
        rendered(&compare(&texts(bedrock), &[ROOTS])),
        Vec::<String>::new()
    );
}

#[test]
fn a_java_conductor_the_map_names_without_a_row_or_a_record_is_reported() {
    let bedrock = r#"{ "rows": [
        { "mc_versions": ["1.21.0", "1.21.60"], "ids": ["stone"] }
    ] }"#;
    assert_eq!(
        rendered(&compare(&texts(bedrock), &[])),
        [
            unclassified("oak_slab", "1.21", "1.21.0"),
            unclassified("mangrove_roots", "1.21.4", "1.21.60"),
            unclassified("mud", "1.21.4", "1.21.60"),
            unclassified("oak_slab", "1.21.4", "1.21.60"),
        ],
    );
}

#[test]
fn a_record_that_classifies_nothing_or_a_block_with_a_row_is_reported() {
    let bedrock = r#"{ "rows": [
        { "mc_versions": ["1.21.0", "1.21.60"], "ids": ["stone", "oak_slab", "mangrove_roots"] },
        { "mc_versions": ["1.21.60"], "ids": ["mud"], "unsettled": true }
    ] }"#;
    let unknown = Insulating {
        id: "vault",
        mc_versions: &["1.21.60"],
        why: "no map names it",
    };
    let stale = |id: &str, why: &str| {
        Finding::new(
            RULE_FILE,
            None,
            format!(
                "records `{id}` as passing no power on Bedrock 1.21.60 ({why}), but it is no Java \
                 conductor that version's state map names without a row; drop the record"
            ),
        )
        .render("bedrock-conductors")
    };
    assert_eq!(
        rendered(&compare(&texts(bedrock), &[ROOTS, unknown])),
        [
            stale("mangrove_roots", "a source says so"),
            stale("vault", "no map names it")
        ],
    );
}

#[test]
fn a_version_whose_release_no_java_table_reaches_is_reported() {
    let pack = r#"{ "extracted": { "states": [
        { "mc_version": "1.21.0", "java": "1.20.4" },
        { "mc_version": "1.21.60", "java": "1.21.5" }
    ] } }"#;
    let bedrock = r#"{ "rows": [
        { "mc_versions": ["1.21.60"], "ids": ["stone", "oak_slab", "mud", "mangrove_roots"] }
    ] }"#;
    let found = compare(
        &Texts {
            pack,
            ..texts(bedrock)
        },
        &[],
    );
    assert_eq!(
        rendered(&found),
        [Finding::new(
            JAVA,
            None,
            "no table at or before Java 1.20.4, which the Bedrock 1.21.0 state map follows",
        )
        .render("bedrock-conductors")],
    );
}

#[test]
fn a_file_that_is_not_the_json_the_rule_reads_is_reported() {
    assert_eq!(
        rendered(&compare(&texts(r#"{ "rows": [{ "ids": "stone" }] }"#), &[])),
        [
            Finding::new(BEDROCK, None, "a row's `ids` is no array of strings")
                .render("bedrock-conductors")
        ],
    );
}
