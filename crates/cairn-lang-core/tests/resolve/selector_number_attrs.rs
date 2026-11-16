//! A selector attribute whose value is a number selects the members that write the same number,
//! compared by value: `1.5` and `1.50` are one value, and so are `2` and `2.0`.

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::ast::ValueKind;

use crate::common::{code_spans, diagnose, selector_extras};

/// A theme row selecting by `tags=row`, and the one window, which writes `tags=member`.
fn source(row: &str, member: &str) -> String {
    format!(
        "theme t:\n  slot glass -> @glass_pane\n  window[tags={row}] -> frame=@spruce_wood\n\n\
         struct s size=9x7\n  window tags={member} side=front offset=2 y=2 size=2x2 \
         mat_slot=glass\n"
    )
}

#[test]
fn a_number_attribute_matches_a_member_that_writes_the_same_number() {
    for (row, member) in [
        ("1.50", "1.5"),
        ("2", "2.0"),
        ("02.0", "2"),
        ("[1.5, 2]", "[1.50, 2.0]"),
    ] {
        let source = source(row, member);
        let binding = source.find("@spruce_wood").expect("the binding");
        assert_eq!(
            code_spans(&diagnose(&source)),
            [(
                DiagnosticCode::IgnoredArgument.as_str(),
                binding..binding + "@spruce_wood".len(),
            )],
            "[tags={row}] against tags={member}",
        );
        assert_eq!(
            selector_extras(&source),
            [(
                "frame".to_owned(),
                ValueKind::Token("spruce_wood".to_owned())
            )],
            "[tags={row}] against tags={member}",
        );
    }
}

#[test]
fn a_number_attribute_does_not_match_another_number() {
    for (row, member) in [
        ("1.5", "1.05"),
        ("2", "2.5"),
        ("1.5", "15"),
        ("[1.5]", "[1.5, 0]"),
    ] {
        let source = source(row, member);
        let row_start = source.find("window[").expect("the row");
        let row_end = source[row_start..].find('\n').expect("the row's end") + row_start;
        let binding = source.find("@spruce_wood").expect("the binding");
        assert_eq!(
            code_spans(&diagnose(&source)),
            [
                (
                    DiagnosticCode::ThemeSelectorUnmatched.as_str(),
                    row_start..row_end,
                ),
                (
                    DiagnosticCode::IgnoredArgument.as_str(),
                    binding..binding + "@spruce_wood".len(),
                ),
            ],
            "[tags={row}] against tags={member}",
        );
    }
}

#[test]
fn two_rows_whose_numbers_are_one_value_select_the_same_members() {
    let source = "theme t:\n  slot wall -> @stone\n  walls[height=4] -> trim=@stone\n  \
                  walls[height=4.0] -> trim=@oak\n\nstruct s size=5x5\n  \
                  walls height=4 mat_slot=wall\n";
    let found: Vec<(&str, &str)> = diagnose(source)
        .iter()
        .map(|d| (d.code.as_str(), &source[d.span.clone()]))
        .collect();
    assert_eq!(
        found,
        [
            (DiagnosticCode::IgnoredArgument.as_str(), "@stone"),
            (
                DiagnosticCode::DuplicateSelector.as_str(),
                "walls[height=4.0] -> trim=@oak",
            ),
            (DiagnosticCode::IgnoredArgument.as_str(), "@oak"),
        ],
    );
}
