use super::*;

const LIBRARY_TEXT: &str = "theme furniture:\n  slot chair -> @oak_stairs\n\ndef chair size=1x1:\n";

fn chapter(block: &str) -> String {
    format!("# Chapter\n\nProse.\n\n```\n{block}```\n\nMore prose.\n")
}

fn found(text: &str) -> Option<String> {
    compare(Path::new("spec.md"), text, LIBRARY_TEXT).map(|f| f.render(RULE.name))
}

#[test]
fn a_block_printing_the_library_draws_nothing() {
    assert_eq!(found(&chapter(LIBRARY_TEXT)), None);
}

#[test]
fn a_changed_line_is_reported_where_it_stands() {
    let block = LIBRARY_TEXT.replace("@oak_stairs", "@spruce_stairs");
    assert_eq!(
        found(&chapter(&block)),
        Some(format!(
            "spec.md:7: furniture-library: the library block parts from {LIBRARY} at its line 2; \
             print the file as it is"
        )),
    );
}

#[test]
fn a_shorter_block_is_reported_where_it_ends() {
    let block = "theme furniture:\n  slot chair -> @oak_stairs\n";
    assert_eq!(
        found(&chapter(block)),
        Some(format!(
            "spec.md:8: furniture-library: the library block parts from {LIBRARY} at its line 3; \
             print the file as it is"
        )),
    );
}

#[test]
fn a_chapter_without_the_block_is_reported() {
    assert_eq!(
        found("# Chapter\n\n```\ntheme other:\n```\n"),
        Some(format!(
            "spec.md: furniture-library: prints no fenced block opening with `theme furniture:`; \
             print {LIBRARY} there"
        )),
    );
}
