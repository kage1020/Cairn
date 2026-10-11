//! Every diagnostic code has a row of its own in the spec's catalog, in each language: a code is
//! what `--format json` consumers branch on.

use std::collections::BTreeSet;
use std::path::Path;

use syn::visit::Visit;

use super::{Finding, Rule};
use crate::markdown::section;
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "diagnostic-catalog",
    summary: "every core DiagnosticCode has a row in spec/lint's catalog, in each language",
    check,
};

const CORE_SRC: &str = "crates/cairn-lang-core/src";

/// The spec directory in each language, with its catalog section's title. The chapter file is
/// joined on apart, since a path spelled whole would read as a citation to `spec-citations`.
const CATALOGS: &[(&str, &str)] = &[
    ("website/src/content/docs/spec", "Diagnostic codes"),
    ("website/src/content/docs/ja/spec", "診断コード"),
];

const LINT_CHAPTER: &str = "lint.md";

/// String literals in the body of `impl DiagnosticCode { fn as_str }`.
#[derive(Default)]
struct Codes {
    in_as_str: bool,
    found: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for Codes {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let is_code = matches!(&*item.self_ty, syn::Type::Path(path)
            if path.path.segments.last().is_some_and(|last| last.ident == "DiagnosticCode"));
        if !is_code || item.trait_.is_some() {
            return;
        }
        for impl_item in &item.items {
            if let syn::ImplItem::Fn(function) = impl_item
                && function.sig.ident == "as_str"
            {
                self.in_as_str = true;
                self.visit_block(&function.block);
                self.in_as_str = false;
            }
        }
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if self.in_as_str {
            self.found.insert(literal.value());
        }
    }
}

fn catalog_rows(text: &str, title: &str) -> Option<BTreeSet<String>> {
    let lines = section(text, 2, |heading| heading.ends_with(title))?;
    Some(
        lines
            .into_iter()
            .filter_map(|(_, line)| line.strip_prefix("| `")?.split_once("` |"))
            .map(|(code, _)| code.to_owned())
            .filter(|code| code.starts_with("E_") || code.starts_with("W_"))
            .collect(),
    )
}

fn check(repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut codes = Codes::default();
    for path in repo.files_in(Path::new(CORE_SRC), &["rs"]) {
        let Ok(source) = repo.read(path) else {
            continue;
        };
        match syn::parse_file(&source) {
            Ok(file) => codes.visit_file(&file),
            Err(error) => findings.push(Finding::new(
                RULE.name,
                path,
                Some(error.span().start().line),
                error.to_string(),
            )),
        }
    }
    if codes.found.is_empty() {
        findings.push(Finding::new(
            RULE.name,
            CORE_SRC,
            None,
            "found no `impl DiagnosticCode { fn as_str }` returning code strings",
        ));
        return findings;
    }

    for (dir, title) in CATALOGS {
        let path = Path::new(dir).join(LINT_CHAPTER);
        let path = path.as_path();
        let rows = match repo.read(path) {
            Ok(text) => catalog_rows(&text, title),
            Err(error) => {
                findings.push(Finding::new(RULE.name, path, None, error));
                continue;
            }
        };
        let Some(rows) = rows.filter(|rows| !rows.is_empty()) else {
            findings.push(Finding::new(
                RULE.name,
                path,
                None,
                format!("no code rows under a `## … {title}` heading"),
            ));
            continue;
        };
        for code in codes.found.difference(&rows) {
            findings.push(Finding::new(
                RULE.name,
                path,
                None,
                format!("`{code}` has no row of its own in the catalog"),
            ));
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_as_str_literals_are_codes() {
        let source = r#"
impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self { Self::A => "E_A", Self::B => "W_B" }
    }
    pub fn other(self) -> &'static str { "E_NOT_A_CODE" }
}
impl Severity { pub fn as_str(self) -> &'static str { "error" } }
"#;
        let mut codes = Codes::default();
        codes.visit_file(&syn::parse_file(source).unwrap());
        assert_eq!(codes.found.into_iter().collect::<Vec<_>>(), ["E_A", "W_B"]);
    }

    #[test]
    fn rows_are_read_from_the_catalog_section_and_its_subsections() {
        let text = "\
## 1.1 Diagnostic codes
| `E_A` | a |
### Group
| `W_B` | b, unlike `E_C` |
## 1.2 Payload
| `E_D` | d |
";
        let rows: Vec<_> = catalog_rows(text, "Diagnostic codes")
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(rows, ["E_A", "W_B"]);
    }
}
