use super::*;

const TEST: Language = Language {
    dir: "spec",
    words: "Flag",
    flag: "**Flag.**",
};

fn section(line: usize, heading: &str, flagged: bool) -> Section {
    Section {
        line,
        heading: heading.to_owned(),
        flagged,
    }
}

fn page<'a>(path: &'a str, language: &'static Language, flags: Flags) -> Page<'a> {
    Page {
        path: Path::new(path),
        language,
        flags,
    }
}

fn rendered(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| finding.render(RULE.name))
        .collect()
}

const MISPLACED: &str = "this subject-to-change flag is not the first paragraph under a heading; \
                         move it directly under the heading of the section whose syntax it names";

#[test]
fn a_flag_counts_only_as_the_first_paragraph_under_a_heading() {
    let text = "\
---
title: \"1. Page\"
---

**Flag.** Above every heading.

## 1.1 Flagged

**Flag.** Not built yet.

Prose.

**Flag.** A second paragraph.

### 1.1.1 After prose

Prose that wraps onto a line
**Flag.** opening with the flag's words.

## 1.2 After a list

- **Flag.** An item.
";
    assert_eq!(
        flags(text, &TEST),
        Flags {
            sections: vec![
                section(7, "1.1 Flagged", true),
                section(15, "1.1.1 After prose", false),
                section(20, "1.2 After a list", false),
            ],
            misplaced: vec![5, 13],
            malformed: vec![22],
        }
    );
}

#[test]
fn a_block_opening_with_the_flags_words_but_written_otherwise_is_malformed() {
    let text = "\
## 1.1 Variants

**Flag**. The stop outside the bold.

 **Flag.** Indented one space.

> **Flag.** In a block quote.

- Prose.
- *Flag.* In a second item.
1. Flag in a numbered item.

**Flag.** Well formed, after the others.
";
    assert_eq!(
        flags(text, &TEST),
        Flags {
            sections: vec![section(1, "1.1 Variants", false)],
            misplaced: vec![13],
            malformed: vec![3, 5, 7, 10, 11],
        }
    );
}

#[test]
fn a_heading_or_flag_inside_fenced_code_is_not_read() {
    let text = "\
## 1.1 Opens with code

```
## Not a heading
```crn
**Flag.** Not a flag.
```

**Flag.** After the code.

~~~~text
~~~
## Still code
~~~~

####### Seven marks are not a heading.

## 1.2 Last
";
    assert_eq!(
        flags(text, &TEST),
        Flags {
            sections: vec![
                section(1, "1.1 Opens with code", false),
                section(18, "1.2 Last", false),
            ],
            misplaced: vec![9],
            malformed: vec![],
        }
    );
}

#[test]
fn each_page_reports_its_stray_flags_before_the_sections_only_one_page_flags() {
    let english = page(
        "en/page.md",
        &ENGLISH,
        Flags {
            sections: vec![section(3, "1.1 A", true), section(9, "1.2 B", false)],
            misplaced: vec![7],
            malformed: vec![],
        },
    );
    let japanese = page(
        "ja/page.md",
        &JAPANESE,
        Flags {
            sections: vec![section(4, "1.1 あ", false), section(8, "1.2 い", true)],
            misplaced: vec![12],
            malformed: vec![14],
        },
    );
    assert_eq!(
        rendered(&report(Some(&english), Some(&japanese))),
        [
            format!("en/page.md:7: subject-to-change: {MISPLACED}"),
            format!("ja/page.md:12: subject-to-change: {MISPLACED}"),
            "ja/page.md:14: subject-to-change: this block opens with \"変更の余地あり\" but is not \
             written as the flag; start an unindented paragraph with `**変更の余地あり。**`, or \
             reword it"
                .to_owned(),
            "ja/page.md:4: subject-to-change: \"1.1 あ\" is not flagged subject to change, but \
             \"1.1 A\" in en/page.md is; flag both sections or neither"
                .to_owned(),
            "en/page.md:9: subject-to-change: \"1.2 B\" is not flagged subject to change, but \
             \"1.2 い\" in ja/page.md is; flag both sections or neither"
                .to_owned(),
        ]
    );
}

#[test]
fn pages_whose_headings_differ_in_number_are_held_to_them_only_when_one_has_a_flag() {
    let japanese = page(
        "ja/page.md",
        &JAPANESE,
        Flags {
            sections: vec![],
            misplaced: vec![],
            malformed: vec![],
        },
    );
    let english = |flagged| {
        page(
            "en/page.md",
            &ENGLISH,
            Flags {
                sections: vec![section(3, "1.1 A", flagged)],
                misplaced: vec![5],
                malformed: vec![],
            },
        )
    };
    assert_eq!(
        rendered(&report(Some(&english(true)), Some(&japanese))),
        [
            format!("en/page.md:5: subject-to-change: {MISPLACED}"),
            "ja/page.md: subject-to-change: has 0 headings and en/page.md has 1, so their flags \
             cannot be paired; give the two pages the same headings"
                .to_owned(),
        ]
    );
    assert_eq!(
        rendered(&report(Some(&english(false)), Some(&japanese))),
        [format!("en/page.md:5: subject-to-change: {MISPLACED}")]
    );
}

#[test]
fn a_page_with_no_twin_still_reports_its_stray_flags() {
    let english = page(
        "en/page.md",
        &ENGLISH,
        Flags {
            sections: vec![section(1, "1.1 A", true)],
            misplaced: vec![5],
            malformed: vec![],
        },
    );
    let japanese = page(
        "ja/page.md",
        &JAPANESE,
        Flags {
            sections: vec![],
            misplaced: vec![],
            malformed: vec![3],
        },
    );
    assert_eq!(
        rendered(&report(Some(&english), None)),
        [format!("en/page.md:5: subject-to-change: {MISPLACED}")]
    );
    assert_eq!(
        rendered(&report(None, Some(&japanese))),
        [
            "ja/page.md:3: subject-to-change: this block opens with \"変更の余地あり\" but is not \
             written as the flag; start an unindented paragraph with `**変更の余地あり。**`, or \
             reword it"
        ]
    );
}

/// The file `name` in `language`'s spec directory.
fn spec(language: &Language, name: &str) -> PathBuf {
    Path::new(language.dir).join(name)
}

const NO_PAGE: &str = "subject-to-change: found no spec page; update this rule's directory if the \
                       spec moved";

#[test]
fn spec_pages_pair_by_file_name_and_a_page_with_no_twin_stands_alone() {
    let files = [
        PathBuf::from("README.md"),
        spec(&JAPANESE, "a.md"),
        spec(&JAPANESE, "c.md"),
        spec(&ENGLISH, "a.md"),
        spec(&ENGLISH, "b.md"),
        spec(&ENGLISH, "nested").join("a.md"),
        spec(&ENGLISH, "notes.txt"),
    ];
    let (pairs, findings) = pair(&files);
    assert_eq!(
        pairs,
        [
            (Some(files[3].as_path()), Some(files[1].as_path())),
            (Some(files[4].as_path()), None),
            (None, Some(files[2].as_path())),
        ]
    );
    assert_eq!(rendered(&findings), Vec::<String>::new());
}

#[test]
fn a_spec_directory_with_no_page_is_reported() {
    let files = [PathBuf::from("README.md"), spec(&ENGLISH, "a.md")];
    let (pairs, findings) = pair(&files);
    assert_eq!(pairs, [(Some(files[1].as_path()), None)]);
    assert_eq!(
        rendered(&findings),
        [format!("{}: {NO_PAGE}", JAPANESE.dir)]
    );
    let (pairs, findings) = pair(&[]);
    assert_eq!(pairs, Vec::<Pair>::new());
    assert_eq!(
        rendered(&findings),
        [
            format!("{}: {NO_PAGE}", ENGLISH.dir),
            format!("{}: {NO_PAGE}", JAPANESE.dir),
        ]
    );
}

#[test]
fn the_flag_is_the_one_the_definition_section_shows() {
    let defined = "\
### Evolving

#### Flag

A section opens with a **Flag.** paragraph.

#### Next
";
    assert!(defines(defined, &TEST));
    let elsewhere =
        "#### Flag\n\nA paragraph.\n\n#### Next\n\n**Flag.** Shown in another section.\n";
    assert!(!defines(elsewhere, &TEST));
    let moved = "### Flag\n\nA section opens with a **Flag.** paragraph.\n";
    assert!(!defines(moved, &TEST));
}
