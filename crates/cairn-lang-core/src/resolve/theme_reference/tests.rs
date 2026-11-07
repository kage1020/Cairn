use super::any_member_reads_a_slot;
use crate::diagnostic::DiagnosticCode;
use crate::edition::Edition;
use crate::resolve::resolve;
use crate::{lower, parse};

#[test]
fn a_slot_read_from_a_nested_member_counts_as_reading_one() {
    let src = [
        "theme shop_bedrock:",
        "  slot floor -> @dark_oak_planks",
        "",
        "struct s size=4x4",
        "  level y=1",
        "    floor mat_slot=floor",
        "",
    ]
    .join(
        "
",
    );
    let module = parse(&src).expect("parses");
    let intent = lower(&module);
    assert!(
        any_member_reads_a_slot(&intent),
        "premise: the nested member is the only reader in the module",
    );
    let r = resolve(&intent, Some(Edition::Java));
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
        "got {:?}",
        r.diagnostics,
    );
}

#[test]
fn a_slot_named_on_a_role_that_reads_none_does_not_count_as_reading_one() {
    let src = [
        "theme shop_bedrock:",
        "  slot floor -> @dark_oak_planks",
        "",
        "struct s size=7x5",
        "  circuit region=floor void=2 mat_slot=floor",
        "",
    ]
    .join("\n");
    let module = parse(&src).expect("parses");
    let intent = lower(&module);
    assert!(
        !any_member_reads_a_slot(&intent),
        "the circuit's `mat_slot=` is not a read",
    );
    let r = resolve(&intent, Some(Edition::Java));
    assert!(
        !r.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
        "got {:?}",
        r.diagnostics,
    );
}
