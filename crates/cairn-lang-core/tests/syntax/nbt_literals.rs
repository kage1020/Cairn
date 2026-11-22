//! An NBT compound is a `key=` value in the SNBT subset `spec/syntax` "Literals and separators"
//! reads, kept as written; only the lexer refuses a malformed one.

use cairn_lang_core::ast::{Item, Statement, ValueKind};
use cairn_lang_core::lex::{NbtCompound, NbtValue, parse_compound};
use cairn_lang_core::parse;

use crate::{member_args, member_refusal, pos};

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
        "{z:007.5, y:0, x:-0, f:07f, d:01D, e:-0.5}",
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
fn a_brace_that_follows_no_key_s_equals_sign_opens_no_compound() {
    for (member, message, col) in [
        ("walls {a:1}", "unexpected `{` in value position", 9),
        ("walls tags=[{a:1}]", "unexpected `{` in value position", 15),
        ("logic sig.a = {sig.b}", "expected identifier, got `{`", 17),
        (
            "logic sig.x = {sig.a and sig.b}",
            "expected identifier, got `{`",
            17,
        ),
        ("logic sig.x = {", "expected identifier, got `{`", 17),
        (
            "logic sig.x = latch(set={sig.a}, reset=sig.b)",
            "expected a signal or expression after `set=`, got `{`: each operand of `latch` takes \
             a value, as in `latch(set=…, reset=…)`",
            27,
        ),
    ] {
        assert_eq!(
            member_refusal(member),
            (message.to_owned(), col),
            "{member:?}"
        );
    }
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
            format!("expected `,` or `}}` in an NBT compound, got end of line: {LINE_END}"),
            17,
        ),
        (
            "{a:[1, 2}",
            "expected `,` or `]` in an NBT compound, got `}`".to_owned(),
            21,
        ),
        (
            "{a 1}",
            "expected `:` after the key in an NBT compound, got `1`".to_owned(),
            16,
        ),
        (
            "{:1}",
            "expected a key in an NBT compound, got `:`".to_owned(),
            14,
        ),
        (
            "{a:}",
            "expected a value in an NBT compound, got `}`".to_owned(),
            16,
        ),
        (
            "{a:1,}",
            "a comma in an NBT compound stands between two entries, and none follows this one: \
             delete it"
                .to_owned(),
            17,
        ),
        (
            "{a:1,,b:2}",
            "one comma stands between two entries in an NBT compound, and this is a second: delete \
             it"
            .to_owned(),
            18,
        ),
        (
            "{a:[1,]}",
            "a comma in an NBT compound stands between two values, and none follows this one: \
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
            "expected `,` or `}` in an NBT compound, got `#`".to_owned(),
            18,
        ),
    ]);
}

#[test]
fn a_string_is_quoted_and_escapes_only_a_backslash_and_its_quote() {
    assert_refused(vec![
        (
            "{a:black}",
            "`black` is not quoted: an NBT compound reads a string only where it is quoted, and no \
             unquoted word but `true` and `false`: write `\"black\"`"
                .to_owned(),
            16,
        ),
        (
            "{a:\"x\\n\"}",
            "a string in an NBT compound escapes only `\\` and its own quote, as `\\\\` and `\\\"`"
                .to_owned(),
            18,
        ),
        (
            "{a:'say \\\"hi\\\"'}",
            "a string in an NBT compound escapes only `\\` and its own quote, as `\\\\` and `\\'`"
                .to_owned(),
            21,
        ),
        (
            "{a:'open}",
            "unterminated string in an NBT compound: close it with `'` on this line".to_owned(),
            16,
        ),
    ]);
}

#[test]
fn a_compound_left_open_at_a_carriage_return_is_refused_on_its_own_line() {
    for (member, message) in [
        (
            "walls nbt={a:'x",
            "unterminated string in an NBT compound: close it with `'` on this line",
        ),
        (
            "walls nbt={a:",
            "expected a value in an NBT compound, got end of line: a compound closes on the line \
             it opens on",
        ),
    ] {
        let err = parse(&format!("struct s size=5x5\r  {member}\r")).expect_err("refused");
        assert_eq!(
            (err.user_message(), err.position()),
            (message.to_owned(), pos(2, 16)),
            "{member:?}",
        );
    }
}

#[test]
fn a_number_outside_the_subset_is_refused_with_the_spelling_to_write() {
    let number_runs = "a number in an NBT compound is digits, a `.` and digits for a fraction, and a \
                       type suffix, the one form every Java version Cairn targets reads alike";
    assert_refused(vec![
        (
            "{a:1.5e3}",
            format!(
                "the number `1.5` runs into `e`: {number_runs}: write it in that form, or as \
                 `\"1.5e3\"` if it is text"
            ),
            16,
        ),
        (
            "{a:0x1F}",
            format!(
                "the number `0` runs into `x`: {number_runs}: write it in that form, or as \
                 `\"0x1F\"` if it is text"
            ),
            16,
        ),
        (
            "{a:1_000}",
            format!(
                "the number `1` runs into `_`: {number_runs}: write it in that form, or as \
                 `\"1_000\"` if it is text"
            ),
            16,
        ),
        (
            "{a:1.}",
            "`1.` has no digit after its `.`: a number in an NBT compound with a fraction has \
             digits on both sides of the `.`, as `1.0`"
                .to_owned(),
            16,
        ),
        (
            "{a:.5}",
            "`.5` has no digit before its `.`: a number in an NBT compound with a fraction has \
             digits on both sides of the `.`, as `0.5`"
                .to_owned(),
            16,
        ),
        (
            "{a:.5f}",
            "`.5f` has no digit before its `.`: a number in an NBT compound with a fraction has \
             digits on both sides of the `.`, as `0.5f`"
                .to_owned(),
            16,
        ),
        (
            "{a:2.F}",
            "`2.F` has no digit after its `.`: a number in an NBT compound with a fraction has \
             digits on both sides of the `.`, as `2.0F`"
                .to_owned(),
            16,
        ),
        (
            "{a:-.5}",
            "`-.5` has no digit before its `.`: a number in an NBT compound with a fraction has \
             digits on both sides of the `.`, as `-0.5`"
                .to_owned(),
            16,
        ),
        (
            "{a:-}",
            "expected a digit in an NBT compound, got `}`".to_owned(),
            17,
        ),
        (
            "{a:1.5b}",
            "`1.5b` has a fraction and a whole-number suffix, and a byte, a short or a long has no \
             `.`: write `1b` for a whole number, or `1.5` for a double"
                .to_owned(),
            16,
        ),
        (
            "{a:01}",
            "`01` has a leading zero, which a whole number in an NBT compound does not: write `1`"
                .to_owned(),
            16,
        ),
        (
            "{a:-00s}",
            "`-00s` has a leading zero, which a whole number in an NBT compound does not: write \
             `-0s`"
                .to_owned(),
            16,
        ),
    ]);
}

#[test]
fn a_number_past_its_type_s_range_is_refused_with_a_wider_type_that_holds_it() {
    let past = |number: &str, range: &str, wider: &str| {
        format!("`{number}` is past the range of {range}: write a value inside it{wider}")
    };
    let byte = "a byte, -128 to 127";
    let short = "a short, -32768 to 32767";
    let int = "an int, -2147483648 to 2147483647";
    let long = "a long, -9223372036854775808 to 9223372036854775807";
    let float = "a float, about -3.4e38 to 3.4e38";
    let double = "a double, about -1.8e308 to 1.8e308";
    let float_max = "340282356779733661637539395458142568448.0";
    let double_max = format!("1{}", "0".repeat(309));
    assert_refused(vec![
        (
            "{a:128b}",
            past("128b", byte, ", or `128s` for a short"),
            16,
        ),
        (
            "{a:-129b}",
            past("-129b", byte, ", or `-129s` for a short"),
            16,
        ),
        (
            "{a:99999b}",
            past("99999b", byte, ", or `99999` for an int"),
            16,
        ),
        (
            "{a:32768s}",
            past("32768s", short, ", or `32768` for an int"),
            16,
        ),
        (
            "{a:-32769s}",
            past("-32769s", short, ", or `-32769` for an int"),
            16,
        ),
        (
            "{a:2147483648}",
            past("2147483648", int, ", or `2147483648L` for a long"),
            16,
        ),
        (
            "{a:-2147483649}",
            past("-2147483649", int, ", or `-2147483649L` for a long"),
            16,
        ),
        (
            "{a:9223372036854775808L}",
            past("9223372036854775808L", long, ""),
            16,
        ),
        (
            "{a:-9223372036854775809L}",
            past("-9223372036854775809L", long, ""),
            16,
        ),
        (
            &format!("{{a:{float_max}f}}"),
            past(
                &format!("{float_max}f"),
                float,
                &format!(", or `{float_max}d` for a double"),
            ),
            16,
        ),
        (
            &format!("{{a:-{float_max}f}}"),
            past(
                &format!("-{float_max}f"),
                float,
                &format!(", or `-{float_max}d` for a double"),
            ),
            16,
        ),
        (
            &format!("{{a:{double_max}d}}"),
            past(&format!("{double_max}d"), double, ""),
            16,
        ),
        (
            &format!("{{a:-{double_max}.0}}"),
            past(&format!("-{double_max}.0"), double, ""),
            16,
        ),
    ]);
    let edges = "{a:-128b, b:127B, c:-32768s, d:32767S, e:-2147483648, f:2147483647, \
                 g:-9223372036854775808l, h:9223372036854775807L}";
    assert_eq!(
        member_args(&format!("walls nbt={edges}")),
        [("nbt".to_owned(), nbt(edges))],
    );
}

#[test]
fn a_list_holds_one_type_and_an_array_its_own() {
    assert_refused(vec![
        (
            "{a:[1, 2b]}",
            "an NBT list holds values of one type: its first is an int, and this one is a byte: \
             write each value in it as an int"
                .to_owned(),
            20,
        ),
        (
            "{a:[{}, []]}",
            "an NBT list holds values of one type: its first is a compound, and this one is a \
             list: write each value in it as a compound"
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
            "{a:[B; true]}",
            "an array of bytes holds each value as a number, and `true` is not one: write `1b`"
                .to_owned(),
            20,
        ),
        (
            "{a:[I; false]}",
            "an array of ints holds each value as a number, and `false` is not one: write `0`"
                .to_owned(),
            20,
        ),
        (
            "{a:[L; abc]}",
            "an array of longs holds each value as a number, and `abc` is not one: write each as \
             `1L`"
                .to_owned(),
            20,
        ),
        (
            "{a:[I; \"1\"]}",
            "expected an int in an NBT compound, got `\"`".to_owned(),
            20,
        ),
        (
            "{a:[B ; 1b]}",
            "`[B ;` opens an NBT array with a space among its `[`, `B` and `;`, which an array \
             keeps together: write `[B;`"
                .to_owned(),
            16,
        ),
        (
            "{a:[ I;1]}",
            "`[ I;` opens an NBT array with a space among its `[`, `I` and `;`, which an array \
             keeps together: write `[I;`"
                .to_owned(),
            16,
        ),
    ]);
}

#[test]
fn a_compound_nests_sixty_four_levels_deep_whatever_it_nests() {
    let compounds =
        |depth: usize| format!("{}{}", "{a:".repeat(depth - 1) + "{", "}".repeat(depth));
    let lists = |depth: usize| {
        format!(
            "{{a:{}[I;1]{}}}",
            "[".repeat(depth - 2),
            "]".repeat(depth - 2)
        )
    };
    for deepest in [compounds(64), lists(64)] {
        assert_eq!(
            member_args(&format!("walls nbt={deepest}")),
            [("nbt".to_owned(), nbt(&deepest))],
        );
    }
    let too_deep = "an NBT compound nests compounds, lists and arrays at most 64 deep, itself \
                    included: flatten the levels past the 64th";
    assert_eq!(
        member_refusal(&format!("walls nbt={}", compounds(65))),
        (too_deep.to_owned(), 13 + 3 * 64),
    );
    assert_eq!(
        member_refusal(&format!("walls nbt={}", lists(65))),
        (too_deep.to_owned(), 16 + 63),
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

#[test]
fn siblings_take_no_depth_from_one_another() {
    for siblings in ["{}", "[]", "[B;]", "{a:[{b:1}]}"] {
        let compound = format!("{{a:[{}]}}", vec![siblings; 100].join(", "));
        assert_eq!(
            member_args(&format!("walls nbt={compound}")),
            [("nbt".to_owned(), nbt(&compound))],
            "{siblings}",
        );
    }
}
