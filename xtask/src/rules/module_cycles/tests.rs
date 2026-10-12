use super::*;

fn findings(source: &str) -> Vec<String> {
    let file = syn::parse_file(source).unwrap();
    let mut krate = Crate::default();
    let root = Pending {
        path: Vec::new(),
        file: "lib.rs".into(),
        dir: PathBuf::new(),
    };
    let mut unread = Vec::new();
    krate.collect(&root, &file.items, &[], &mut Vec::new(), &mut unread);
    assert!(unread.is_empty());
    report(&krate, "c")
        .iter()
        .map(|finding| finding.render(RULE.name))
        .collect()
}

const ADVICE: &str = "move what they share down into a module they can all import";

#[test]
fn siblings_that_import_each_other_are_a_cycle() {
    let source = "\
mod a {
    use super::b::B;
    pub struct A;
}
mod b {
    pub struct B;
    fn f() -> super::a::A { todo!() }
}
";
    assert_eq!(
        findings(source),
        [format!(
            "lib.rs:2: module-cycles: `a`, `b` under `c` import each other: `a` → `b` \
             (`super::b::B`, lib.rs:2), `b` → `a` (`super::a::A`, lib.rs:7); {ADVICE}"
        )]
    );
}

#[test]
fn a_longer_loop_is_one_finding() {
    let source = "\
mod a { use crate::b::B; pub struct A; }
mod b { use crate::d::D; pub struct B; }
mod d { use crate::a::A; pub struct D; }
";
    assert_eq!(
        findings(source),
        [format!(
            "lib.rs:1: module-cycles: `a`, `b`, `d` under `c` import each other: `a` → `b` \
             (`crate::b::B`, lib.rs:1), `b` → `d` (`crate::d::D`, lib.rs:2), `d` → `a` \
             (`crate::a::A`, lib.rs:3); {ADVICE}"
        )]
    );
}

#[test]
fn an_import_through_a_re_export_names_the_module_that_defines_it() {
    let source = "\
pub use b::B;
mod a { use crate::B; pub struct A; }
mod b { use super::a::A; pub struct B; }
";
    assert_eq!(
        findings(source),
        [format!(
            "lib.rs:2: module-cycles: `a`, `b` under `c` import each other: `a` → `b` \
             (`crate::B`, lib.rs:2), `b` → `a` (`super::a::A`, lib.rs:3); {ADVICE}"
        )]
    );
}

#[test]
fn a_cycle_is_reported_under_the_parent_the_siblings_share() {
    let source = "\
mod outer {
    mod a { use super::b::B; pub struct A; }
    mod b { use super::a::A; pub struct B; }
}
";
    assert_eq!(
        findings(source),
        [format!(
            "lib.rs:2: module-cycles: `a`, `b` under `c::outer` import each other: `a` → `b` \
             (`super::b::B`, lib.rs:2), `b` → `a` (`super::a::A`, lib.rs:3); {ADVICE}"
        )]
    );
}

#[test]
fn a_parent_and_its_child_may_import_each_other() {
    let source = "\
pub use a::A;
mod a { use super::B; pub struct A; }
pub struct B;
";
    assert_eq!(findings(source), Vec::<String>::new());
}

#[test]
fn one_way_imports_and_test_modules_are_not_cycles() {
    let source = "\
mod a {
    use super::b::B;
    pub struct A;
    #[cfg(test)]
    mod tests { use crate::d::D; }
}
mod b { pub struct B; }
mod d {
    use super::a::A;
    #[cfg(test)]
    use crate::b::B;
    pub struct D;
}
";
    assert_eq!(findings(source), Vec::<String>::new());
}
