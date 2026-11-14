//! The VS Code grammar highlights exactly the keywords core's parser reads: nothing compiles its
//! regexes against the parser.

use std::collections::BTreeSet;
use std::path::Path;

use syn::visit::Visit;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "editor-keywords",
    summary: "the VS Code grammar highlights exactly core's top-level, `edit` and member keywords",
    check,
};

const GRAMMAR: &str = "editors/vscode/syntaxes/cairn.tmLanguage.json";

/// Each grammar pattern, the core source defining its keywords, and which literals there count.
const PAIRS: [(&str, &str, Source); 3] = [
    (
        "top-level",
        "crates/cairn-lang-core/src/ast.rs",
        Source::ItemKind,
    ),
    (
        "edit-line",
        "crates/cairn-lang-core/src/parse/edit.rs",
        Source::EditLine,
    ),
    (
        "member-keyword",
        "crates/cairn-lang-core/src/intent/keyword_table.rs",
        Source::KnownKeywords,
    ),
];

#[derive(Clone, Copy)]
enum Source {
    /// The literals `impl ItemKind { fn keyword }` returns.
    ItemKind,
    /// The literals of `const KNOWN_KEYWORDS`.
    KnownKeywords,
    /// The literals of `const EDIT` and `const SET`.
    EditLine,
}

struct Literals {
    source: Source,
    inside: bool,
    found: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for Literals {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let is_item_kind = matches!(&*item.self_ty, syn::Type::Path(path)
            if path.path.segments.last().is_some_and(|last| last.ident == "ItemKind"));
        if !matches!(self.source, Source::ItemKind) || !is_item_kind {
            return;
        }
        for impl_item in &item.items {
            if let syn::ImplItem::Fn(function) = impl_item
                && function.sig.ident == "keyword"
            {
                self.inside = true;
                self.visit_block(&function.block);
                self.inside = false;
            }
        }
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        let counted = match self.source {
            Source::KnownKeywords => item.ident == "KNOWN_KEYWORDS",
            Source::EditLine => item.ident == "EDIT" || item.ident == "SET",
            Source::ItemKind => false,
        };
        if counted {
            self.inside = true;
            self.visit_expr(&item.expr);
            self.inside = false;
        }
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if self.inside {
            self.found.insert(literal.value());
        }
    }
}

fn core_keywords(source: &str, kind: Source) -> Result<BTreeSet<String>, String> {
    let file = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut literals = Literals {
        source: kind,
        inside: false,
        found: BTreeSet::new(),
    };
    literals.visit_file(&file);
    Ok(literals.found)
}

/// The alternatives of the first capturing group in a `match` such as `^\s*(theme|def)\b`; a
/// `(?` group, such as a lookbehind, is passed over.
fn alternation(pattern: &str) -> Option<BTreeSet<String>> {
    let mut rest = pattern;
    loop {
        (_, rest) = rest.split_once('(')?;
        if !rest.starts_with('?') {
            break;
        }
    }
    let (group, _) = rest.split_once(')')?;
    Some(group.split('|').map(str::to_owned).collect())
}

/// The first group of a pattern's `match`, or of its `begin` and of each `match` nested in it.
fn grammar_keywords(grammar: &str, name: &str) -> Result<BTreeSet<String>, String> {
    let grammar: serde_json::Value =
        serde_json::from_str(grammar).map_err(|error| format!("not JSON: {error}"))?;
    let rule = &grammar["repository"][name];
    let patterns: Vec<&str> = match (rule["match"].as_str(), rule["begin"].as_str()) {
        (Some(pattern), _) => vec![pattern],
        (None, Some(begin)) => std::iter::once(begin)
            .chain(
                rule["patterns"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|nested| nested["match"].as_str()),
            )
            .collect(),
        (None, None) => return Err(format!("no `repository.{name}.match` or `begin` pattern")),
    };
    let mut keywords = BTreeSet::new();
    for pattern in patterns {
        keywords.extend(
            alternation(pattern)
                .ok_or_else(|| format!("`{name}` has no `(a|b|...)` group: {pattern}"))?,
        );
    }
    Ok(keywords)
}

fn check(repo: &Repo) -> Vec<Finding> {
    let grammar_path = Path::new(GRAMMAR);
    let grammar = match repo.read(grammar_path) {
        Ok(grammar) => grammar,
        Err(error) => return vec![Finding::new(grammar_path, None, error)],
    };
    let mut findings = Vec::new();
    for (name, core_path, kind) in PAIRS {
        let core_path = Path::new(core_path);
        let core = match repo
            .read(core_path)
            .and_then(|text| core_keywords(&text, kind))
        {
            Ok(core) if !core.is_empty() => core,
            Ok(_) => {
                findings.push(Finding::new(
                    core_path,
                    None,
                    format!("found no keywords for the grammar's `{name}` pattern"),
                ));
                continue;
            }
            Err(error) => {
                findings.push(Finding::new(core_path, None, error));
                continue;
            }
        };
        let highlighted = match grammar_keywords(&grammar, name) {
            Ok(highlighted) => highlighted,
            Err(error) => {
                findings.push(Finding::new(grammar_path, None, error));
                continue;
            }
        };
        for keyword in core.difference(&highlighted) {
            findings.push(Finding::new(
                grammar_path,
                None,
                format!(
                    "`{name}` does not highlight `{keyword}`, which {} reads",
                    core_path.display()
                ),
            ));
        }
        for keyword in highlighted.difference(&core) {
            findings.push(Finding::new(
                grammar_path,
                None,
                format!(
                    "`{name}` highlights `{keyword}`, which {} does not read",
                    core_path.display()
                ),
            ));
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_keywords_come_from_item_kind_keyword_only() {
        let source = r#"
impl ItemKind {
    pub fn keyword(self) -> &'static str {
        match self { Self::Theme => "theme", Self::Def => "def" }
    }
    pub fn other(self) -> &'static str { "nope" }
}
impl Other { pub fn keyword(self) -> &'static str { "nor_this" } }
const KNOWN_KEYWORDS: &[&str] = &["floor"];
"#;
        let found: Vec<_> = core_keywords(source, Source::ItemKind)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(found, ["def", "theme"]);
    }

    #[test]
    fn known_keywords_come_from_the_const_only() {
        let source = r#"
const KNOWN_KEYWORDS: &[&str] = &["floor", "walls"];
const OTHER: &[&str] = &["nope"];
impl ItemKind { pub fn keyword(self) -> &'static str { "theme" } }
"#;
        let found: Vec<_> = core_keywords(source, Source::KnownKeywords)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(found, ["floor", "walls"]);
    }

    #[test]
    fn edit_keywords_come_from_the_edit_and_set_consts_only() {
        let source = r#"
pub(super) const EDIT: &str = "edit";
const SET: &str = "set";
const OTHER: &str = "nope";
"#;
        let found: Vec<_> = core_keywords(source, Source::EditLine)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(found, ["edit", "set"]);
    }

    #[test]
    fn a_begin_pattern_takes_the_keywords_of_its_nested_matches_too() {
        let grammar = r##"{"repository": {"edit-line": {
            "begin": "^(edit)\\b(?:\\s+([a-z]+))?",
            "patterns": [{"match": "(?<=\\s)(set)(?=\\s)"}, {"include": "#number"}]
        }}}"##;
        let found: Vec<_> = grammar_keywords(grammar, "edit-line")
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(found, ["edit", "set"]);
    }

    #[test]
    fn the_first_group_of_a_pattern_is_its_keywords() {
        let found: Vec<_> = alternation(r"^\s*(theme|def|site)\b(\s+x)?")
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(found, ["def", "site", "theme"]);
    }
}
