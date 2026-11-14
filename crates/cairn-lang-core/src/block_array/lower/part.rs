//! Placing a Tier 0 part, a `repeater`, `comparator`, `redstone_torch` or `redstone_dust`, in the
//! cell a coordinate triple or a wall anchor names (`spec/redstone` "Tier 0 parts").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::BlockState;
use crate::block_array::args::{
    UnreadArgument, diag_invalid_member, ident_or_ignore, read_or_ignore,
};
use crate::block_array::geometry::Declared;
use crate::block_array::joins::Face;

use super::canvas::{MemberCanvas, ScopePalette};
use super::cell_box::{Cell, Shape, written_cells};
use super::fixture::{
    Anchor, Fixture, Refusal, Toward, fixture_cell, outside_the_array, written_facing,
};
use super::members::{StructCtx, default_block_id};

/// The note on an `at=` that is missing or neither form, saying what the two are.
const STANDS: &str = "write `at=` as the coordinate triple `x,y,z` of the cell the part stands in, \
                      as in `at=1,1,1`, or as a wall anchor, `at=<side>.outside` or \
                      `at=inside.<side>`";

/// The note on a compass `facing=`, saying which way each word looks.
const COMPASS: &str = "`facing=` takes one of north, south, east, west: north toward the `back` \
                       wall and east toward the `right` one";

/// The four compass faces, in the order a finding lists them.
const COMPASS_FACES: [Face; 4] = [Face::North, Face::South, Face::East, Face::West];

/// A part's catalog token, and the Java id used without a pack.
#[derive(Debug, Clone, Copy)]
struct Spelled {
    token: &'static str,
    base_id: &'static str,
}

const REPEATER: Spelled = Spelled {
    token: "repeater.default",
    base_id: "minecraft:repeater",
};

const COMPARATOR: Spelled = Spelled {
    token: "comparator.default",
    base_id: "minecraft:comparator",
};

const STANDING_TORCH: Spelled = Spelled {
    token: "redstone_torch.default",
    base_id: "minecraft:redstone_torch",
};

const WALL_TORCH: Spelled = Spelled {
    token: "redstone_torch.wall",
    base_id: "minecraft:redstone_wall_torch",
};

const DUST: Spelled = Spelled {
    token: "redstone_dust.default",
    base_id: "minecraft:redstone_wire",
};

/// Which of the four parts a line places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Repeater,
    Comparator,
    Torch,
    Dust,
}

impl Part {
    fn fixture(self) -> Fixture {
        let (keyword, noun) = match self {
            Self::Repeater => ("repeater", "repeater"),
            Self::Comparator => ("comparator", "comparator"),
            Self::Torch => ("redstone_torch", "redstone torch"),
            Self::Dust => ("redstone_dust", "redstone dust"),
        };
        Fixture { keyword, noun }
    }

    /// Whether it reads the cell behind it and drives the one in front.
    fn drives(self) -> bool {
        matches!(self, Self::Repeater | Self::Comparator)
    }
}

/// The states a part's line writes, read before anything can refuse the part.
#[derive(Debug, Default)]
struct Written<'m> {
    facing: Option<&'m str>,
    delay: Option<i64>,
    mode: Option<&'m str>,
    connect: Option<&'m str>,
}

/// How a part's `at=` names its cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stand {
    /// A coordinate triple.
    Triple,
    /// A wall anchor.
    ByWall,
}

/// Place a Tier 0 part in its cell, its blockstate derived from where it stands.
pub(super) fn fill_part(
    part: Part,
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut unread: Vec<UnreadArgument> = Vec::new();
    let written = read_written(part, member, &mut unread);
    let stand = match place(part, member, &written, y_offset, ctx, palette, canvas) {
        Ok(stand) => Some(stand),
        Err(refusal) => {
            diagnostics.push(*refusal);
            None
        }
    };
    let noun = part.fixture().noun;
    for argument in unread {
        let consequence = match stand {
            None => {
                format!("this {noun} is not built either way — see the finding on the same line")
            }
            Some(stand) => unread_consequence(part, argument.key, stand),
        };
        diagnostics.push(argument.report(&consequence));
    }
}

/// What a part is built as when its `key=` is ignored.
fn unread_consequence(part: Part, key: &str, stand: Stand) -> String {
    let noun = part.fixture().noun;
    match (key, part, stand) {
        ("facing", Part::Torch, Stand::Triple) => {
            "the redstone torch stands on the block under it".to_owned()
        }
        ("facing", Part::Torch, Stand::ByWall) => {
            "the redstone torch hangs on the wall it stands by, pointing away from it".to_owned()
        }
        ("facing", Part::Repeater | Part::Comparator, _) => {
            format!("the {noun} drives away from the wall it stands by")
        }
        ("delay", Part::Repeater, _) => "the repeater is built with `delay=1`".to_owned(),
        ("mode", Part::Comparator, _) => "the comparator is built with `mode=compare`".to_owned(),
        ("connect", Part::Dust, _) => {
            "the redstone dust takes its sides from the blocks beside it".to_owned()
        }
        _ => format!("the {noun} is built as if the line wrote no `{key}=`"),
    }
}

/// Every state `part` reads off its line; a value of the wrong kind joins `unread` instead.
fn read_written<'m>(
    part: Part,
    member: &'m Member,
    unread: &mut Vec<UnreadArgument>,
) -> Written<'m> {
    let mut ident = |key: &'static str, expected: &'static str| {
        ident_or_ignore(member, key, expected).unwrap_or_else(|argument| {
            unread.push(argument);
            None
        })
    };
    let mut written = Written::default();
    match part {
        Part::Repeater | Part::Comparator => {
            written.facing = ident(
                "facing",
                "a way it drives, `in`, `out`, `left` or `right` by a wall and `north`, `south`, \
                 `east` or `west` at a triple",
            );
        }
        Part::Torch => {
            written.facing = ident(
                "facing",
                "a compass face, `north`, `south`, `east` or `west`, at a triple (by a wall the \
                 torch takes none)",
            );
        }
        Part::Dust => written.connect = ident("connect", "`none` or `all`"),
    }
    match part {
        Part::Comparator => written.mode = ident("mode", "`compare` or `subtract`"),
        Part::Repeater => {
            written.delay = read_or_ignore(
                member,
                "delay",
                |kind| match kind {
                    ValueKind::Int(delay) => Some(*delay),
                    _ => None,
                },
                "a whole number from 1 to 4",
            )
            .unwrap_or_else(|argument| {
                unread.push(argument);
                None
            });
        }
        Part::Torch | Part::Dust => {}
    }
    written
}

/// [`fill_part`], answering how the part's `at=` named its cell, or the finding that refuses it.
fn place(
    part: Part,
    member: &Member,
    written: &Written<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
) -> Result<Stand, Refusal> {
    let fixture = part.fixture();
    let mut properties = states(part, member, written)?;
    let stand = stand_of(fixture, member)?;
    let (block, facing) = turn(part, member, written.facing, stand)?;
    let (cell, host_face) = match stand {
        Stand::Triple => (triple_cell(fixture, member, y_offset, ctx)?, None),
        Stand::ByWall => {
            let (anchor, cell) = fixture_cell(fixture, member, y_offset, ctx)?;
            if let Some(facing) = by_the_wall(part, anchor, facing) {
                properties.insert("facing".to_owned(), facing.as_str().to_owned());
            }
            (cell, Some(anchor.host_face()))
        }
    };
    if let Some(Look::Compass(face)) = facing {
        let game = if part.drives() { face.opposite() } else { face };
        properties.insert("facing".to_owned(), game.as_str().to_owned());
    }
    properties.sort_keys();
    let id = default_block_id(member, ctx.registry, block.token, block.base_id)?;
    let state = BlockState { id, properties };
    canvas.declare(Declared::cell(cell, host_face));
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if canvas.try_paint(cell, || palette.intern(state)) {
        Ok(stand)
    } else {
        Err(Box::new(outside_the_array(fixture, member)))
    }
}

/// The `delay`, `mode` or dust sides a part's line writes, or the refusal of a value it does not
/// take.
fn states(
    part: Part,
    member: &Member,
    written: &Written<'_>,
) -> Result<IndexMap<String, String>, Refusal> {
    let mut properties = IndexMap::new();
    match part {
        Part::Repeater => {
            let delay = written.delay.unwrap_or(1);
            if !(1..=4).contains(&delay) {
                return Err(Box::new(diag_invalid_member(
                    member,
                    format!("repeater `delay={delay}` is no delay a repeater takes"),
                    vec!["`delay=` takes a whole number from 1 to 4, the ticks the repeater holds a signal".to_owned()],
                )));
            }
            properties.insert("delay".to_owned(), delay.to_string());
        }
        Part::Comparator => {
            let mode = word(member, "mode", written.mode, &["compare", "subtract"])?;
            properties.insert("mode".to_owned(), mode.unwrap_or("compare").to_owned());
        }
        Part::Dust => {
            let side = match word(member, "connect", written.connect, &["none", "all"])? {
                Some("none") => Some("none"),
                Some(_) => Some("side"),
                None => None,
            };
            for face in side.map_or(&[][..], |_| &Face::SIDES[..]) {
                properties.insert(face.as_str().to_owned(), side.unwrap_or("none").to_owned());
            }
        }
        Part::Torch => {}
    }
    Ok(properties)
}

/// `written` when it is one of `words`, `None` when the line writes none, or the refusal of any
/// other word.
fn word<'m>(
    member: &Member,
    key: &str,
    written: Option<&'m str>,
    words: &[&'static str],
) -> Result<Option<&'m str>, Refusal> {
    let Some(name) = written else {
        return Ok(None);
    };
    if words.contains(&name) {
        return Ok(Some(name));
    }
    let mut notes = vec![format!("`{key}=` takes {}", words.join(" or "))];
    notes.extend(
        nearest_match(name, words.iter().copied())
            .map(|near| did_you_mean(&format!("{key}={near}"))),
    );
    Err(Box::new(diag_invalid_member(
        member,
        format!("unknown {} `{key}={name}`", member.role.keyword()),
        notes,
    )))
}

/// How the part's `at=` names its cell, or the refusal of an `at=` that is missing or neither form.
fn stand_of(fixture: Fixture, member: &Member) -> Result<Stand, Refusal> {
    let keyword = fixture.keyword;
    let refuse = |primary: String| {
        Box::new(diag_invalid_member(
            member,
            primary,
            vec![STANDS.to_owned()],
        ))
    };
    let Some(raw) = member.arguments.get("at") else {
        return Err(refuse(format!(
            "{keyword} has no `at=`, so nothing says where it stands"
        )));
    };
    match raw.value.kind {
        ValueKind::Coord { .. } => Ok(Stand::Triple),
        ValueKind::DotRef(_) => Ok(Stand::ByWall),
        _ => Err(refuse(format!(
            "{keyword} `at=` must be a coordinate triple or a wall anchor, not {}",
            raw.value.describe(),
        ))),
    }
}

/// Which way a part's `facing=` turns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// A way from the wall it stands by.
    FromTheWall(Option<Toward>),
    /// A compass face, at a triple.
    Compass(Face),
}

/// The block a part is built from and the way its line turns it, or the refusal of a `facing=`
/// its anchor does not take there.
fn turn(
    part: Part,
    member: &Member,
    facing: Option<&str>,
    stand: Stand,
) -> Result<(Spelled, Option<Look>), Refusal> {
    let fixture = part.fixture();
    let keyword = fixture.keyword;
    let block = match part {
        Part::Repeater => REPEATER,
        Part::Comparator => COMPARATOR,
        Part::Torch => STANDING_TORCH,
        Part::Dust => DUST,
    };
    match (part, stand) {
        (Part::Dust, _) => Ok((block, None)),
        (Part::Torch, Stand::ByWall) => match facing {
            None => Ok((WALL_TORCH, Some(Look::FromTheWall(None)))),
            Some(word) => Err(Box::new(diag_invalid_member(
                member,
                format!(
                    "redstone_torch `facing={word}` by a wall: the torch hangs on the wall's face, \
                     so `facing=` has nothing to choose"
                ),
                vec!["drop `facing=`; the torch points away from the wall it hangs on".to_owned()],
            ))),
        },
        (_, Stand::ByWall) => {
            let toward = written_facing(fixture, member, facing, &Toward::HORIZONTAL)?;
            Ok((block, Some(Look::FromTheWall(toward))))
        }
        (_, Stand::Triple) => {
            let Some(word) = facing else {
                if part == Part::Torch {
                    return Ok((block, None));
                }
                return Err(Box::new(diag_invalid_member(
                    member,
                    format!(
                        "{keyword} at a coordinate triple has no `facing=`, so nothing says which \
                         way it drives"
                    ),
                    vec![
                        COMPASS.to_owned(),
                        "no wall stands by a part at a triple for it to drive away from".to_owned(),
                    ],
                )));
            };
            if let Some(face) = Face::from_name(word).filter(|face| COMPASS_FACES.contains(face)) {
                let block = if part == Part::Torch {
                    WALL_TORCH
                } else {
                    block
                };
                return Ok((block, Some(Look::Compass(face))));
            }
            let mut notes = vec![COMPASS.to_owned()];
            let from_a_wall = Toward::HORIZONTAL
                .iter()
                .any(|toward| toward.name() == word);
            if from_a_wall && part == Part::Torch {
                notes.push(format!(
                    "`{word}` names a way from a wall, which a redstone torch does not take: by a \
                     wall it hangs on the wall and points away from it, with no `facing=`"
                ));
            } else if from_a_wall {
                notes.push(format!(
                    "`{word}` names a way from a wall, and a part at a coordinate triple stands by \
                     none; anchor it by a wall with `at=<side>.outside` or `at=inside.<side>` to \
                     turn it that way"
                ));
            } else {
                let names = COMPASS_FACES.map(Face::as_str);
                notes.extend(
                    nearest_match(word, names).map(|near| did_you_mean(&format!("facing={near}"))),
                );
            }
            Err(Box::new(diag_invalid_member(
                member,
                format!("unknown {keyword} `facing={word}` at a coordinate triple"),
                notes,
            )))
        }
    }
}

/// The game's `facing` of a part set by a wall at `anchor`, or `None` for dust.
fn by_the_wall(part: Part, anchor: Anchor, look: Option<Look>) -> Option<Face> {
    let side = anchor.side();
    let away = Toward::away_from_the_wall(anchor);
    match (part, look) {
        (Part::Torch, _) => Some(away.face(side)),
        (_, Some(Look::FromTheWall(toward))) => Some(toward.unwrap_or(away).face(side).opposite()),
        _ => None,
    }
}

/// The cell a part's coordinate triple names, or the refusal of an `offset=` or `y=` beside it.
fn triple_cell(
    fixture: Fixture,
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<Cell, Refusal> {
    let beside: Vec<String> = ["offset", "y"]
        .into_iter()
        .filter(|key| member.arguments.contains_key(*key))
        .map(|key| format!("`{key}=`"))
        .collect();
    if !beside.is_empty() {
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "{} {} beside a coordinate triple, which names the cell whole",
                fixture.keyword,
                beside.join(" and "),
            ),
            vec![format!(
                "drop {}, or set the part by a wall with `at=<side>.outside` or \
                 `at=inside.<side>`, which reads them",
                beside.join(" and "),
            )],
        )));
    }
    let cells = written_cells(member, Shape::Cell, y_offset, ctx)?;
    Ok(cells
        .cells()
        .next()
        .expect("a cell box holds at least one cell"))
}
