use super::*;

const FLAG: &str = "**Flag.**";

fn section(line: usize, heading: &str, flagged: bool) -> Section {
    Section {
        line,
        heading: heading.to_owned(),
        flagged,
    }
}

fn rendered(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| finding.render(RULE.name))
        .collect()
}

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
        flags(text, FLAG),
        Flags {
            sections: vec![
                section(7, "1.1 Flagged", true),
                section(15, "1.1.1 After prose", false),
                section(20, "1.2 After a list", false),
            ],
            misplaced: vec![5, 13],
        }
    );
}

#[test]
fn a_heading_or_flag_inside_fenced_code_is_not_read() {
    let text = "\
## 1.1 Opens with code

```
## Not a heading

**Flag.** Not a flag.
```

**Flag.** After the code.

~~~text
```
~~~

## 1.2 Last
";
    assert_eq!(
        flags(text, FLAG),
        Flags {
            sections: vec![
                section(1, "1.1 Opens with code", false),
                section(15, "1.2 Last", false),
            ],
            misplaced: vec![9],
        }
    );
}

#[test]
fn a_section_flagged_in_one_language_only_is_reported_at_the_other() {
    let english = Flags {
        sections: vec![section(3, "1.1 A", true), section(9, "1.2 B", false)],
        misplaced: vec![],
    };
    let japanese = Flags {
        sections: vec![section(4, "1.1 あ", false), section(8, "1.2 い", true)],
        misplaced: vec![12],
    };
    let findings = compare(
        (Path::new("en/page.md"), &english),
        (Path::new("ja/page.md"), &japanese),
    );
    assert_eq!(
        rendered(&findings),
        [
            "ja/page.md:12: subject-to-change: a subject-to-change flag opens its section; make it \
             the first paragraph under the heading of the section whose syntax it names",
            "ja/page.md:4: subject-to-change: \"1.1 あ\" is not flagged subject to change, but \
             \"1.1 A\" in en/page.md is; flag both sections or neither",
            "en/page.md:9: subject-to-change: \"1.2 B\" is not flagged subject to change, but \
             \"1.2 い\" in ja/page.md is; flag both sections or neither",
        ]
    );
}

#[test]
fn pages_whose_headings_differ_in_number_are_not_paired() {
    let english = Flags {
        sections: vec![section(3, "1.1 A", true)],
        misplaced: vec![],
    };
    let japanese = Flags {
        sections: vec![],
        misplaced: vec![],
    };
    let findings = compare(
        (Path::new("en/page.md"), &english),
        (Path::new("ja/page.md"), &japanese),
    );
    assert_eq!(
        rendered(&findings),
        [
            "ja/page.md: subject-to-change: has 0 headings and en/page.md has 1, so their flags \
          cannot be paired; give the two pages the same headings"
        ]
    );
}
