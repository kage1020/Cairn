//! Every code core's `DiagnosticCode` spells, and every code a CLI string literal prints itself,
//! has a row of its own in the spec's catalog, in each language: a code is what a reader of the
//! output, or a consumer of `--format json`, branches on. A code no literal spells out, such as
//! one interpolated from a `const`, is not seen.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use syn::visit::Visit;

use super::{Finding, Rule};
use crate::markdown::section;
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "diagnostic-catalog",
    summary: "every code core's DiagnosticCode spells, or a CLI string literal prints itself, has a \
              row in spec/lint's catalog, in each language",
    check,
};

const CORE_SRC: &str = "crates/cairn-lang-core/src";

/// The CLI prints the codes of the checks it runs itself, outside core's enum.
const CLI_SRC: &str = "crates/cairn-lang-cli/src";

/// A code as the CLI prints one: `error[E_X]`, `warning[W_X]`, or `W_X:` opening a line.
static PRINTED_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)(?:^\s*|error\[|warning\[)([EW]_[A-Z0-9_]+)(?:\]|:)").expect("a valid pattern")
});

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

/// Codes printed by string literals outside `#[cfg(test)]` items.
#[derive(Default)]
struct Printed {
    found: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for Printed {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            _ => return syn::visit::visit_item(self, item),
        };
        if !attrs.iter().any(is_cfg_test) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        let value = literal.value();
        self.found.extend(
            PRINTED_CODE
                .captures_iter(&value)
                .map(|captures| captures[1].to_owned()),
        );
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        match mac.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        ) {
            Ok(exprs) => {
                for expr in exprs {
                    self.visit_expr(&expr);
                }
            }
            Err(_) => self.visit_tokens(mac.tokens.clone()),
        }
    }
}

impl Printed {
    /// Reads every string literal in a macro body that is not a list of expressions.
    fn visit_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        for tree in tokens {
            match tree {
                proc_macro2::TokenTree::Group(group) => self.visit_tokens(group.stream()),
                proc_macro2::TokenTree::Literal(literal) => {
                    if let Ok(literal) = syn::parse_str::<syn::LitStr>(&literal.to_string()) {
                        self.visit_lit_str(&literal);
                    }
                }
                proc_macro2::TokenTree::Ident(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
}

fn is_cfg_test(attr: &syn::Attribute) -> bool {
    attr.path().is_ident("cfg")
        && attr
            .parse_args::<syn::Ident>()
            .is_ok_and(|ident| ident == "test")
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

/// Feeds each parsed `.rs` file under `dir` to `visit`, skipping test modules kept in files of
/// their own when `skip_tests` is set.
fn visit_sources(
    repo: &Repo,
    dir: &str,
    skip_tests: bool,
    findings: &mut Vec<Finding>,
    mut visit: impl FnMut(&syn::File),
) {
    for path in repo.files_in(Path::new(dir), &["rs"]) {
        if skip_tests
            && path
                .iter()
                .any(|part| part == "tests" || part == "tests.rs")
        {
            continue;
        }
        let source = match repo.read(path) {
            Ok(source) => source,
            Err(error) => {
                findings.push(Finding::new(path, None, error));
                continue;
            }
        };
        match syn::parse_file(&source) {
            Ok(file) => visit(&file),
            Err(error) => findings.push(Finding::new(
                path,
                Some(error.span().start().line),
                error.to_string(),
            )),
        }
    }
}

fn check(repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut codes = Codes::default();
    visit_sources(repo, CORE_SRC, false, &mut findings, |file| {
        codes.visit_file(file);
    });
    if codes.found.is_empty() {
        findings.push(Finding::new(
            CORE_SRC,
            None,
            "found no `impl DiagnosticCode { fn as_str }` returning code strings",
        ));
        return findings;
    }
    let mut printed = Printed::default();
    visit_sources(repo, CLI_SRC, true, &mut findings, |file| {
        printed.visit_file(file);
    });
    if printed.found.is_empty() {
        findings.push(Finding::new(
            CLI_SRC,
            None,
            "found no string literal printing a code as `error[E_…]`, `warning[W_…]` or `W_…:`",
        ));
    }
    let mut codes = codes.found;
    codes.append(&mut printed.found);

    for (dir, title) in CATALOGS {
        let path = Path::new(dir).join(LINT_CHAPTER);
        let path = path.as_path();
        let rows = match repo.read(path) {
            Ok(text) => catalog_rows(&text, title),
            Err(error) => {
                findings.push(Finding::new(path, None, error));
                continue;
            }
        };
        let Some(rows) = rows.filter(|rows| !rows.is_empty()) else {
            findings.push(Finding::new(
                path,
                None,
                format!("no code rows under a `## … {title}` heading"),
            ));
            continue;
        };
        for code in codes.difference(&rows) {
            findings.push(Finding::new(
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
    fn printed_codes_are_read_from_literals_and_macros_outside_tests() {
        let source = r#"
fn warn() { eprintln!("W_SEEN: {} now", x); }
fn fail() -> String { format!("error[E_SEEN]: {}", y) }
const NOTE: &str = "warning[W_ALSO]: z";
const HELP: &str = "fires `E_ONLY_NAMED` when";
const LATER: &str = "first line\nW_ON_A_LATER_LINE: z";
const INDENTED: &str = "  W_INDENTED: z";
macro_rules! warn { ($x:expr) => { eprintln!("W_IN_MACRO_RULES: {}", $x) }; }
fn repeated() -> Vec<&'static str> { vec!["error[E_REPEATED]: z"; 2] }
#[cfg(test)]
mod tests { fn t() { assert!(s.contains("error[E_IN_TEST]")); } }
"#;
        let mut printed = Printed::default();
        printed.visit_file(&syn::parse_file(source).unwrap());
        assert_eq!(
            printed.found.into_iter().collect::<Vec<_>>(),
            [
                "E_REPEATED",
                "E_SEEN",
                "W_ALSO",
                "W_INDENTED",
                "W_IN_MACRO_RULES",
                "W_ON_A_LATER_LINE",
                "W_SEEN"
            ]
        );
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
