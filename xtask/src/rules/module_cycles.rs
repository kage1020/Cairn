//! The modules under one parent import each other in one direction only, so any of them can be
//! read without the ones that import it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use syn::visit::Visit;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "module-cycles",
    summary: "within a crate, the modules under one parent import each other in one direction only",
    check,
};

/// A module's path from its crate root, empty for the root.
type ModPath = Vec<String>;

/// A path that names another module, as written.
struct Reference {
    segments: Vec<String>,
    line: usize,
}

#[derive(Default)]
struct Module {
    file: PathBuf,
    children: BTreeSet<String>,
    items: BTreeSet<String>,
    /// Each name a `use` binds, with the path it binds.
    imports: BTreeMap<String, Vec<String>>,
    globs: Vec<Vec<String>>,
    references: Vec<Reference>,
}

/// Where a reference is written: its file, line and path.
type Site = (PathBuf, usize, String);

#[derive(Default)]
struct Crate {
    modules: BTreeMap<ModPath, Module>,
}

/// A file module still to read, and the directory its own `mod` items resolve against.
struct Pending {
    path: ModPath,
    file: PathBuf,
    dir: PathBuf,
}

fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && matches!(&attr.meta, syn::Meta::List(list) if list.tokens.to_string() == "test")
    })
}

fn attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn item_name(item: &syn::Item) -> Option<String> {
    let ident = match item {
        syn::Item::Const(item) => &item.ident,
        syn::Item::Enum(item) => &item.ident,
        syn::Item::Fn(item) => &item.sig.ident,
        syn::Item::Macro(item) => item.ident.as_ref()?,
        syn::Item::Static(item) => &item.ident,
        syn::Item::Struct(item) => &item.ident,
        syn::Item::Trait(item) => &item.ident,
        syn::Item::TraitAlias(item) => &item.ident,
        syn::Item::Type(item) => &item.ident,
        syn::Item::Union(item) => &item.ident,
        _ => return None,
    };
    Some(ident.to_string())
}

/// The `crate::`, `super::` and `self::` paths outside `use` items.
#[derive(Default)]
struct Paths(Vec<Reference>);

impl<'ast> Visit<'ast> for Paths {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path.leading_colon.is_none()
            && path.segments.len() > 1
            && path
                .segments
                .first()
                .is_some_and(|first| ["crate", "super", "self"].iter().any(|k| first.ident == k))
        {
            self.0.push(Reference {
                segments: path.segments.iter().map(|s| s.ident.to_string()).collect(),
                line: path
                    .segments
                    .first()
                    .map_or(0, |first| first.ident.span().start().line),
            });
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut module = Module::default();
        module.add_use(&item.tree, &mut Vec::new());
        self.0.extend(module.references);
    }
}

impl Module {
    fn add_use(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.add_use(&path.tree, prefix);
                prefix.pop();
            }
            syn::UseTree::Name(name) => self.bind(prefix, &name.ident, None),
            syn::UseTree::Rename(rename) => self.bind(prefix, &rename.ident, Some(&rename.rename)),
            syn::UseTree::Glob(glob) => {
                self.globs.push(prefix.clone());
                self.references.push(Reference {
                    segments: prefix.clone(),
                    line: glob.star_token.span.start().line,
                });
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.add_use(tree, prefix);
                }
            }
        }
    }

    fn bind(&mut self, prefix: &[String], ident: &syn::Ident, rename: Option<&syn::Ident>) {
        let mut segments = prefix.to_vec();
        if ident != "self" {
            segments.push(ident.to_string());
        }
        let name = rename.map_or_else(|| segments.last().cloned(), |r| Some(r.to_string()));
        if let Some(name) = name
            && name != "_"
        {
            self.imports.insert(name, segments.clone());
        }
        self.references.push(Reference {
            segments,
            line: ident.span().start().line,
        });
    }
}

impl Crate {
    /// The crate whose root is `root`, read through its `mod` items.
    fn load(repo: &Repo, root: &Path, findings: &mut Vec<Finding>) -> Self {
        let mut krate = Self::default();
        let mut pending = vec![Pending {
            path: Vec::new(),
            file: root.to_path_buf(),
            dir: root.parent().unwrap_or(Path::new("")).to_path_buf(),
        }];
        while let Some(next) = pending.pop() {
            let source = match repo.read(&next.file) {
                Ok(source) => source,
                Err(error) => {
                    findings.push(Finding::new(&next.file, None, error));
                    continue;
                }
            };
            match syn::parse_file(&source) {
                Ok(file) => krate.collect(&next, &file.items, repo.files(), &mut pending, findings),
                Err(error) => findings.push(Finding::new(
                    &next.file,
                    Some(error.span().start().line),
                    error.to_string(),
                )),
            }
        }
        krate
    }

    fn collect(
        &mut self,
        at: &Pending,
        items: &[syn::Item],
        files: &[PathBuf],
        pending: &mut Vec<Pending>,
        findings: &mut Vec<Finding>,
    ) {
        let mut module = Module {
            file: at.file.clone(),
            ..Module::default()
        };
        for item in items {
            if is_test_only(attrs(item)) {
                continue;
            }
            match item {
                syn::Item::Mod(item) => {
                    let name = item.ident.to_string();
                    module.children.insert(name.clone());
                    let mut path = at.path.clone();
                    path.push(name.clone());
                    let dir = at.dir.join(&name);
                    if let Some((_, items)) = &item.content {
                        let inline = Pending {
                            path,
                            file: at.file.clone(),
                            dir,
                        };
                        self.collect(&inline, items, files, pending, findings);
                        continue;
                    }
                    let flat = at.dir.join(format!("{name}.rs"));
                    let nested = dir.join("mod.rs");
                    let Some(file) = [flat, nested]
                        .into_iter()
                        .find(|file| files.binary_search(file).is_ok())
                    else {
                        findings.push(Finding::new(
                            &at.file,
                            Some(item.ident.span().start().line),
                            format!("no file for `mod {name}` under {}", at.dir.display()),
                        ));
                        continue;
                    };
                    pending.push(Pending { path, file, dir });
                }
                syn::Item::Use(item) => module.add_use(&item.tree, &mut Vec::new()),
                other => {
                    module.items.extend(item_name(other));
                    let mut paths = Paths::default();
                    paths.visit_item(other);
                    module.references.extend(paths.0);
                }
            }
        }
        self.modules.insert(at.path.clone(), module);
    }

    /// The module `segments`, written in `from`, names or names an item of.
    fn resolve(&self, from: &[String], segments: &[String], depth: usize) -> Option<ModPath> {
        if depth > 16 {
            return None;
        }
        let (first, rest) = segments.split_first()?;
        let mut at = from.to_vec();
        let rest = match first.as_str() {
            "crate" => {
                at.clear();
                rest
            }
            "self" => rest,
            "super" => {
                at.pop()?;
                let supers = rest.iter().take_while(|s| *s == "super").count();
                for _ in 0..supers {
                    at.pop()?;
                }
                &rest[supers..]
            }
            _ => {
                let module = self.modules.get(&at)?;
                if !module.children.contains(first)
                    && !module.items.contains(first)
                    && !module.imports.contains_key(first)
                {
                    return None;
                }
                segments
            }
        };
        for (index, segment) in rest.iter().enumerate() {
            let module = self.modules.get(&at)?;
            if module.children.contains(segment) {
                at.push(segment.clone());
            } else if module.items.contains(segment) {
                return Some(at);
            } else if let Some(target) = module.imports.get(segment) {
                let mut path = target.clone();
                path.extend_from_slice(&rest[index + 1..]);
                return self.resolve(&at, &path, depth + 1);
            } else {
                return module.globs.iter().find_map(|glob| {
                    let mut path = glob.clone();
                    path.extend_from_slice(&rest[index..]);
                    self.resolve(&at, &path, depth + 1)
                });
            }
        }
        Some(at)
    }

    /// For each parent, each import from the subtree of one child into another's, with where the
    /// first one is written.
    fn sibling_edges(&self) -> BTreeMap<ModPath, BTreeMap<(String, String), Site>> {
        let mut edges: BTreeMap<ModPath, BTreeMap<(String, String), Site>> = BTreeMap::new();
        for (from, module) in &self.modules {
            for reference in &module.references {
                let Some(to) = self.resolve(from, &reference.segments, 0) else {
                    continue;
                };
                let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
                let (Some(a), Some(b)) = (from.get(shared), to.get(shared)) else {
                    continue;
                };
                edges
                    .entry(from[..shared].to_vec())
                    .or_default()
                    .entry((a.clone(), b.clone()))
                    .or_insert_with(|| {
                        (
                            module.file.clone(),
                            reference.line,
                            reference.segments.join("::"),
                        )
                    });
            }
        }
        edges
    }
}

/// The shortest path from `start` back to itself, if one exists.
fn shortest_cycle(edges: &BTreeMap<(String, String), Site>, start: &str) -> Option<Vec<String>> {
    let mut previous: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        for (a, b) in edges.keys() {
            if a != node {
                continue;
            }
            if b == start {
                let mut cycle = vec![b.clone(), a.clone()];
                let mut at = a.as_str();
                while at != start {
                    at = previous[at];
                    cycle.push(at.to_owned());
                }
                cycle.reverse();
                return Some(cycle);
            }
            if !previous.contains_key(b.as_str()) {
                previous.insert(b, a);
                queue.push_back(b);
            }
        }
    }
    None
}

/// Each set of siblings that import one another, with the shortest loop through its first.
fn cycles(edges: &BTreeMap<(String, String), Site>) -> Vec<(BTreeSet<&str>, Vec<String>)> {
    let nodes: BTreeSet<&str> = edges
        .keys()
        .flat_map(|(a, b)| [a.as_str(), b.as_str()])
        .collect();
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for &node in &nodes {
        if seen.contains(node) {
            continue;
        }
        let Some(cycle) = shortest_cycle(edges, node) else {
            continue;
        };
        let component: BTreeSet<&str> = nodes
            .iter()
            .copied()
            .filter(|other| reaches(edges, node, other) && reaches(edges, other, node))
            .collect();
        seen.extend(component.iter().copied());
        found.push((component, cycle));
    }
    found
}

fn reaches(edges: &BTreeMap<(String, String), Site>, from: &str, to: &str) -> bool {
    let mut seen = BTreeSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(node) = queue.pop_front() {
        if node == to {
            return true;
        }
        for (a, b) in edges.keys() {
            if a == node && seen.insert(b) {
                queue.push_back(b);
            }
        }
    }
    false
}

fn report(krate: &Crate, name: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (parent, edges) in krate.sibling_edges() {
        for (component, cycle) in cycles(&edges) {
            let steps: Vec<String> = cycle
                .windows(2)
                .map(|pair| {
                    let (file, line, path) = &edges[&(pair[0].clone(), pair[1].clone())];
                    format!(
                        "`{}` → `{}` (`{path}`, {}:{line})",
                        pair[0],
                        pair[1],
                        file.display()
                    )
                })
                .collect();
            let (file, line, _) = &edges[&(cycle[0].clone(), cycle[1].clone())];
            let members: Vec<String> = component.iter().map(|m| format!("`{m}`")).collect();
            findings.push(Finding::new(
                file,
                Some(*line),
                format!(
                    "{} under `{}` import each other: {}; move what they share down into a \
                     module they can all import",
                    members.join(", "),
                    std::iter::once(name)
                        .chain(parent.iter().map(String::as_str))
                        .collect::<Vec<_>>()
                        .join("::"),
                    steps.join(", "),
                ),
            ));
        }
    }
    findings
}

fn check(repo: &Repo) -> Vec<Finding> {
    let members = match repo.members() {
        Ok(members) => members,
        Err(error) => return vec![Finding::new("Cargo.toml", None, error)],
    };
    let mut findings = Vec::new();
    for member in members {
        for root in &member.roots {
            let krate = Crate::load(repo, root, &mut findings);
            findings.extend(report(&krate, &member.name.replace('-', "_")));
        }
    }
    findings
}

#[cfg(test)]
mod tests;
