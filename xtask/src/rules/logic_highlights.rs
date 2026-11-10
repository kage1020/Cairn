//! The tree-sitter highlights capture each primitive core's logic library holds, an infix or prefix
//! one as an operator and a called or applied one as a builtin: the grammar reads the words as
//! plain literals, so a primitive the library gains is otherwise left unhighlighted.

use std::collections::BTreeMap;
use std::path::Path;

use syn::visit::Visit;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "logic-highlights",
    summary: "the tree-sitter highlights capture exactly core's logic primitives, each by its notation",
    check,
};

const LIBRARY: &str = "crates/cairn-lang-core/src/logic_library.rs";
const HIGHLIGHTS: &str = "crates/cairn-lang-tree-sitter/queries/highlights.scm";

const OPERATOR: &str = "keyword.operator";
const BUILTIN: &str = "function.builtin";

/// What `impl LogicPrimitive` says of each variant: its word, and its notation's variant.
#[derive(Default)]
struct Library {
    names: BTreeMap<String, String>,
    notations: BTreeMap<String, String>,
}

impl<'ast> Visit<'ast> for Library {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let is_primitive = matches!(&*item.self_ty, syn::Type::Path(path)
            if path.path.segments.last().is_some_and(|last| last.ident == "LogicPrimitive"));
        if !is_primitive {
            return;
        }
        for impl_item in &item.items {
            let syn::ImplItem::Fn(function) = impl_item else {
                continue;
            };
            let into = match function.sig.ident.to_string().as_str() {
                "name" => &mut self.names,
                "notation" => &mut self.notations,
                _ => continue,
            };
            for arm in match_arms(&function.block) {
                let Some(value) = arm_value(&arm.body) else {
                    continue;
                };
                for variant in variants(&arm.pat) {
                    into.insert(variant, value.clone());
                }
            }
        }
    }
}

/// The arms of the `match` a function body ends with.
fn match_arms(block: &syn::Block) -> Vec<syn::Arm> {
    match block.stmts.last() {
        Some(syn::Stmt::Expr(syn::Expr::Match(found), None)) => found.arms.clone(),
        _ => Vec::new(),
    }
}

/// The `Self::X` variants an arm's pattern names.
fn variants(pattern: &syn::Pat) -> Vec<String> {
    match pattern {
        syn::Pat::Or(or) => or.cases.iter().flat_map(variants).collect(),
        syn::Pat::Path(path) => path
            .path
            .segments
            .last()
            .map(|last| vec![last.ident.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// A string literal an arm returns, or the variant of the `Notation` it builds.
fn arm_value(body: &syn::Expr) -> Option<String> {
    let path = match body {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(literal),
            ..
        }) => return Some(literal.value()),
        syn::Expr::Path(path) => &path.path,
        syn::Expr::Struct(built) => &built.path,
        syn::Expr::Call(call) => match &*call.func {
            syn::Expr::Path(path) => &path.path,
            _ => return None,
        },
        _ => return None,
    };
    path.segments.last().map(|last| last.ident.to_string())
}

/// Each primitive's word, with the capture its notation takes.
fn expected(source: &str) -> Result<BTreeMap<String, &'static str>, String> {
    let file = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut library = Library::default();
    library.visit_file(&file);
    if library.names.is_empty() {
        return Err("found no `LogicPrimitive::name` arms".to_owned());
    }
    library
        .names
        .iter()
        .map(|(variant, word)| {
            let capture = match library.notations.get(variant).map(String::as_str) {
                Some("Infix" | "Prefix") => OPERATOR,
                Some("Call" | "Apply") => BUILTIN,
                Some(other) => {
                    return Err(format!(
                        "`LogicPrimitive::{variant}` has notation `{other}`, which this rule \
                         does not map to a capture"
                    ));
                }
                None => {
                    return Err(format!(
                        "found no `LogicPrimitive::notation` arm for `{variant}`"
                    ));
                }
            };
            Ok((word.clone(), capture))
        })
        .collect()
}

/// Each `"word" @capture` line of the query, with its line number.
fn captured(query: &str) -> Vec<(usize, String, String)> {
    query
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let (word, capture) = line.trim().split_once(' ')?;
            let word = word.strip_prefix('"')?.strip_suffix('"')?;
            let capture = capture.trim().strip_prefix('@')?;
            Some((index + 1, word.to_owned(), capture.to_owned()))
        })
        .collect()
}

fn check(repo: &Repo) -> Vec<Finding> {
    let library_path = Path::new(LIBRARY);
    let highlights_path = Path::new(HIGHLIGHTS);
    let expected = match repo.read(library_path).and_then(|text| expected(&text)) {
        Ok(expected) => expected,
        Err(error) => return vec![Finding::new(library_path, None, error)],
    };
    let query = match repo.read(highlights_path) {
        Ok(query) => query,
        Err(error) => return vec![Finding::new(highlights_path, None, error)],
    };
    let captured = captured(&query);
    let mut findings = Vec::new();
    for (word, capture) in &expected {
        let lines: Vec<_> = captured.iter().filter(|(_, w, _)| w == word).collect();
        if lines.is_empty() {
            findings.push(Finding::new(
                highlights_path,
                None,
                format!("captures no `\"{word}\"`, which core's logic library holds: add `\"{word}\" @{capture}`"),
            ));
        }
        for (line, _, found) in lines {
            if found != capture {
                findings.push(Finding::new(
                    highlights_path,
                    Some(*line),
                    format!(
                        "captures `\"{word}\"` as `@{found}`, but its notation in core's logic \
                         library takes `@{capture}`"
                    ),
                ));
            }
        }
    }
    for (line, word, capture) in &captured {
        if capture == BUILTIN && !expected.contains_key(word) {
            findings.push(Finding::new(
                highlights_path,
                Some(*line),
                format!(
                    "captures `\"{word}\"` as `@{BUILTIN}`, but core's logic library holds no \
                     primitive `{word}`: remove the line, or add the primitive to the library"
                ),
            ));
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY_SOURCE: &str = r#"
impl LogicPrimitive {
    pub const fn name(self) -> &'static str {
        match self { Self::And => "and", Self::Not => "not", Self::Mux => "mux", Self::Delay => "delay" }
    }
    pub const fn notation(self) -> Notation {
        match self {
            Self::And => Notation::Infix { rank: 2 },
            Self::Not => Notation::Prefix,
            Self::Mux => Notation::Call(CallSignature { callee: self.name(), operands: &["a"] }),
            Self::Delay => Notation::Apply { counted: true },
        }
    }
    pub const fn other(self) -> &'static str { match self { Self::And => "nope" } }
}
impl Other { pub const fn name(self) -> &'static str { match self { Self::X => "nor_this" } } }
"#;

    #[test]
    fn each_primitive_takes_the_capture_its_notation_names() {
        let found: Vec<_> = expected(LIBRARY_SOURCE).unwrap().into_iter().collect();
        assert_eq!(
            found,
            [
                ("and".to_owned(), OPERATOR),
                ("delay".to_owned(), BUILTIN),
                ("mux".to_owned(), BUILTIN),
                ("not".to_owned(), OPERATOR),
            ]
        );
    }

    #[test]
    fn a_primitive_with_no_notation_arm_is_refused() {
        let source = r#"
impl LogicPrimitive {
    pub const fn name(self) -> &'static str { match self { Self::Xor => "xor" } }
    pub const fn notation(self) -> Notation { match self { Self::And => Notation::Prefix } }
}
"#;
        assert_eq!(
            expected(source).unwrap_err(),
            "found no `LogicPrimitive::notation` arm for `Xor`"
        );
    }

    #[test]
    fn only_single_quoted_word_captures_are_read() {
        let query = "; comment\n\"and\" @keyword.operator\n[\"theme\" \"struct\"] @keyword\n  \"mux\" @function.builtin\n(directive_name) @keyword.directive\n";
        assert_eq!(
            captured(query),
            [
                (2, "and".to_owned(), OPERATOR.to_owned()),
                (4, "mux".to_owned(), BUILTIN.to_owned()),
            ]
        );
    }
}
