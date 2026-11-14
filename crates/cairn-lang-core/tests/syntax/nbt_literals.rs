//! An `nbt={...}` compound is a `key=` value in the SNBT subset `spec/syntax` "Literals and
//! separators" reads, kept as written; only the lexer refuses a malformed one.

use cairn_lang_core::ast::{Item, Statement, ValueKind};
use cairn_lang_core::lex::{NbtCompound, NbtValue, parse_compound};
use cairn_lang_core::parse;

use crate::{member_args, member_refusal};

fn nbt(text: &str) -> ValueKind {
    ValueKind::Nbt(text.to_owned())
}

#[test]
fn a_brace_after_an_equals_sign_opens_a_compound_kept_as_written() {
    for compound in [
        "{}",
        "{variant:\"black\"}",
        "{ a : 1 , b : 2b }",
        "{\"a b\":'it\\'s', c-d.e+f:\"\\\\\"}",
        "{n:[1, -2, +3], f:[0.5f, 1f], d:[1.5, 2d], t:[true, 0b, 1B]}",
        "{w:[B; 1b, -128b], i:[I; 2147483647], l:[L; 1L, -9223372036854775808l], e:[I;]}",
        "{nested:{deeper:[{a:1s}, {b:[]}]}, lists:[[1], [2b], []]}",
        "{z:007.5, y:0, x:-0}",
    ] {
        assert_eq!(
            member_args(&format!("walls nbt={compound} height=3")),
            [
                ("nbt".to_owned(), nbt(compound)),
                ("height".to_owned(), ValueKind::Int(3)),
            ],
            "{compound}",
        );
    }
}

#[test]
fn a_compound_spans_its_braces_and_may_stand_apart_from_its_equals_sign() {
    let source = "struct s size=5x5\n  walls nbt= {a:1}\n  door[nbt={b:2}] side=front\n";
    let module = parse(source).expect("parses");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct");
    };
    let Statement::Generic { args, .. } = &body[0] else {
        panic!("expected a member line");
    };
    assert_eq!(&source[args[0].value.span.clone()], "{a:1}");
    assert_eq!(&source[args[0].span.clone()], "nbt= {a:1}");
    let Statement::Generic {
        selector: Some(attrs),
        ..
    } = &body[1]
    else {
        panic!("expected a selector line");
    };
    assert_eq!(attrs[0].value.kind, nbt("{b:2}"));
}

#[test]
fn a_brace_that_follows_no_equals_sign_opens_no_compound() {
    assert_eq!(
        member_refusal("walls {a:1}"),
        ("unexpected `{` in value position".to_owned(), 9),
    );
}

/// Each `(compound, message, column)`: the compound on a `walls` line is refused with `message`.
fn assert_refused(cases: Vec<(&str, String, u32)>) {
    for (compound, message, col) in cases {
        assert_eq!(
            member_refusal(&format!("walls nbt={compound}")),
            (message, col),
            "{compound}",
        );
    }
}

#[test]
fn a_compound_out_of_shape_is_refused_where_it_strays() {
    const LINE_END: &str = "a compound closes on the line it opens on";
    assert_refused(vec![
        (
            "{a:1",
            format!("expected `,` or `}}` in `nbt={{...}}`, got end of line: {LINE_END}"),
            17,
        ),
        (
            "{a:[1, 2}",
            "expected `,` or `]` in `nbt={...}`, got `}`".to_owned(),
            21,
        ),
        (
            "{a 1}",
            "expected `:` after the key in `nbt={...}`, got `1`".to_owned(),
            16,
        ),
        (
            "{:1}",
            "expected a key in `nbt={...}`, got `:`".to_owned(),
            14,
        ),
        (
            "{a:}",
            "expected a value in `nbt={...}`, got `}`".to_owned(),
            16,
        ),
        (
            "{a:1,}",
            "a comma in `nbt={...}` stands between two entries, and none follows this one: \
             delete it"
                .to_owned(),
            17,
        ),
        (
            "{a:1,,b:2}",
            "one comma stands between two entries in `nbt={...}`, and this is a second: delete \
             it"
            .to_owned(),
            18,
        ),
        (
            "{a:[1,]}",
            "a comma in `nbt={...}` stands between two values, and none follows this one: \
             delete it"
                .to_owned(),
            18,
        ),
        (
            "{a:1, \"a\":2}",
            "an NBT compound names each key once, and `a` is named twice: delete one of them"
                .to_owned(),
            19,
        ),
        (
            "{a:1 # note}",
            "expected `,` or `}` in `nbt={...}`, got `#`".to_owned(),
            18,
        ),
    ]);
}

#[test]
fn a_string_is_quoted_and_escapes_only_a_backslash_and_its_quote() {
    assert_refused(vec![
        (
            "{a:black}",
            "`black` is not quoted: a string in `nbt={...}` is, as `\"black\"`, since SNBT reads \
             a word that is not a number as text"
                .to_owned(),
            16,
        ),
        (
            "{a:\"x\\n\"}",
            "a string in `nbt={...}` escapes only `\\` and its own quote, as `\\\\` and `\\\"`"
                .to_owned(),
            18,
        ),
        (
            "{a:'open}",
            "unterminated string in `nbt={...}`: close it with `'` on this line".to_owned(),
            16,
        ),
    ]);
}

#[test]
fn a_number_outside_the_subset_or_its_range_is_refused() {
    let number_runs = "a number in `nbt={...}` is digits, a `.` and digits for a fraction, and a \
                       type suffix, with no exponent, and SNBT reads the whole run as text";
    assert_refused(vec![
        (
            "{a:1.5e3}",
            format!("the number `1.5` runs into `e`: {number_runs}"),
            16,
        ),
        (
            "{a:0x1F}",
            format!("the number `0` runs into `x`: {number_runs}"),
            16,
        ),
        (
            "{a:1_000}",
            format!("the number `1` runs into `_`: {number_runs}"),
            16,
        ),
        (
            "{a:1.}",
            "`1.` has no digit after its `.`: a number in `nbt={...}` with a fraction has digits \
             on both sides of the `.`, as `1.0`"
                .to_owned(),
            16,
        ),
        (
            "{a:-}",
            "expected a digit in `nbt={...}`, got `}`".to_owned(),
            17,
        ),
        (
            "{a:1.5b}",
            "`1.5b` has a fraction and a whole-number suffix: a byte, a short or a long has no `.`"
                .to_owned(),
            16,
        ),
        (
            "{a:01}",
            "`01` has a leading zero, and SNBT reads a whole number with one as text: write `1`"
                .to_owned(),
            16,
        ),
        (
            "{a:-00s}",
            "`-00s` has a leading zero, and SNBT reads a whole number with one as text: write \
             `-0s`"
                .to_owned(),
            16,
        ),
        (
            "{a:128b}",
            "`128b` is past the range of a byte, -128 to 127".to_owned(),
            16,
        ),
        (
            "{a:32768s}",
            "`32768s` is past the range of a short, -32768 to 32767".to_owned(),
            16,
        ),
        (
            "{a:2147483648}",
            "`2147483648` is past the range of an int, -2147483648 to 2147483647".to_owned(),
            16,
        ),
        (
            "{a:9223372036854775808L}",
            "`9223372036854775808L` is past the range of a long, -9223372036854775808 to \
             9223372036854775807"
                .to_owned(),
            16,
        ),
        (
            "{a:340282356779733661637539395458142568448.0f}",
            "`340282356779733661637539395458142568448.0f` is past the range of a float".to_owned(),
            16,
        ),
    ]);
}

#[test]
fn a_list_holds_one_type_and_an_array_its_own() {
    assert_refused(vec![
        (
            "{a:[1, 2b]}",
            "an NBT list holds values of one type: its first is an int, and this one is a byte"
                .to_owned(),
            20,
        ),
        (
            "{a:[{}, []]}",
            "an NBT list holds values of one type: its first is a compound, and this one is a \
             list"
                .to_owned(),
            21,
        ),
        (
            "{a:[B; 1, 2]}",
            "an array of bytes holds only bytes, and this is an int: write each as `1b`".to_owned(),
            20,
        ),
        (
            "{a:[L; 1]}",
            "an array of longs holds only longs, and this is an int: write each as `1L`".to_owned(),
            20,
        ),
        (
            "{a:[I; 1b]}",
            "an array of ints holds only ints, and this is a byte: write each as `1`".to_owned(),
            20,
        ),
        (
            "{a:[I; true]}",
            "expected an int in `nbt={...}`, got `t`".to_owned(),
            20,
        ),
    ]);
}

#[test]
fn a_compound_nests_sixty_four_levels_deep() {
    let nested = |depth: usize| format!("{}{}", "{a:".repeat(depth - 1) + "{", "}".repeat(depth));
    let deepest = nested(64);
    assert_eq!(
        member_args(&format!("walls nbt={deepest}")),
        [("nbt".to_owned(), nbt(&deepest))],
    );
    assert_eq!(
        member_refusal(&format!("walls nbt={}", nested(65))),
        (
            "an `nbt={...}` value nests at most 64 compounds, lists and arrays deep".to_owned(),
            13 + 3 * 64,
        ),
    );
}

#[test]
fn a_compound_reads_as_the_typed_values_snbt_gives_it_in_the_order_written() {
    use NbtValue::{
        Byte, ByteArray, Compound, Double, Float, Int, IntArray, List, Long, LongArray, Short,
    };
    let entry = |key: &str, value| (key.to_owned(), value);
    assert_eq!(
        parse_compound(
            "{z:1b, \"a b\":'it\\'s', s:-2s, i:+3, l:4L, f:0.5f, d:1.5, e:2d, t:true, u:false, \
             n:[1, 2], c:{x:[]}, w:[B; 1b], v:[I;], q:[L; -9223372036854775808l], o:007.5}"
        ),
        Some(NbtCompound {
            entries: vec![
                entry("z", Byte(1)),
                entry("a b", NbtValue::String("it's".to_owned())),
                entry("s", Short(-2)),
                entry("i", Int(3)),
                entry("l", Long(4)),
                entry("f", Float(0.5)),
                entry("d", Double(1.5)),
                entry("e", Double(2.0)),
                entry("t", Byte(1)),
                entry("u", Byte(0)),
                entry("n", List(vec![Int(1), Int(2)])),
                entry(
                    "c",
                    Compound(NbtCompound {
                        entries: vec![entry("x", List(Vec::new()))],
                    }),
                ),
                entry("w", ByteArray(vec![1])),
                entry("v", IntArray(Vec::new())),
                entry("q", LongArray(vec![i64::MIN])),
                entry("o", Double(7.5)),
            ],
        }),
    );
    for refused in ["", "{a:1", "{a:1} ", "[1]", "{a:b}", "{a:1, a:2}"] {
        assert_eq!(parse_compound(refused), None, "{refused:?}");
    }
}
