//! An `@edition` guard stands in a body and holds the lines indented under it
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch"); only the parser
//! refuses a malformed one.

use cairn_lang_core::Edition;
use cairn_lang_core::ast::{Item, Statement};
use cairn_lang_core::parse;

/// A guard's edition, the text of its own line, and the keywords of the members it holds.
type Guard<'a> = (Edition, &'a str, Vec<&'a str>);

/// Each guard among `body`'s lines, as a [`Guard`].
fn guards<'a>(source: &'a str, body: &'a [Statement]) -> Vec<Guard<'a>> {
    body.iter()
        .filter_map(|line| match line {
            Statement::EditionGuard {
                edition,
                children,
                span,
            } => Some((
                *edition,
                &source[span.clone()],
                children
                    .iter()
                    .map(|child| match child {
                        Statement::Generic { keyword, .. } => keyword.as_str(),
                        other => panic!("expected a member line, got {other:?}"),
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

/// The body of `source`'s item `n`.
fn body_of(source: &str, n: usize) -> Vec<Statement> {
    let module = parse(source).unwrap_or_else(|err| panic!("{source:?}: {err}"));
    match &module.items[n] {
        Item::Struct { body, .. } | Item::Def { body, .. } | Item::Site { body, .. } => {
            body.clone()
        }
        other => panic!("expected a body, got {other:?}"),
    }
}

/// The message `parse` refuses `source` with, and the 1-based `(line, column)` it points at.
fn refusal(source: &str) -> (String, (u32, u32)) {
    let err = parse(source).expect_err("the source is malformed");
    let at = err.position();
    (err.user_message(), (at.line.get(), at.col.get()))
}

#[test]
fn a_guard_holds_the_lines_indented_under_it_and_names_its_edition() {
    let source = "struct s size=5x5\n  floor mat_slot=f\n  @edition java\n    \
                  raw_block mat=minecraft:light[level=15] at=2,1,2\n    \
                  raw_fill mat=minecraft:glass from=0,0,0 to=1,0,1\n  @edition  bedrock\n    \
                  raw_block mat=minecraft:light_block_15 at=2,1,2\n  walls mat_slot=w\n";
    let body = body_of(source, 0);
    assert_eq!(
        guards(source, &body),
        [
            (
                Edition::Java,
                "@edition java",
                vec!["raw_block", "raw_fill"]
            ),
            (Edition::Bedrock, "@edition  bedrock", vec!["raw_block"]),
        ],
    );
    let lines: Vec<&str> = body
        .iter()
        .map(|line| &source[line.span().clone()])
        .collect();
    assert_eq!(
        lines,
        [
            "floor mat_slot=f",
            "@edition java",
            "@edition  bedrock",
            "walls mat_slot=w"
        ],
    );
}

#[test]
fn a_guard_stands_in_a_def_body_and_under_a_member() {
    let def = "def d size=3x3\n  @edition bedrock\n    raw_block mat=minecraft:stone at=1,0,1\n";
    assert_eq!(
        guards(def, &body_of(def, 0)),
        [(Edition::Bedrock, "@edition bedrock", vec!["raw_block"])],
    );
    let level = "struct s size=3x3\n  level y=1\n    @edition java\n      \
                 raw_block mat=minecraft:stone at=1,0,1\n";
    let body = body_of(level, 0);
    let Statement::Generic { children, .. } = &body[0] else {
        panic!("expected the level, got {:?}", body[0]);
    };
    assert_eq!(
        guards(level, children),
        [(Edition::Java, "@edition java", vec!["raw_block"])],
    );
}

#[test]
fn a_guard_parses_whatever_lines_it_holds_and_leaves_them_to_the_check() {
    let source = "struct s size=3x3\n  @edition java\n    walls mat_slot=w\n    \
                  level y=1\n      @edition bedrock\n        raw_block mat=minecraft:stone \
                  at=1,0,1\n    logic sig.a = sig.b\n";
    let body = body_of(source, 0);
    let [
        Statement::EditionGuard {
            edition: Edition::Java,
            children,
            ..
        },
    ] = body.as_slice()
    else {
        panic!("expected one java guard, got {body:?}");
    };
    let kinds: Vec<&str> = children
        .iter()
        .map(|line| match line {
            Statement::Generic { keyword, .. } => keyword.as_str(),
            Statement::Logic { .. } => "logic",
            other => panic!("unexpected line {other:?}"),
        })
        .collect();
    assert_eq!(kinds, ["walls", "level", "logic"]);
}

#[test]
fn a_malformed_guard_is_refused_at_what_is_wrong_with_it() {
    let body = |guard: &str| format!("struct s size=3x3\n  {guard}\n");
    let cases: [(String, &str, (u32, u32)); 8] = [
        (
            body("@edition pocket\n    raw_block mat=minecraft:stone at=1,0,1"),
            "`pocket` is not an edition: an `@edition` guard names `java` or `bedrock`",
            (2, 12),
        ),
        (
            body("@edition\n    raw_block mat=minecraft:stone at=1,0,1"),
            "an `@edition` guard names the edition whose build keeps its lines: `@edition java` \
             or `@edition bedrock`",
            (2, 11),
        ),
        (
            body("@edition java { raw_block mat=minecraft:stone at=1,0,1 }"),
            "an `@edition` guard holds its lines indented under it, not in braces",
            (2, 17),
        ),
        (
            body("@edition java raw_block\n    raw_block mat=minecraft:stone at=1,0,1"),
            "an `@edition` guard takes nothing after its edition: the lines it guards go on the \
             lines below it, one indent deeper",
            (2, 17),
        ),
        (
            body("@edition java\n  raw_block mat=minecraft:stone at=1,0,1"),
            "nothing is indented under this `@edition java` guard: the lines it guards go on the \
             lines below it, one indent deeper",
            (2, 3),
        ),
        (
            body(
                "@edition java\n    @edition bedrock\n      raw_block mat=minecraft:stone at=1,0,1",
            ),
            "an `@edition` guard does not nest: the guard this one stands in already picks the \
             edition, so write its lines there or dedent this guard out",
            (3, 5),
        ),
        (
            body("@requires version>=1.21"),
            "`@requires` is a file directive, which goes at the top of the file: inside a body, \
             `@` opens only an `@edition` guard",
            (2, 3),
        ),
        (
            body("@foo java"),
            "unknown directive `@foo`: inside a body, `@` opens only an `@edition` guard",
            (2, 3),
        ),
    ];
    for (source, message, at) in cases {
        assert_eq!(refusal(&source), (message.to_owned(), at), "{source:?}");
    }
}

#[test]
fn a_floor_under_a_guard_is_refused_for_the_guard() {
    let guarded = |item: &str| {
        format!(
            "{item}\n  @edition java\n    requires version>=1.21\n    \
             raw_block mat=minecraft:stone at=1,0,1\n"
        )
    };
    assert_eq!(
        refusal(&guarded("def d")),
        (
            "an `@edition` guard may not hold `requires version>=X`: the floor belongs to the \
             whole part on every edition, so it goes at the `def` body's own level"
                .to_owned(),
            (3, 5),
        ),
    );
    assert_eq!(
        refusal(&guarded("struct s size=3x3")),
        (
            "an `@edition` guard may not hold `requires version>=X`, and neither may the \
             `struct` or `site` it is in: the floor is the whole file's and is written \
             `@requires version>=X` at the top of the file"
                .to_owned(),
            (3, 5),
        ),
    );
    assert_eq!(
        refusal("def d\n  @requires version>=1.21\n  raw_block mat=minecraft:stone at=1,0,1\n"),
        (
            "a `def` or `theme` body's floor is written `requires version>=X`, without the `@`: \
             inside a body, `@` opens only an `@edition` guard"
                .to_owned(),
            (2, 3),
        ),
    );
}

#[test]
fn a_guard_in_a_theme_body_is_refused_for_a_theme_named_for_its_edition() {
    let in_theme =
        |line: &str| format!("theme t\n  slot f -> @stone\n  {line}\n    slot g -> @dirt\n");
    assert_eq!(
        refusal(&in_theme("@edition java")),
        (
            "an `@edition` guard stands inside a `struct`, `def` or `site` body, never in a \
             `theme`: a theme is picked per edition by its name, so write this edition's slots in \
             a `theme NAME_java` or a `theme NAME_bedrock`"
                .to_owned(),
            (3, 3),
        ),
    );
    assert_eq!(
        refusal(&in_theme("@intended_targets java")),
        (
            "`@intended_targets` is a file directive, which goes at the top of the file: inside a \
             body, `@` opens only an `@edition` guard"
                .to_owned(),
            (3, 3),
        ),
    );
}

#[test]
fn a_guard_outside_every_body_is_refused() {
    let outside = "an `@edition` guard stands inside a `struct`, `def` or `site` body, around \
                   the raw members it guards: the items of a file are edition-neutral, so move \
                   the guard into the body that places those blocks";
    let at_the_top = "@cairn 2026.11\n@edition java\nstruct s size=3x3\n  floor mat_slot=f\n";
    assert_eq!(refusal(at_the_top), (outside.to_owned(), (2, 1)));
    let between_items = "struct s size=3x3\n  floor mat_slot=f\n@edition java\n  \
                         raw_block mat=minecraft:stone at=1,0,1\n";
    assert_eq!(refusal(between_items), (outside.to_owned(), (3, 1)));
    let late_directive = "struct s size=3x3\n  floor mat_slot=f\n@requires version>=1.21\n";
    assert_eq!(
        refusal(late_directive),
        (
            "a file directive goes at the top of the file, before its first item".to_owned(),
            (3, 1)
        ),
    );
}
