use super::*;

/// `text` as `//!` lines of four words each.
fn inner_doc(text: &str) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    words
        .chunks(4)
        .map(|line| format!("//! {}\n", line.join(" ")))
        .collect::<Vec<_>>()
        .concat()
}

fn crate_doc(source: &str) -> Option<String> {
    opening_finding(Path::new("lib.rs"), source, Place::CrateDoc)
        .map(|finding| finding.render(RULE.name))
}

fn readme(text: &str) -> Option<String> {
    opening_finding(Path::new("README.md"), text, Place::Readme)
        .map(|finding| finding.render(RULE.name))
}

fn crate_doc_fix(line: usize) -> String {
    format!(
        "lib.rs:{line}: internal-tier: the crate doc does not open with the Internal-tier \
         statement; make this its first paragraph, word for word, over as many `//!` lines as it \
         takes: {STATEMENT}"
    )
}

fn readme_fix(line: usize) -> String {
    format!(
        "README.md:{line}: internal-tier: the README does not open with the Internal-tier \
         statement; make this its first paragraph under the title, word for word: {STATEMENT}"
    )
}

#[test]
fn a_crate_doc_opens_with_the_statement_over_any_number_of_lines() {
    let summary = "//!\n//! What the crate does.\n\npub mod a;\n";
    assert_eq!(
        crate_doc(&format!("{}{summary}", inner_doc(STATEMENT))),
        None
    );
    let after_attributes =
        format!("#![forbid(unsafe_code)]\n#![crate_type = \"lib\"]\n//! {STATEMENT}\n{summary}");
    assert_eq!(crate_doc(&after_attributes), None);
}

#[test]
fn a_readme_opens_with_the_statement_under_its_title() {
    let text = format!("# crate\n\n{STATEMENT}\n\nWhat the crate does.\n");
    assert_eq!(readme(&text), None);
    assert_eq!(readme(&format!("\n{STATEMENT}\n\n## Use\n")), None);
}

#[test]
fn a_missing_statement_is_reported_where_the_first_paragraph_starts_or_belongs() {
    assert_eq!(
        crate_doc("//! What the crate does,\n//! over two lines.\n\npub mod a;\n"),
        Some(crate_doc_fix(1))
    );
    assert_eq!(
        crate_doc("#![forbid(unsafe_code)]\n\n//! What the crate does.\n"),
        Some(crate_doc_fix(3))
    );
    assert_eq!(crate_doc("pub mod a;\n"), Some(crate_doc_fix(1)));
    assert_eq!(
        crate_doc(&format!("/// {STATEMENT}\npub fn a() {{}}\n")),
        Some(crate_doc_fix(1))
    );

    assert_eq!(
        readme("# crate\n\nWhat the crate does.\n"),
        Some(readme_fix(3))
    );
    assert_eq!(
        readme(&format!("## Status\n\n{STATEMENT}\n")),
        Some(readme_fix(1))
    );
    assert_eq!(
        readme("\n\n# crate\n\n\n## Use\n\nProse.\n"),
        Some(readme_fix(6))
    );
    assert_eq!(readme("\n# crate\n"), Some(readme_fix(2)));
    assert_eq!(readme(""), Some(readme_fix(1)));
}

#[test]
fn an_altered_statement_is_reported() {
    let altered = [
        STATEMENT.replace("no promise", "no promises"),
        STATEMENT.replace("#internal", "#evolving"),
        STATEMENT.replace("[Internal tier](", "Internal tier ("),
        STATEMENT.replace("This crate's", "The"),
        format!("> {STATEMENT}"),
        format!("**{STATEMENT}**"),
        format!("{STATEMENT} Pin it."),
    ];
    for statement in altered {
        assert_eq!(
            crate_doc(&inner_doc(&statement)),
            Some(crate_doc_fix(1)),
            "{statement}"
        );
        assert_eq!(
            readme(&format!("# crate\n\n{statement}\n")),
            Some(readme_fix(3)),
            "{statement}"
        );
    }
}

#[test]
fn the_statement_is_a_paragraph_of_its_own() {
    let continued = format!("# crate\n\n{STATEMENT}\nWhat the crate does.\n");
    assert_eq!(readme(&continued), Some(readme_fix(3)));
    let (first, second) = STATEMENT.split_once(" and ").unwrap();
    let split = format!("//! {first}\n//!\n//! and {second}\n");
    assert_eq!(crate_doc(&split), Some(crate_doc_fix(1)));
}

#[test]
fn a_statement_below_the_first_paragraph_is_reported_at_the_first() {
    let below = format!("# crate\n\nWhat the crate does.\n\n{STATEMENT}\n");
    assert_eq!(readme(&below), Some(readme_fix(3)));
    let below = format!("//! What the crate does.\n//!\n{}", inner_doc(STATEMENT));
    assert_eq!(crate_doc(&below), Some(crate_doc_fix(1)));
}

#[test]
fn a_statement_in_only_one_of_the_two_places_is_reported_at_the_other() {
    let stated = (inner_doc(STATEMENT), format!("# crate\n\n{STATEMENT}\n"));
    let unstated = (
        "//! What the crate does.\n",
        "# crate\n\nWhat the crate does.\n",
    );
    assert_eq!(
        [crate_doc(&stated.0), readme(unstated.1)],
        [None, Some(readme_fix(3))]
    );
    assert_eq!(
        [crate_doc(unstated.0), readme(&stated.1)],
        [Some(crate_doc_fix(1)), None]
    );
}

#[test]
fn a_crate_root_that_does_not_parse_is_reported_at_the_error() {
    assert_eq!(
        crate_doc("//! What the crate does.\npub fn () {}\n"),
        Some("lib.rs:2: internal-tier: expected identifier".to_owned())
    );
}

#[test]
fn the_statement_links_to_the_tier_definition_on_the_site() {
    assert_eq!(link(STATEMENT), Some(("spec/compatibility", "internal")));
    assert_eq!(
        link("[Internal](https://example.com/spec/compatibility/#internal)"),
        None
    );
    assert_eq!(link("Internal, with no link."), None);
}
