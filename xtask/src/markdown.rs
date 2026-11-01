//! Just enough Markdown for the rules: sections, fences, headings, table rows and code spans.
//!
//! Fences and headings are read by `cairn-lang-docs`, whose reading `site-parity` holds to the
//! built site's.

pub use cairn_lang_docs::{heading, lines};

/// The numbered lines of the first level-`level` section whose title matches, fences left out.
pub fn section(
    text: &str,
    level: usize,
    title_matches: impl Fn(&str) -> bool,
) -> Option<Vec<(usize, &str)>> {
    let mut found: Option<Vec<(usize, &str)>> = None;
    for (index, line) in lines(text).enumerate() {
        if line.fenced {
            continue;
        }
        if let Some((depth, title)) = heading(line.text) {
            if found.is_some() && depth <= level {
                break;
            }
            if found.is_none() && depth == level && title_matches(title) {
                found = Some(Vec::new());
                continue;
            }
        }
        if let Some(lines) = &mut found {
            lines.push((index + 1, line.text));
        }
    }
    found
}

/// The text inside each code span of `text`, whose backticks pair up.
pub fn code_spans(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2)
}

/// The code spans in the first cell of a table row, or `None` when `line` is not a row.
pub fn first_cell_spans(line: &str) -> Result<Option<Vec<&str>>, String> {
    let Some(cell) = line
        .trim()
        .strip_prefix('|')
        .and_then(|rest| rest.split('|').next())
    else {
        return Ok(None);
    };
    if cell.matches('`').count() % 2 != 0 {
        return Err(format!(
            "unpaired backtick in a table row's first cell: {line:?}"
        ));
    }
    Ok(Some(code_spans(cell).map(str::trim).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const README: &str = "\
# crate

## Public API

Prose with `a::B` is not a row.

| Item | Role |
|---|---|
| `tag::Tag` / `tag::List` | Two items. |

```text
| `fenced::Row` | Not a row. |
```

### Details

| `nested::Row` | A subsection belongs to its section. |

## Next

| `next::Row` | Another section. |
";

    fn rows(text: &str) -> Vec<&str> {
        section(text, 2, |title| title == "Public API")
            .unwrap()
            .into_iter()
            .filter_map(|(_, line)| first_cell_spans(line).unwrap())
            .flatten()
            .collect()
    }

    #[test]
    fn a_section_runs_to_the_next_heading_of_its_level_and_skips_fences() {
        assert_eq!(rows(README), ["tag::Tag", "tag::List", "nested::Row"]);
    }

    #[test]
    fn a_heading_inside_a_fence_is_not_a_heading() {
        let text = "## A\n```md\n## B\n```\n| `a` |\n";
        assert_eq!(section(text, 2, |title| title == "B"), None);
        assert_eq!(
            section(text, 2, |title| title == "A").unwrap(),
            [(5, "| `a` |")]
        );
    }

    #[test]
    fn a_fence_closes_only_on_a_bare_marker_run_as_long_as_its_opening() {
        let text = "## A\n````\n```crn\n## B\n```\n````\n| `a` |\n";
        assert_eq!(section(text, 2, |title| title == "B"), None);
        assert_eq!(
            section(text, 2, |title| title == "A").unwrap(),
            [(7, "| `a` |")]
        );
    }

    #[test]
    fn an_unpaired_backtick_is_refused() {
        assert!(first_cell_spans("| `tag::Tag` and `tag::List | x |").is_err());
    }
}
