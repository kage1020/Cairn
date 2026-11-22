//! Standing a `text_display`, a `block_display` or an `item_display` on its cell's floor, and
//! building a `hologram` as the member form its slot binds (`spec/entities` "Displays").

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole};

use crate::block_array::args::{diag_invalid_member, read_or_ignore, strings_or_ignore};
use crate::block_array::{Billboard, EntityData, Item, StackCount};

use super::canvas::{MemberCanvas, ScopePalette};
use super::entity::{
    AN_ID, Hold, diag_absent_entity, facing_left, id_value, place, place_one, report_unread,
    scale_of, standing_point,
};
use super::fixture::{Fixture, Refusal};
use super::members::{MemberState, StructCtx, resolve_member_state};
use super::sign::fill_sign;

/// How a text display's findings name it.
const TEXT_DISPLAY: Fixture = Fixture {
    keyword: "text_display",
    noun: "text display",
};

/// How a block display's findings name it.
const BLOCK_DISPLAY: Fixture = Fixture {
    keyword: "block_display",
    noun: "block display",
};

/// How an item display's findings name it.
const ITEM_DISPLAY: Fixture = Fixture {
    keyword: "item_display",
    noun: "item display",
};

/// What becomes of an unread argument of an entity that is not built.
const NOT_BUILT: &str = "this entity is not built either way — see the finding on the same line";

/// What a display the target's edition lacks may be written as instead.
const DISPLAY_INSTEAD: &str = "the semantic layer is edition-neutral: write floating text as a \
                               `hologram` whose slot a `theme NAME_java` binds to `text_display` \
                               and a `theme NAME_bedrock` to `sign`, or place a block or an \
                               `item_frame`, which both editions have";

/// What a hologram the target's edition builds no display for binds instead.
const HOLOGRAM_INSTEAD: &str = "bind the hologram's slot to `sign` in this edition's theme, as \
                                `slot NAME -> sign glowing=true`, and to `text_display` in the \
                                other's";

/// Stand a `text_display` of its `text=` on its cell's floor.
pub(super) fn fill_text_display(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    show_text(member, y_offset, ctx, canvas, diagnostics, DISPLAY_INSTEAD);
}

/// [`fill_text_display`], `instead` saying what a target without displays takes.
fn show_text(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    instead: &str,
) {
    const ID: &str = "minecraft:text_display";
    if let Some(absent) = ctx.registry.and_then(|registry| registry.absent_entity(ID)) {
        diagnostics.push(diag_absent_entity(member, ID, &absent, instead.to_owned()));
        return;
    }
    let mut unread = Vec::new();
    let lines = strings_or_ignore(member, "text", "a string, or a list of strings, its lines")
        .unwrap_or_else(|argument| {
            unread.push(argument);
            None
        })
        .unwrap_or_default();
    let billboard = read_or_ignore(
        member,
        "billboard",
        |kind| match kind {
            ValueKind::Ident(word) => Billboard::ALL
                .iter()
                .copied()
                .find(|known| known.as_str() == word.as_str()),
            _ => None,
        },
        "one of `fixed`, `vertical`, `horizontal` and `center`",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    })
    .unwrap_or(Billboard::Fixed);
    let scale = scale_of(member, &mut unread);
    let built = match place(
        TEXT_DISPLAY,
        Hold::Stands,
        member,
        y_offset,
        ctx,
        &mut unread,
    ) {
        Ok(placed) => {
            place_one(
                member,
                canvas,
                placed,
                ID,
                standing_point(placed.cell),
                EntityData::TextDisplay {
                    facing: placed.compass(),
                    lines,
                    billboard,
                    scale,
                },
                &mut unread,
            );
            Some(placed)
        }
        Err(refusal) => {
            diagnostics.push(*refusal);
            None
        }
    };
    report_unread(unread, diagnostics, |key| match (built, key) {
        (None, _) => NOT_BUILT,
        (Some(_), "text") => "the display shows no text",
        (Some(_), "billboard") => "the display is fixed",
        (Some(_), "scale") => "the display is scale 1",
        (Some(placed), "facing") => facing_left(&placed),
        (Some(_), _) => "it is placed without it",
    });
}

/// Stand a `block_display` of the block its `mat_slot=` binds on its cell's floor.
pub(super) fn fill_block_display(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    const ID: &str = "minecraft:block_display";
    if let Some(absent) = ctx.registry.and_then(|registry| registry.absent_entity(ID)) {
        diagnostics.push(diag_absent_entity(
            member,
            ID,
            &absent,
            DISPLAY_INSTEAD.to_owned(),
        ));
        return;
    }
    let mut unread = Vec::new();
    let block = match resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    ) {
        MemberState::Bound(state) => Some(state),
        MemberState::Unbound | MemberState::Refused => None,
    };
    let scale = scale_of(member, &mut unread);
    let placed = place(
        BLOCK_DISPLAY,
        Hold::Stands,
        member,
        y_offset,
        ctx,
        &mut unread,
    );
    let built = match (placed, block) {
        (Ok(placed), Some(block)) => {
            place_one(
                member,
                canvas,
                placed,
                ID,
                standing_point(placed.cell),
                EntityData::BlockDisplay {
                    facing: placed.compass(),
                    block,
                    scale,
                },
                &mut unread,
            );
            Some(placed)
        }
        (Ok(_), None) => None,
        (Err(refusal), _) => {
            diagnostics.push(*refusal);
            None
        }
    };
    report_unread(unread, diagnostics, |key| match (built, key) {
        (None, _) => NOT_BUILT,
        (Some(_), "scale") => "the display is scale 1",
        (Some(placed), "facing") => facing_left(&placed),
        (Some(_), _) => "it is placed without it",
    });
}

/// Stand an `item_display` of its `item=` on its cell's floor.
pub(super) fn fill_item_display(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    const ID: &str = "minecraft:item_display";
    if let Some(absent) = ctx.registry.and_then(|registry| registry.absent_entity(ID)) {
        diagnostics.push(diag_absent_entity(
            member,
            ID,
            &absent,
            DISPLAY_INSTEAD.to_owned(),
        ));
        return;
    }
    let mut unread = Vec::new();
    let scale = scale_of(member, &mut unread);
    let built = match shown_item(member).and_then(|item| {
        let placed = place(
            ITEM_DISPLAY,
            Hold::Stands,
            member,
            y_offset,
            ctx,
            &mut unread,
        )?;
        Ok((item, placed))
    }) {
        Ok((item, placed)) => {
            place_one(
                member,
                canvas,
                placed,
                ID,
                standing_point(placed.cell),
                EntityData::ItemDisplay {
                    facing: placed.compass(),
                    item,
                    scale,
                },
                &mut unread,
            );
            Some(placed)
        }
        Err(refusal) => {
            diagnostics.push(*refusal);
            None
        }
    };
    report_unread(unread, diagnostics, |key| match (built, key) {
        (None, _) => NOT_BUILT,
        (Some(_), "scale") => "the display is scale 1",
        (Some(placed), "facing") => facing_left(&placed),
        (Some(_), _) => "it is placed without it",
    });
}

/// The item an item display's `item=` names, one of it, or the refusal of a line that names none.
fn shown_item(member: &Member) -> Result<Item, Refusal> {
    let written = member.written("item");
    match written.and_then(|raw| id_value(&raw.value.kind)) {
        Some(id) => Ok(Item {
            id,
            count: StackCount::ONE,
            enchantment: None,
        }),
        None => Err(Box::new(diag_invalid_member(
            member,
            match written {
                None => "item_display has no `item=`, so nothing says what it shows".into(),
                Some(raw) => format!(
                    "item_display `item=` must be {AN_ID}, not {}",
                    raw.value.describe()
                ),
            },
            vec!["write `item=` with the item's id, such as `item=diamond_sword`".into()],
        ))),
    }
}

/// Build a `hologram` as the member form its `mat_slot=` binds, its line's keys beside the
/// form's (`spec/versioning-editions` "Writing an alternative").
pub(super) fn fill_hologram(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if ctx.theme_missing {
        return;
    }
    // INVARIANT(upstream-diagnosed): a slot that binds nothing is reported where it is resolved.
    let Some(binding) = ctx
        .scope
        .and_then(|scope| scope.members.get(&member.span.start))
    else {
        return;
    };
    let Some(value) = binding.slot_value.as_ref() else {
        return;
    };
    let role = match &value.value.kind {
        ValueKind::Ident(form) if form == "text_display" => MemberRole::TextDisplay,
        ValueKind::Ident(form) if form == "sign" => MemberRole::Sign,
        // INVARIANT(upstream-diagnosed): `check_slot_targets` reports any other member form.
        ValueKind::Ident(_) => return,
        _ => {
            let slot = member.read_slot().unwrap_or_default();
            diagnostics.push(diag_invalid_member(
                member,
                format!(
                    "hologram `mat_slot={slot}` binds {}, a block, where a hologram is built as \
                     the member form its slot binds",
                    value.value.describe(),
                ),
                vec![format!(
                    "bind the slot to a member form in the theme: `slot {slot} -> text_display`, \
                     or `slot {slot} -> sign`"
                )],
            ));
            return;
        }
    };
    let mut realized = member.clone();
    realized.role = role;
    realized.mat_slot = None;
    for (key, form_value) in &binding.slot_arguments {
        realized.arguments.insert(key.clone(), form_value.clone());
    }
    match realized.role {
        MemberRole::TextDisplay => show_text(
            &realized,
            y_offset,
            ctx,
            canvas,
            diagnostics,
            HOLOGRAM_INSTEAD,
        ),
        _ => fill_sign(&realized, y_offset, ctx, palette, canvas, diagnostics),
    }
}
