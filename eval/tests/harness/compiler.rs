//! What the harness makes of `cairn check`, `cairn compile` and `cairn lower` on one program.

use std::collections::BTreeMap;

use eval::compiler::{Cells, Checked, Evaluation};
use eval::record::DiagnosticRecord;

use crate::common::{self, HUT_THEME_ONLY, check_report};

fn evaluated(program: &str) -> Evaluation {
    let dir = tempfile::tempdir().expect("a scratch directory");
    common::compiler()
        .evaluate(program, dir.path())
        .expect("the compiler runs")
}

#[test]
fn a_refusal_of_the_whole_build_is_a_failed_check_with_no_diagnostic() {
    let program = "struct a size=3x3\n";
    let evaluation = evaluated(program);
    let report = check_report(program);
    assert!(
        report.starts_with("error[E_PARTIAL_BUILD]: main.crn: "),
        "{report}"
    );
    assert_eq!(evaluation.diagnostics, []);
    assert_eq!(evaluation.checked, Checked::Check(report));
}

#[test]
fn a_build_of_no_block_is_empty() {
    let evaluation = evaluated(HUT_THEME_ONLY);
    assert_eq!(evaluation.diagnostics, []);
    assert_eq!(evaluation.checked, Checked::Empty);
}

#[test]
fn a_program_check_passes_and_compile_refuses_is_a_failed_compile() {
    let line = "a".repeat(usize::from(u16::MAX) + 1);
    let program = format!(
        "\
theme t:
  slot wall -> @cobblestone
  slot board -> @sign.oak

struct s size=5x5
  walls mat_slot=wall height=3
  sign at=inside.front offset=2 y=1 mat_slot=board back={line:?}
"
    );
    let evaluation = evaluated(&program);
    assert_eq!(evaluation.diagnostics, []);
    assert_eq!(
        evaluation.checked,
        Checked::Compile(format!(
            "error: checking `struct::s`: the sign at (2, 1, 3) has the line `{line}`, which cannot \
             be written as NBT: nbt: string length 65538 exceeds wire limit 65535. Fix: shorten the \
             sign's text\n"
        ))
    );
}

#[test]
fn a_warning_does_not_fail_a_program() {
    let program = "\
theme stone:
  slot floor -> @oak_planks
  slot wall -> @cobblestone

def porch:
  floor mat_slot=floor

struct shed size=5x5
  floor mat_slot=floor
  walls mat_slot=wall height=3
";
    let evaluation = evaluated(program);
    assert_eq!(
        evaluation.diagnostics,
        [DiagnosticRecord {
            code: "W_UNUSED_DEF".to_owned(),
            severity: "warning".to_owned(),
            line: 5,
            col: 1,
        }]
    );
    let Checked::Pass(cells) = evaluation.checked else {
        panic!("the program passes: {:?}", evaluation.checked);
    };
    assert_eq!(cells.len(), 73);
}

#[test]
fn a_bed_and_a_sign_carry_what_cairn_lower_prints_of_them() {
    let program = "\
theme t:
  slot floor -> @oak_planks
  slot wall -> @cobblestone
  slot cot -> @bed.brown
  slot board -> @sign.oak

struct s size=5x5
  floor mat_slot=floor
  walls mat_slot=wall height=3
  bed at=inside.back offset=1 y=1 mat_slot=cot
  sign at=inside.front offset=2 y=1 mat_slot=board text=[\"Hi\"]
";
    let Checked::Pass(Cells(cells)) = evaluated(program).checked else {
        panic!("the program passes");
    };
    let entities: BTreeMap<_, _> = cells
        .into_iter()
        .filter(|(_, cell)| cell.contains('{'))
        .collect();
    let at = |x, y, z| ("struct::s".to_owned(), x, y, z);
    assert_eq!(
        entities,
        BTreeMap::from([
            (
                at(2, 1, 3),
                "minecraft:oak_wall_sign[facing=north]{\"back\":{\"glowing\":false,\"lines\":[]},\
                 \"front\":{\"glowing\":false,\"lines\":[\"Hi\"]},\"kind\":\"sign\"}"
                    .to_owned()
            ),
            (
                at(3, 1, 1),
                "minecraft:brown_bed[facing=north,part=head]{\"color\":\"brown\",\"kind\":\"bed\"}"
                    .to_owned()
            ),
            (
                at(3, 1, 2),
                "minecraft:brown_bed[facing=north,part=foot]{\"color\":\"brown\",\"kind\":\"bed\"}"
                    .to_owned()
            ),
        ])
    );
}
