//! Standing an `armor_stand`, a `villager` or a `spawn` entity on its cell's floor, and offering
//! a `trade` from a villager (`spec/entities` "Standing entities").

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::args::{UnreadArgument, diag_invalid_member, read_or_ignore};
use crate::block_array::{Enchantment, EntityData, Item, Trade};

use super::canvas::MemberCanvas;
use super::entity::{AN_ID, Hold, id_value, place, place_one, report_unread, standing_point};
use super::fixture::{Fixture, Refusal};
use super::members::StructCtx;

/// How an armor stand's findings name it.
const ARMOR_STAND: Fixture = Fixture {
    keyword: "armor_stand",
    noun: "armor stand",
};

/// How a villager's findings name it.
const VILLAGER: Fixture = Fixture {
    keyword: "villager",
    noun: "villager",
};

/// How a spawned entity's findings name it.
const SPAWN: Fixture = Fixture {
    keyword: "spawn",
    noun: "entity",
};

/// What becomes of an unread argument of an entity that is not built.
const NOT_BUILT: &str = "this entity is not built either way — see the finding on the same line";

/// The professions a villager's `profession=` takes, `none` first.
pub(super) const PROFESSIONS: [&str; 15] = [
    "none",
    "armorer",
    "butcher",
    "cartographer",
    "cleric",
    "farmer",
    "fisherman",
    "fletcher",
    "leatherworker",
    "librarian",
    "mason",
    "nitwit",
    "shepherd",
    "toolsmith",
    "weaponsmith",
];

/// The levels a villager's `level=` takes, from level 1.
pub(super) const LEVELS: [&str; 5] = ["novice", "apprentice", "journeyman", "expert", "master"];

/// The most items of one kind a trade buys or sells.
const STACK: i64 = 64;

/// Stand an `armor_stand` on its cell's floor.
pub(super) fn fill_armor_stand(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    stand(
        ARMOR_STAND,
        member,
        y_offset,
        ctx,
        canvas,
        diagnostics,
        |_| Ok(("minecraft:armor_stand".to_owned(), EntityData::ArmorStand)),
    );
}

/// Stand a `villager` of its `profession=` and `level=` on its cell's floor.
pub(super) fn fill_villager(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    stand(
        VILLAGER,
        member,
        y_offset,
        ctx,
        canvas,
        diagnostics,
        |member| {
            let profession = word_of(member, "profession", &PROFESSIONS)?.unwrap_or(PROFESSIONS[0]);
            let level = word_of(member, "level", &LEVELS)?
                .and_then(|level| LEVELS.iter().position(|known| *known == level))
                .unwrap_or(0);
            Ok((
                "minecraft:villager".to_owned(),
                EntityData::Villager {
                    profession: profession.to_owned(),
                    level: u8::try_from(level + 1).expect("five levels fit a byte"),
                    trades: Vec::new(),
                },
            ))
        },
    );
}

/// Stand the entity a `spawn` names by its `type=` on its cell's floor.
pub(super) fn fill_spawn(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    stand(
        SPAWN,
        member,
        y_offset,
        ctx,
        canvas,
        diagnostics,
        |member| match member.written("type").map(|raw| &raw.value.kind) {
            Some(kind) if id_value(kind).is_some() => Ok((
                id_value(kind).expect("checked by the guard"),
                EntityData::Spawn,
            )),
            written => Err(Box::new(diag_invalid_member(
                member,
                match written {
                    None => "spawn has no `type=`, so nothing says which entity it places".into(),
                    Some(_) => format!(
                        "spawn `type=` must be an entity id, not {}",
                        member
                            .written("type")
                            .map(|raw| raw.value.describe())
                            .unwrap_or_default()
                    ),
                },
                vec!["write `type=` with the entity's id, such as `type=cat`".into()],
            ))),
        },
    );
}

/// Place a standing entity, `kind` reading its id and what it holds off its line.
fn stand(
    fixture: Fixture,
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    kind: impl FnOnce(&Member) -> Result<(String, EntityData), Refusal>,
) {
    let mut unread = Vec::new();
    let built = kind(member).and_then(|(id, data)| {
        let placed = place(fixture, Hold::Stands, member, y_offset, ctx, &mut unread)?;
        Ok((id, data, placed))
    });
    let built = match built {
        Ok((id, data, placed)) => {
            let pos = standing_point(placed.cell);
            place_one(member, canvas, placed, &id, pos, data, &mut unread);
            true
        }
        Err(refusal) => {
            diagnostics.push(*refusal);
            false
        }
    };
    report_unread(unread, diagnostics, |key| match (built, key) {
        (false, _) => NOT_BUILT,
        (true, "facing") => "it faces south",
        (true, _) => "it is placed without it",
    });
}

/// `key=` when it is one of `words`, `None` when the line writes none, or the refusal of any
/// other value.
fn word_of<'m>(
    member: &'m Member,
    key: &str,
    words: &[&'static str],
) -> Result<Option<&'m str>, Refusal> {
    let Some(raw) = member.written(key) else {
        return Ok(None);
    };
    if let ValueKind::Ident(word) = &raw.value.kind
        && words.contains(&word.as_str())
    {
        return Ok(Some(word.as_str()));
    }
    let mut notes = vec![format!("`{key}=` takes {}", words.join(", "))];
    if let ValueKind::Ident(word) = &raw.value.kind {
        notes.extend(
            nearest_match(word, words.iter().copied())
                .map(|near| did_you_mean(&format!("{key}={near}"))),
        );
    }
    Err(Box::new(diag_invalid_member(
        member,
        format!(
            "unknown {} `{key}={}`",
            member.role.keyword(),
            raw.value.describe()
        ),
        notes,
    )))
}

/// Offer a `trade` from the villager its `villager=` names.
pub(super) fn fill_trade(
    member: &Member,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut unread = Vec::new();
    match read_trade(member, ctx, &mut unread, diagnostics) {
        Ok((villager, trade)) => canvas.offer_trade(villager, trade),
        Err(refusal) => diagnostics.push(*refusal),
    }
    report_unread(unread, diagnostics, |key| match key {
        "count" => "the trade buys one",
        "sell_count" => "the trade sells one",
        "enchant" => "the item it sells carries no enchantment",
        _ => "the enchantment is level 1",
    });
}

/// The line its `villager=` names and the trade, or the refusal of a trade no villager offers.
fn read_trade(
    member: &Member,
    ctx: &StructCtx<'_>,
    unread: &mut Vec<UnreadArgument>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(usize, Trade), Refusal> {
    let villager = match member.written("villager").map(|raw| &raw.value.kind) {
        Some(ValueKind::Ident(name)) => name.as_str(),
        written => {
            return Err(Box::new(diag_invalid_member(
                member,
                match written {
                    None => "trade has no `villager=`, so no villager offers it".into(),
                    Some(_) => "trade `villager=` must be the `id=` of a villager".into(),
                },
                vec!["write `villager=` with the `id=` of a `villager` of this body".into()],
            )));
        }
    };
    let ids: Vec<&str> = ctx
        .villagers
        .iter()
        .filter_map(|villager| villager.id.as_deref())
        .collect();
    let Some(offered_by) = ctx
        .villagers
        .iter()
        .find(|known| known.id.as_deref() == Some(villager))
    else {
        let mut notes: Vec<String> = nearest_match(villager, ids.iter().copied())
            .map(|near| did_you_mean(&format!("villager={near}")))
            .into_iter()
            .collect();
        notes.push(if ids.is_empty() {
            "this body has no villager with an `id=`: give the villager one".into()
        } else {
            format!("this body's villagers are {}", ids.join(", "))
        });
        return Err(Box::new(diag_invalid_member(
            member,
            format!("trade `villager={villager}`: no villager of this body has `id={villager}`"),
            notes,
        )));
    };
    let buy = traded(member, "buy")?;
    let sell = traded(member, "sell")?;
    let buy_count = count_of(member, "count", unread);
    let sell_count = count_of(member, "sell_count", unread);
    Ok((
        offered_by.span.start,
        Trade {
            buy: Item {
                id: buy,
                count: buy_count,
                enchantment: None,
            },
            sell: Item {
                id: sell,
                count: sell_count,
                enchantment: enchantment_of(member, unread, diagnostics),
            },
        },
    ))
}

/// The item a trade's `key=` names, or the refusal of a line that names none.
fn traded(member: &Member, key: &'static str) -> Result<String, Refusal> {
    match member.written(key) {
        Some(raw) => id_value(&raw.value.kind).ok_or_else(|| {
            Box::new(diag_invalid_member(
                member,
                format!("trade `{key}=` must be {AN_ID}, not {}", raw.value.describe()),
                vec![format!("write `{key}=` with the item's id")],
            ))
        }),
        None => Err(Box::new(diag_invalid_member(
            member,
            format!("trade has no `{key}=`, so nothing says what it {key}s"),
            vec![format!(
                "write `{key}=` with the item's id, such as `{key}=emerald`"
            )],
        ))),
    }
}

/// How many items a trade's `key=` counts, 1 when it writes none; another value joins `unread`.
fn count_of(member: &Member, key: &'static str, unread: &mut Vec<UnreadArgument>) -> u8 {
    read_or_ignore(
        member,
        key,
        |kind| match kind {
            ValueKind::Int(count) if (1..=STACK).contains(count) => u8::try_from(*count).ok(),
            _ => None,
        },
        "a whole number from 1 to 64",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    })
    .unwrap_or(1)
}

/// The enchantment a trade's `enchant=` and `enchant_level=` put on the item it sells.
fn enchantment_of(
    member: &Member,
    unread: &mut Vec<UnreadArgument>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Enchantment> {
    let enchant = read_or_ignore(member, "enchant", id_value, AN_ID).unwrap_or_else(|argument| {
        unread.push(argument);
        None
    });
    let level = read_or_ignore(
        member,
        "enchant_level",
        |kind| match kind {
            ValueKind::Int(level) => u8::try_from(*level).ok().filter(|level| *level >= 1),
            _ => None,
        },
        "a whole number from 1 to 255",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    });
    if enchant.is_none()
        && level.is_some()
        && let Some(raw) = member.written("enchant_level")
    {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::IgnoredArgument,
            span: raw.span.clone(),
            primary: "trade `enchant_level=` is the level of an enchantment, and the line writes \
                      no `enchant=`"
                .into(),
            notes: vec![DiagnosticNote {
                span: None,
                message: "write `enchant=` with the enchantment's id, or drop `enchant_level=`; \
                          the item it sells carries no enchantment"
                    .into(),
            }],
            data: None,
        });
    }
    enchant.map(|id| Enchantment {
        id,
        level: level.unwrap_or(1),
    })
}
