use super::*;

/// Sources whose lines Rust already agrees about. `split` has to match
/// `str::lines` on all of them, or replacing one with the other changes
/// behaviour nobody asked to change.
const AGREED: &[&str] = &[
    "",
    "a",
    "a\n",
    "a\nb",
    "a\nb\n",
    "\n",
    "a\n\nb\n",
    "a\r\nb\r\n",
    "a\r\n\r\nb",
    "\r\n",
];

#[test]
fn terminator_len_names_each_of_the_three_breaks() {
    assert_eq!(terminator_len("a\r\nb", 1), Some(2));
    assert_eq!(terminator_len("a\rb", 1), Some(1));
    assert_eq!(terminator_len("a\nb", 1), Some(1));
    assert_eq!(terminator_len("a\r\nb", 0), None);
    // Past the end is not a terminator, so callers can ask about any
    // offset without a bounds check of their own.
    assert_eq!(terminator_len("a", 1), None);
    assert_eq!(terminator_len("a", 99), None);
    // The `\n` of a `\r\n` is the pair's second byte, not a break of
    // its own — asking about it would double-count the line.
    assert_eq!(terminator_len("a\r\nb", 2), Some(1));
}

#[test]
fn starts_a_new_line_after_every_break() {
    assert_eq!(starts(""), vec![0]);
    assert_eq!(starts("ab"), vec![0]);
    assert_eq!(starts("ab\ncd"), vec![0, 3]);
    assert_eq!(starts("ab\r\ncd"), vec![0, 4]);
    assert_eq!(starts("ab\rcd"), vec![0, 3]);
    // A trailing break opens a final empty line.
    assert_eq!(starts("ab\r"), vec![0, 3]);
    // Consecutive breaks of different kinds each open one line.
    assert_eq!(starts("\r\r\n\n"), vec![0, 1, 3, 4]);
}

/// Sources with a lone `\r`, an empty line, or a break at either edge —
/// the shapes where stepping across a terminator can leave its own line.
const AWKWARD: &[&str] = &[
    "\n",
    "\nx",
    "\n\r\n",
    "\r\r",
    "\r\r\r",
    "a\r\r\n",
    "a\rb\r\nc\nd",
    "\r\n\r",
    "x\r\ny",
];

#[test]
fn end_before_stops_at_the_terminator_it_follows() {
    assert_eq!(end_before("a\nb", 2), 1);
    assert_eq!(end_before("a\r\nb", 3), 1);
    assert_eq!(end_before("a\rb", 2), 1);
    // A source opening with a break: the first line is empty and ends
    // where it starts. Reading the byte before it would leave the
    // source entirely.
    assert_eq!(end_before("\nx", 1), 0);
    assert_eq!(end_before("\r\nx", 2), 0);
    // An empty line between two breaks gives up its own terminator and
    // not the one that opened it.
    assert_eq!(end_before("\r\r", 2), 1);
    assert_eq!(end_before("a\r\r\n", 4), 2);
}

#[test]
fn end_before_never_leaves_the_line_it_ends() {
    for source in AGREED.iter().copied().chain(AWKWARD.iter().copied()) {
        let index = starts(source);
        for pair in index.windows(2) {
            let [line_start, next_start] = *pair else {
                unreachable!("windows(2)")
            };
            let end = end_before(source, next_start);
            assert!(
                (line_start..next_start).contains(&end),
                "{source:?}: line {line_start}..{next_start} ends at {end}",
            );
            assert!(
                !source[line_start..end].contains(['\r', '\n']),
                "{source:?}: line {line_start}..{end} covers its terminator",
            );
            assert!(
                terminator_len(source, end).is_some(),
                "{source:?}: {end} should be where the terminator begins",
            );
        }
    }
}

#[test]
fn split_matches_str_lines_wherever_str_lines_is_right() {
    for source in AGREED {
        assert_eq!(
            split(source).collect::<Vec<_>>(),
            source.lines().collect::<Vec<_>>(),
            "{source:?}",
        );
    }
}

#[test]
fn split_also_breaks_at_a_lone_carriage_return() {
    assert_eq!(split("a\rb\r").collect::<Vec<_>>(), vec!["a", "b"]);
    assert_eq!(split("a\r\rb").collect::<Vec<_>>(), vec!["a", "", "b"]);
    // The case that motivates the module: `str::lines` yields one line
    // here, so a scanner built on it sees `floor walls` as one command.
    assert_eq!(
        split("floor\rwalls").collect::<Vec<_>>(),
        vec!["floor", "walls"]
    );
}

#[test]
fn split_matches_str_lines_backwards_too() {
    for source in AGREED {
        assert_eq!(
            split(source).rev().collect::<Vec<_>>(),
            source.lines().rev().collect::<Vec<_>>(),
            "{source:?}",
        );
    }
}

/// The two ends have to meet exactly once, or a line is yielded twice
/// or not at all. Alternating is the only way to catch that.
#[test]
fn split_yields_each_line_once_when_walked_from_both_ends() {
    for source in AGREED.iter().copied().chain(AWKWARD.iter().copied()) {
        let expected: Vec<&str> = split(source).collect();
        let mut iterator = split(source);
        let (mut front, mut back) = (Vec::new(), Vec::new());
        let mut take_front = true;
        loop {
            let next = if take_front {
                iterator.next()
            } else {
                iterator.next_back()
            };
            let Some(line) = next else { break };
            if take_front { &mut front } else { &mut back }.push(line);
            take_front = !take_front;
        }
        back.reverse();
        front.extend(back);
        assert_eq!(front, expected, "{source:?}");
    }
}

#[test]
fn split_also_breaks_at_a_lone_carriage_return_backwards() {
    assert_eq!(split("a\rb\r").rev().collect::<Vec<_>>(), vec!["b", "a"]);
    assert_eq!(
        split("a\r\rb").rev().collect::<Vec<_>>(),
        vec!["b", "", "a"]
    );
}

#[test]
fn start_of_agrees_with_the_index_at_every_offset() {
    for source in AGREED.iter().copied().chain(AWKWARD.iter().copied()) {
        let index = starts(source);
        for offset in 0..=source.len() {
            let from_index = index[index.partition_point(|&s| s <= offset) - 1];
            assert_eq!(
                start_of(source, offset),
                from_index,
                "{source:?} at {offset}"
            );
        }
    }
}

#[test]
fn split_and_starts_describe_the_same_lines() {
    for source in ["a\rb\r\nc\nd", "\r\n\r\r\n", "x", ""] {
        let from_starts: Vec<&str> = starts(source)
            .iter()
            .map(|&start| {
                let end = source[start..]
                    .char_indices()
                    .find(|(i, _)| terminator_len(source, start + i).is_some())
                    .map_or(source.len(), |(i, _)| start + i);
                &source[start..end]
            })
            .collect();
        // `starts` keeps the trailing empty line that `split` drops, so
        // the comparison is against the prefix.
        let split_lines: Vec<&str> = split(source).collect();
        assert_eq!(
            from_starts[..split_lines.len()],
            split_lines[..],
            "{source:?}"
        );
    }
}

fn position(line: u32, col: u32) -> Position {
    Position {
        line: NonZeroU32::new(line).expect("line"),
        col: NonZeroU32::new(col).expect("col"),
    }
}

/// Every byte offset survives the trip out to a position and back.
///
/// The two conversions are written separately — one walks the line
/// starts forward, the other counts characters along a line — so
/// nothing but this says they agree. Non-ASCII on both sides of a
/// break, because the column counts Unicode scalar values and a
/// byte-counting version passes an ASCII-only round trip.
#[test]
fn a_byte_offset_survives_the_round_trip_through_a_position() {
    let source = "α\nfoo\nβar\n";
    let index = LineStarts::new(source);
    for (offset, _) in source.char_indices() {
        assert_eq!(
            index.offset_of(source, index.position(source, offset)),
            offset,
            "round trip failed at byte {offset}",
        );
    }
}

/// A position from outside the source clamps instead of panicking.
///
/// Nothing in this crate can reach it — a `ParseError` reports a
/// position into the source it failed on — so the guard exists for the
/// caller holding a position from a *different* revision of a
/// document, which an editor does between a keystroke and a re-parse.
/// Asserted here because there is nowhere else it can be.
#[test]
fn a_position_from_outside_the_source_clamps() {
    let source = "ab\ncd";
    let index = LineStarts::new(source);
    assert_eq!(
        index.offset_of(source, position(1, 99)),
        2,
        "a column past the end of its line clamps to that line's end, \
             before the terminator",
    );
    assert_eq!(
        index.offset_of(source, position(9, 1)),
        source.len(),
        "a line past the end of the source clamps to its end",
    );
}

#[test]
fn position_at_handles_unicode_columns() {
    // Two-byte UTF-8 character: the column count must advance by 1
    // (one Unicode scalar value), not by the byte length.
    let source = "α\nβ\n";
    let pos_after_alpha = position_at(source, 2); // byte 2 = start of '\n'
    assert_eq!(pos_after_alpha.line.get(), 1);
    assert_eq!(pos_after_alpha.col.get(), 2);

    let pos_on_beta = position_at(source, 3); // byte 3 = start of 'β'
    assert_eq!(pos_on_beta.line.get(), 2);
    assert_eq!(pos_on_beta.col.get(), 1);
}

#[test]
fn position_at_for_offset_past_end_clamps_to_eof() {
    let source = "abc\n";
    let pos = position_at(source, 99);
    assert_eq!(pos.line.get(), 2);
    assert_eq!(pos.col.get(), 1);
}

#[test]
fn position_agrees_with_a_direct_walk_of_the_source() {
    // The index exists so N diagnostics do not re-walk the source N
    // times, and the walk is what it has to reproduce: step one
    // character at a time, bumping the line at each break and the
    // column otherwise. Comparing against `position_at` instead would
    // now be comparing the index with itself — there is one
    // implementation, which is the point of having one rule.
    for source in ["α\nfoo\nbar\nβaz\n", "a\r\nb\rc", "\r\r\n", "", "no break"] {
        let lines = LineStarts::new(source);
        let (mut line, mut col, mut offset) = (1_u32, 1_u32, 0_usize);
        loop {
            let expected = Position {
                line: NonZeroU32::new(line).expect("1-based"),
                col: NonZeroU32::new(col).expect("1-based"),
            };
            assert_eq!(
                lines.position(source, offset),
                expected,
                "{source:?} at byte {offset}",
            );
            if offset == source.len() {
                break;
            }
            if let Some(len) = terminator_len(source, offset) {
                offset += len;
                line += 1;
                col = 1;
            } else {
                offset += source[offset..]
                    .chars()
                    .next()
                    .expect("offset is inside the source")
                    .len_utf8();
                col += 1;
            }
        }
    }
}

/// `line_of` is the line `position` reports, counted from zero, at
/// every offset under each break style and past the end of the source.
#[test]
fn line_of_is_the_zero_based_line_of_position() {
    for source in ["α\nfoo\nbar\nβaz\n", "a\r\nb\rc", "\r\r\n", "", "no break"] {
        let lines = LineStarts::new(source);
        for offset in
            (0..=source.len() + 2).filter(|&o| o > source.len() || source.is_char_boundary(o))
        {
            let line = lines.position(source, offset).line.get() as usize;
            assert_eq!(
                lines.line_of(offset),
                line - 1,
                "{source:?} at byte {offset}"
            );
        }
    }
    let crlf = LineStarts::new("a\r\nb");
    assert_eq!(crlf.line_of(1), 0, "the `\\r` is on the line it ends");
    assert_eq!(crlf.line_of(2), 0, "so is the `\\n` after it");
    assert_eq!(crlf.line_of(3), 1);
}
