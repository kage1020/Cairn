use std::path::PathBuf;

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

fn opaque_fix(line: usize) -> String {
    format!(
        "lib.rs:{line}: internal-tier: this `doc` attribute's text is not a string literal, so the \
         rule cannot tell what the crate doc opens with; put the Internal-tier statement before \
         it, word for word, as a `//!` paragraph of its own: {STATEMENT}"
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
    let heading_next = format!("//! {STATEMENT}\n//! # Use\n");
    assert_eq!(crate_doc(&heading_next), None);
}

#[test]
fn a_block_crate_doc_opens_with_the_statement_once_rustdoc_strips_its_stars() {
    let (first, second) = STATEMENT.split_once(" and ").unwrap();
    let starred = format!("/*!\n * {first}\n * and {second}\n *\n * What the crate does.\n */\n");
    assert_eq!(crate_doc(&starred), None);
    let bare = format!("/*! {STATEMENT}\n\nWhat the crate does.\n*/\n");
    assert_eq!(crate_doc(&bare), None);
    let other = "\n\n/*!\n * What the crate does.\n */\n";
    assert_eq!(crate_doc(other), Some(crate_doc_fix(4)));
}

#[test]
fn a_doc_attribute_the_rule_cannot_read_is_reported_unless_the_statement_ends_before_it() {
    let included = "#![doc = include_str!(\"../README.md\")]\n";
    assert_eq!(crate_doc(included), Some(opaque_fix(1)));
    assert_eq!(
        crate_doc(&format!("{included}{}", inner_doc(STATEMENT))),
        Some(opaque_fix(1))
    );
    let conditional = "#![cfg_attr(docsrs, doc = \"Shown on docs.rs.\")]\n";
    assert_eq!(
        crate_doc(&format!("{conditional}//! {STATEMENT}\n")),
        Some(opaque_fix(1))
    );
    assert_eq!(
        crate_doc(&format!("//! {STATEMENT}\n{included}")),
        Some(opaque_fix(2))
    );

    assert_eq!(
        crate_doc(&format!("//! {STATEMENT}\n//!\n{included}")),
        None
    );
    assert_eq!(
        crate_doc(&format!("//! What the crate does.\n//!\n{included}")),
        Some(crate_doc_fix(1))
    );
    let not_doc = format!("#![cfg_attr(docsrs, feature(doc_cfg))]\n//! {STATEMENT}\n");
    assert_eq!(crate_doc(&not_doc), None);
}

#[test]
fn a_readme_opens_with_the_statement_under_its_title() {
    assert_eq!(
        readme(&format!("# crate\n\n{STATEMENT}\n\nWhat the crate does.\n")),
        None
    );
    assert_eq!(readme(&format!("crate\n=====\n\n{STATEMENT}\n")), None);
    assert_eq!(readme(&format!("# crate\n\n{STATEMENT}\n## Use\n")), None);
    assert_eq!(readme(&format!("\u{feff}# crate\n\n{STATEMENT}\n")), None);
}

#[test]
fn a_readme_without_a_title_opens_with_its_first_paragraph() {
    assert_eq!(readme(&format!("\n{STATEMENT}\n\n## Use\n")), None);
    assert_eq!(readme("\nWhat the crate does.\n"), Some(readme_fix(2)));
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
        crate_doc(&format!("//! # crate\n//!\n//! {STATEMENT}\n")),
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
fn a_statement_markdown_does_not_render_as_a_paragraph_is_reported() {
    assert_eq!(
        readme(&format!("# crate\n\n    {STATEMENT}\n")),
        Some(readme_fix(3))
    );
    assert_eq!(
        readme(&format!("# crate\n\n{STATEMENT}\n---\n")),
        Some(readme_fix(3))
    );
    assert_eq!(
        crate_doc(&format!(
            "//!     {STATEMENT}\n//!\n//! What the crate does.\n"
        )),
        Some(crate_doc_fix(1))
    );
    assert_eq!(crate_doc(&format!("//!     {STATEMENT}\n")), None);
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
fn a_crate_root_that_does_not_parse_is_reported_at_the_error() {
    assert_eq!(
        crate_doc("//! What the crate does.\npub fn () {}\n"),
        Some("lib.rs:2: internal-tier: expected identifier".to_owned())
    );
}

fn member(name: &str, library: bool, readme: bool, published: bool) -> Member {
    let dir = PathBuf::from("crates").join(name);
    Member {
        name: name.to_owned(),
        roots: vec![dir.join(if library { "src/lib.rs" } else { "src/main.rs" })],
        library: library.then(|| dir.join("src/lib.rs")),
        readme: readme.then(|| dir.join("README.md")),
        published,
        dir,
    }
}

fn rendered(members: &[Member], files: &[(&str, &str)]) -> Vec<String> {
    let read = |path: &Path| {
        files
            .iter()
            .find(|(name, _)| Path::new(name) == path)
            .map(|(_, text)| (*text).to_owned())
            .ok_or_else(|| format!("cannot read {}: not found", path.display()))
    };
    findings(members, read)
        .iter()
        .map(|finding| finding.render(RULE.name))
        .collect()
}

#[test]
fn each_published_library_is_held_to_both_pages_and_the_rest_are_skipped() {
    let stated_doc = inner_doc(STATEMENT);
    let stated_readme = format!("# crate\n\n{STATEMENT}\n");
    let members = [
        member("stated", true, true, true),
        member("bare-readme", true, true, true),
        member("bare-doc", true, true, true),
        member("no-readme", true, false, true),
        member("unread", true, true, true),
        member("unpublished", true, true, false),
        member("binary", false, true, true),
    ];
    let files = [
        ("crates/stated/src/lib.rs", stated_doc.as_str()),
        ("crates/stated/README.md", stated_readme.as_str()),
        ("crates/bare-readme/src/lib.rs", stated_doc.as_str()),
        ("crates/bare-readme/README.md", "# crate\n\nWhat it does.\n"),
        ("crates/bare-doc/src/lib.rs", "//! What it does.\n"),
        ("crates/bare-doc/README.md", stated_readme.as_str()),
        ("crates/no-readme/src/lib.rs", stated_doc.as_str()),
        ("crates/unpublished/src/lib.rs", "//! What it does.\n"),
        ("crates/unpublished/README.md", "# crate\n"),
        ("crates/binary/README.md", "# crate\n"),
    ];
    assert_eq!(
        rendered(&members, &files),
        [
            readme_fix(3).replacen("README.md", "crates/bare-readme/README.md", 1),
            crate_doc_fix(1).replacen("lib.rs", "crates/bare-doc/src/lib.rs", 1),
            format!(
                "crates/no-readme/Cargo.toml: internal-tier: publishes a library with no README \
                 for crates.io to show; remove `readme = false` if the manifest sets it, and add \
                 a README.md beside it whose first paragraph under its title is, word for word: \
                 {STATEMENT}"
            ),
            "crates/unread/src/lib.rs: internal-tier: cannot read crates/unread/src/lib.rs: not \
             found"
                .to_owned(),
            "crates/unread/README.md: internal-tier: cannot read crates/unread/README.md: not \
             found"
                .to_owned(),
        ]
    );
}

#[test]
fn a_workspace_with_no_published_library_is_reported() {
    let members = [
        member("unpublished", true, true, false),
        member("binary", false, true, true),
    ];
    assert_eq!(
        rendered(&members, &[]),
        [
            "Cargo.toml: internal-tier: found no published workspace crate with a library; \
             update this rule if the workspace changed"
        ]
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

#[test]
fn a_link_to_a_section_the_page_lacks_is_reported_at_the_page() {
    assert!(link_finding(STATEMENT).is_none());
    let broken = STATEMENT.replace("#internal", "#no-such-section");
    assert_eq!(
        link_finding(&broken).map(|finding| finding.render(RULE.name)),
        Some(format!(
            "website/src/content/docs/spec/compatibility.md: internal-tier: has no section at \
             `#no-such-section`, where the Internal-tier statement every published library crate \
             opens with links; give the tier definition that heading back, or change the link in \
             xtask's `STATEMENT` and in every crate: {broken}"
        ))
    );
}
