//! `unsafe_code` is lifted only inside the modules `ALLOWED` lists.
//!
//! Everywhere else the workspace forbids it and rustc enforces that. The `manifest` rule's
//! exempt crate only denies it, so there an `allow`, `expect` or `warn` attribute — bare or
//! inside `cfg_attr` — could lift it anywhere; this rule keeps the lift to the listed modules,
//! and fails on a listed module that no longer lifts it so no stale exemption survives.

use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

use super::manifest::UNSAFE_EXEMPT;
use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "unsafe-code",
    summary: "unsafe_code is lifted only inside the tree-sitter binding's ffi module",
    check,
};

/// `(file, module)` pairs whose module body may lift `unsafe_code`.
const ALLOWED: &[(&str, &str)] = &[("crates/cairn-lang-tree-sitter/bindings/rust/lib.rs", "ffi")];

/// Whether `tokens` hold `allow(…unsafe_code…)`, `expect(…)` or `warn(…)` at any depth, as a
/// `cfg_attr` body does.
fn lowers_unsafe_code(tokens: TokenStream) -> bool {
    let mut lowering = false;
    for token in tokens {
        match token {
            TokenTree::Ident(ident) => {
                lowering = ident == "allow" || ident == "expect" || ident == "warn";
            }
            TokenTree::Group(group) => {
                let inner = group.stream();
                if lowering && names_unsafe_code(inner.clone()) || lowers_unsafe_code(inner) {
                    return true;
                }
                lowering = false;
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => lowering = false,
        }
    }
    false
}

fn names_unsafe_code(tokens: TokenStream) -> bool {
    tokens.into_iter().any(|token| match token {
        TokenTree::Ident(ident) => ident == "unsafe_code",
        TokenTree::Group(group) => names_unsafe_code(group.stream()),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

/// Attributes that lift `unsafe_code`, each with its line and the module path it sits in.
#[derive(Default)]
struct Lifts {
    modules: Vec<String>,
    found: Vec<(usize, Vec<String>)>,
}

impl<'ast> Visit<'ast> for Lifts {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        self.modules.push(item.ident.to_string());
        syn::visit::visit_item_mod(self, item);
        self.modules.pop();
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        let syn::Meta::List(list) = &attribute.meta else {
            return;
        };
        let lifts = if ["allow", "expect", "warn"]
            .iter()
            .any(|level| list.path.is_ident(level))
        {
            names_unsafe_code(list.tokens.clone())
        } else {
            list.path.is_ident("cfg_attr") && lowers_unsafe_code(list.tokens.clone())
        };
        if lifts {
            let line = attribute.pound_token.span.start().line;
            self.found.push((line, self.modules.clone()));
        }
    }
}

fn check(repo: &Repo) -> Vec<Finding> {
    let members = match repo.members() {
        Ok(members) => members,
        Err(error) => return vec![Finding::new(RULE.name, "Cargo.toml", None, error)],
    };
    let Some(exempt) = members.iter().find(|member| member.name == UNSAFE_EXEMPT) else {
        return vec![Finding::new(
            RULE.name,
            "Cargo.toml",
            None,
            format!("{UNSAFE_EXEMPT} is not a workspace member; update `UNSAFE_EXEMPT`"),
        )];
    };

    let mut findings = Vec::new();
    let mut used = vec![false; ALLOWED.len()];
    let sources: Vec<PathBuf> = repo.files_in(&exempt.dir, &["rs"]).cloned().collect();
    for path in &sources {
        let source = match repo.read(path) {
            Ok(source) => source,
            Err(error) => {
                findings.push(Finding::new(RULE.name, path, None, error));
                continue;
            }
        };
        let file = match syn::parse_file(&source) {
            Ok(file) => file,
            Err(error) => {
                let line = error.span().start().line;
                findings.push(Finding::new(RULE.name, path, Some(line), error.to_string()));
                continue;
            }
        };
        let mut lifts = Lifts::default();
        lifts.visit_file(&file);
        for (line, modules) in lifts.found {
            let allowed = ALLOWED.iter().position(|(allowed_file, module)| {
                Path::new(allowed_file) == path.as_path()
                    && modules.first().map(String::as_str) == Some(*module)
            });
            match allowed {
                Some(index) => used[index] = true,
                None => findings.push(Finding::new(
                    RULE.name,
                    path,
                    Some(line),
                    "lifts `unsafe_code` outside the modules listed in xtask's `ALLOWED`; keep \
                     unsafe code in the FFI module, or list a new module with its invariants",
                )),
            }
        }
    }
    for ((file, module), used) in ALLOWED.iter().zip(used) {
        if !used {
            findings.push(Finding::new(
                RULE.name,
                *file,
                None,
                format!("module `{module}` is listed in `ALLOWED` but lifts no `unsafe_code`"),
            ));
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lifts(source: &str) -> Vec<(usize, Vec<String>)> {
        let mut lifts = Lifts::default();
        lifts.visit_file(&syn::parse_file(source).unwrap());
        lifts.found
    }

    #[test]
    fn every_lowering_attribute_is_found_with_its_module() {
        let source = "\
#![allow(unsafe_code)]
mod ffi {
    #![expect(unsafe_code, reason = \"ffi\")]
    #[cfg_attr(test, warn(unsafe_code))]
    fn f() {}
}
#[allow(dead_code, unsafe_code)]
fn g() {}
";
        assert_eq!(
            lifts(source),
            [
                (1, vec![]),
                (3, vec!["ffi".to_owned()]),
                (4, vec!["ffi".to_owned()]),
                (7, vec![]),
            ]
        );
    }

    #[test]
    fn comments_strings_and_other_lints_are_not_lifts() {
        let source = "\
// #[allow(unsafe_code)]
#[deny(unsafe_code)]
#[allow(dead_code)]
#[doc = \"allow(unsafe_code)\"]
fn f() { let _ = \"#[allow(unsafe_code)]\"; }
";
        assert!(lifts(source).is_empty());
    }
}
