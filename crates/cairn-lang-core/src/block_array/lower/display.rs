//! Standing a `text_display`, a `block_display` or an `item_display` on its cell's floor, and
//! building a `hologram` as the member form its slot binds (`spec/entities` "Displays").

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole};

use crate::block_array::EntityData;
use crate::block_array::args::{diag_invalid_member, read_or_ignore, strings_or_ignore};

use super::canvas::{MemberCanvas, ScopePalette};
use super::entity::{
    Hold, diag_absent_entity, item_of, place, place_one, report_unread, scale_of, standing_point,
};
use super::fixture::Fixture;
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

/// The ways a text display's `billboard=` turns it toward the viewer, its default first.
pub(super) const BILLBOARDS: [&str; 4] = ["fixed", "vertical", "horizontal", "center"];

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
            ValueKind::Ident(word) => BILLBOARDS.iter().find(|known| **known == word.as_str()),
            _ => None,
        },
        "one of `fixed`, `vertical`, `horizontal` and `center`",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    })
    .copied()
    .unwrap_or(BILLBOARDS[0]);
    let scale = scale_of(member, &mut unread);
    let built = match place(TEXT_DISPLAY, Hold::Stands, member, y_offset, ctx, &mut unread) {
        Ok(placed) => {
            place_one(
                member,
                canvas,
                placed,
                ID,
                standing_point(placed.cell),
                EntityData::TextDisplay {
                    lines,
                    billboard: billboard.to_owned(),
                    scale,
                },
                &mut unread,
            );
            true
        }
        Err(refusal) => {
            diagnostics.push(*refusal);
            false
        }
    };
    report_unread(unread, diagnostics, |key| match (built, key) {
        (false, _) => NOT_BUILT,
        (true, "text") => "the display shows no text",
        (true, "billboard") => "the display is fixed",
        (true, "scale") => "the display is scale 1",
        (true, "facing") => "it faces south",
        (true, _) => "it is placed without it",
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
    let placed = place(BLOCK_DISPLAY, Hold::Stands, member, y_offset, ctx, &mut unread);
    let built = match (placed, block) {
        (Ok(placed), Some(block)) => {
            place_one(
                member,
                canvas,
                placed,
                ID,
                standing_point(placed.cell),
                EntityData::BlockDisplay { block, scale },
                &mut unread,
            );
            true
        }
        (Ok(_), None) => false,
        (Err(refusal), _) => {
            diagnostics.push(*refusal);
            false
        }
    };
    report_unread(unread, diagnostics, |key| match (built, key) {
        (false, _) => NOT_BUILT,
        (true, "scale") => "the display is scale 1",
        (true, "facing") => "it faces south",
        (true, _) => "it is placed without it",
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
    let item = item_of(member, "item", &mut unread);
    let scale = scale_of(member, &mut unread);
    let built = match (
        place(ITEM_DISPLAY, Hold::Stands, member, y_offset, ctx, &mut unread),
        item,
    ) {
        (Ok(placed), Some(item)) => {
            place_one(
                member,
                canvas,
                placed,
                ID,
                standing_point(placed.cell),
                EntityData::ItemDisplay { item, scale },
                &mut unread,
            );
            true
        }
        (Ok(_), None) => {
            if member.written("item").is_none() {
                diagnostics.push(diag_invalid_member(
                    member,
                    "item_display has no `item=`, so nothing says what it shows".into(),
                    vec!["write `item=` with the item's id, such as `item=diamond_sword`".into()],
                ));
            }
            false
        }
        (Err(refusal), _) => {
            diagnostics.push(*refusal);
            false
        }
    };
    report_unread(unread, diagnostics, |key| match (built, key) {
        (false, _) => NOT_BUILT,
        (true, "scale") => "the display is scale 1",
        (true, "facing") => "it faces south",
        (true, _) => "it is placed without it",
    });
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
