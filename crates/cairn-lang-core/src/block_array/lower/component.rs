//! Setting a `lever`, `button`, `daylight`, `observer`, `lamp`, `piston` or `dispenser` against
//! the wall it stands by (`spec/redstone` "Signal binding").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole};

use crate::block_array::BlockState;
use crate::block_array::args::{UnreadArgument, ident_or_ignore, read_or_ignore};

use super::canvas::{MemberCanvas, ScopePalette};
use super::fixture::{
    Fixture, Refusal, Toward, fixture_cell, outside_the_array, with_binding_note, written_facing,
};
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{MemberState, StructCtx, default_block_id, resolve_member_state};

/// A block the pack spells per edition: its catalog token, and the Java id used without a pack
/// (`spec/versioning-editions` "Backend = data tables").
#[derive(Debug, Clone, Copy)]
struct Spelled {
    /// The catalog token the pack spells it under.
    token: &'static str,
    /// Its Java id, Java being the base edition.
    base_id: &'static str,
}

/// How a component sits in its cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mount {
    /// Hung on the face of the wall cell it stands by, pointing away from that wall.
    OnTheWall,
    /// Standing in its cell, looking no way.
    Standing,
    /// Standing in its cell, looking away from the wall unless its `facing=` turns it.
    Facing,
}

/// One kind of component, as its line is built.
struct Component {
    fixture: Fixture,
    mount: Mount,
    /// The block it is built from when nothing chooses another.
    block: Spelled,
    /// The block `sticky=true` builds instead.
    sticky: Option<Spelled>,
    /// The family a `mat_slot=` has to name, on the one component that takes a material.
    material: Option<Family>,
}

const LEVER: Component = Component {
    fixture: Fixture {
        keyword: "lever",
        noun: "lever",
    },
    mount: Mount::OnTheWall,
    block: Spelled {
        token: "lever.default",
        base_id: "minecraft:lever",
    },
    sticky: None,
    material: None,
};

const BUTTON: Component = Component {
    fixture: Fixture {
        keyword: "button",
        noun: "button",
    },
    mount: Mount::OnTheWall,
    block: Spelled {
        token: "button.default",
        base_id: "minecraft:oak_button",
    },
    sticky: None,
    material: Some(Family::Button),
};

const DAYLIGHT: Component = Component {
    fixture: Fixture {
        keyword: "daylight",
        noun: "daylight detector",
    },
    mount: Mount::Standing,
    block: Spelled {
        token: "daylight.default",
        base_id: "minecraft:daylight_detector",
    },
    sticky: None,
    material: None,
};

const OBSERVER: Component = Component {
    fixture: Fixture {
        keyword: "observer",
        noun: "observer",
    },
    mount: Mount::Facing,
    block: Spelled {
        token: "observer.default",
        base_id: "minecraft:observer",
    },
    sticky: None,
    material: None,
};

const LAMP: Component = Component {
    fixture: Fixture {
        keyword: "lamp",
        noun: "lamp",
    },
    mount: Mount::Standing,
    block: Spelled {
        token: "lamp.default",
        base_id: "minecraft:redstone_lamp",
    },
    sticky: None,
    material: None,
};

const STICKY_PISTON: Spelled = Spelled {
    token: "piston.sticky",
    base_id: "minecraft:sticky_piston",
};

const PISTON: Component = Component {
    fixture: Fixture {
        keyword: "piston",
        noun: "piston",
    },
    mount: Mount::Facing,
    block: Spelled {
        token: "piston.default",
        base_id: "minecraft:piston",
    },
    sticky: Some(STICKY_PISTON),
    material: None,
};

const DISPENSER: Component = Component {
    fixture: Fixture {
        keyword: "dispenser",
        noun: "dispenser",
    },
    mount: Mount::Facing,
    block: Spelled {
        token: "dispenser.default",
        base_id: "minecraft:dispenser",
    },
    sticky: None,
    material: None,
};

impl Component {
    /// The component a member of `role` builds, or `None` for a role that is none of them.
    fn of(role: &MemberRole) -> Option<&'static Self> {
        Some(match role {
            MemberRole::Lever => &LEVER,
            MemberRole::Button => &BUTTON,
            MemberRole::Daylight => &DAYLIGHT,
            MemberRole::Observer => &OBSERVER,
            MemberRole::Lamp => &LAMP,
            MemberRole::Piston => &PISTON,
            MemberRole::Dispenser => &DISPENSER,
            _ => return None,
        })
    }
}

/// The `facing=` and `sticky=` a component's line writes, read before anything can refuse it.
#[derive(Debug, Clone, Copy)]
struct WrittenStates<'m> {
    facing: Option<&'m str>,
    sticky: bool,
}

/// Set a component's block in the cell its anchor names, turned as its mount says.
pub(super) fn fill_component(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(component) = Component::of(&member.role) else {
        debug_assert!(
            false,
            "fill_component called on a `{}`, which no component builds",
            member.role.keyword(),
        );
        return;
    };
    let noun = component.fixture.noun;
    // Read before anything can refuse the component, so every unreadable state is reported now.
    let mut unread: Vec<(UnreadArgument, String)> = Vec::new();
    let facing = if component.mount == Mount::Facing {
        ident_or_ignore(
            member,
            "facing",
            "`up`, `down`, `in`, `out`, `left` or `right`",
        )
        .unwrap_or_else(|argument| {
            unread.push((
                argument,
                format!("the {noun} faces away from the wall it stands by"),
            ));
            None
        })
    } else {
        None
    };
    let sticky = component.sticky.is_some()
        && read_or_ignore(
            member,
            "sticky",
            |kind| match kind {
                ValueKind::Bool(sticky) => Some(*sticky),
                _ => None,
            },
            "`true` or `false`",
        )
        .unwrap_or_else(|argument| {
            unread.push((
                argument,
                format!("the {noun} is built without a sticky head, as `sticky=false` builds it"),
            ));
            None
        })
        .unwrap_or(false);
    let written = WrittenStates { facing, sticky };
    let built = match place(
        component,
        member,
        written,
        y_offset,
        ctx,
        palette,
        canvas,
        diagnostics,
    ) {
        Ok(()) => true,
        Err(refusal) => {
            diagnostics.push(with_binding_note(component.fixture, member, refusal));
            false
        }
    };
    for (argument, consequence) in unread {
        diagnostics.push(argument.report(&if built {
            consequence
        } else {
            format!("this {noun} is not built either way — see the finding on the same line")
        }));
    }
}

/// [`fill_component`], answering the finding that refuses the component.
#[expect(
    clippy::too_many_arguments,
    reason = "the painter's context, plus the component and the states its line writes"
)]
fn place(
    component: &Component,
    member: &Member,
    written: WrittenStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), Refusal> {
    let fixture = component.fixture;
    let toward = written_facing(fixture, member, written.facing, &Toward::ALL)?;
    let (anchor, cell) = fixture_cell(fixture, member, y_offset, ctx)?;
    let block = match component.sticky {
        Some(sticky) if written.sticky => sticky,
        _ => component.block,
    };
    let fallback = || default_block_id(member, ctx.registry, block.token, block.base_id);
    let id = match component.material {
        None => fallback()?,
        Some(family) => {
            let resolved = match resolve_member_state(
                member,
                ctx.scope,
                ctx.registry,
                diagnostics,
                ctx.theme_missing,
            ) {
                MemberState::Bound(state) => Some(state),
                MemberState::Unbound => None,
                MemberState::Refused => return Ok(()),
            };
            geometry_material_id(
                member,
                ctx.scope,
                ctx.registry,
                resolved.as_ref(),
                fallback,
                &GeometryMemberDescription {
                    subject: fixture.keyword.to_owned(),
                    states_from: "the wall it hangs on",
                    requires: Some(family),
                },
                diagnostics,
            )?
        }
    };
    let side = anchor.side();
    let away = Toward::away_from_the_wall(anchor);
    let mut properties = IndexMap::new();
    match component.mount {
        Mount::OnTheWall => {
            properties.insert("face".to_owned(), "wall".to_owned());
            properties.insert("facing".to_owned(), away.face(side).as_str().to_owned());
        }
        Mount::Facing => {
            let facing = toward.unwrap_or(away).face(side);
            properties.insert("facing".to_owned(), facing.as_str().to_owned());
        }
        Mount::Standing => {}
    }
    let state = BlockState { id, properties };
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if canvas.try_paint(cell, || palette.intern(state)) {
        Ok(())
    } else {
        Err(Box::new(outside_the_array(fixture, member)))
    }
}
