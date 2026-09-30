//! Which physical door an actuator patch acts on.
//!
//! `door[id=front] opened_by=sig.x` is written after the door it binds,
//! and its brackets pick that door by `id=`. Two passes need the answer:
//! block-array lowering, which recognises the patch and builds nothing
//! for it, and the redstone front end, which turns the binding into a
//! netlist port. Asked separately they disagreed — lowering deferred
//! `door[id=nope]` while the front end gave it a port and a pad — so both
//! read it from [`actuator_patch_target`].

use std::fmt;

use crate::intent::{Member, MemberRole};

/// Why an actuator patch's `[selector]` picks no physical door.
///
/// The [`fmt::Display`] form is the reason sentence both passes print, so
/// the two cannot describe the same line differently.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatchTargetError {
    /// The selector has no `id=` to pick a door by.
    MissingId,
    /// The `id=` value is not an identifier or a string. Carries the
    /// value's kind name, as `Value::kind_name` spells it.
    IdNotALabel(&'static str),
    /// No physical door in the scope carries this id.
    NoSuchDoor {
        /// The id the selector names.
        id: String,
        /// Every physical door id in the scope, in source order, each
        /// once.
        known: Vec<String>,
    },
    /// The id is declared on more than one physical door, so the patch
    /// has no single door to act on.
    Ambiguous {
        /// The id the selector names.
        id: String,
        /// How many physical doors carry it.
        count: u32,
    },
}

impl fmt::Display for PatchTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingId => f.write_str(
                "door actuator patch requires an `[id=<label>]` selector naming the physical door to bind against",
            ),
            Self::IdNotALabel(kind) => write!(
                f,
                "door actuator patch `[id=]` selector must be an identifier or string label, got {kind}",
            ),
            Self::NoSuchDoor { id, known } => {
                write!(
                    f,
                    "door actuator patch selects `id={id}` but no physical door with that id exists (",
                )?;
                if known.is_empty() {
                    f.write_str("no physical door members are declared in this scope")?;
                } else {
                    write!(f, "known door ids: {}", known.join(", "))?;
                }
                f.write_str(")")
            }
            Self::Ambiguous { id, count } => write!(
                f,
                "door actuator patch selects `id={id}` but the same id is declared on {count} physical doors in this scope; disambiguate the target before binding an actuator signal",
            ),
        }
    }
}

/// The one physical door `patch`'s `[selector]` picks out of `candidates`.
///
/// A physical door is a [`MemberRole::Door`] written without brackets; a
/// bracketed one is another patch, not something a patch can act on.
/// `candidates` is the scope's members as the caller sees them —
/// lowering passes its flattened view, so a door written under
/// `level y=N` is selectable. An id declared on two doors is
/// [`PatchTargetError::Ambiguous`] rather than "first hit wins", because
/// the two doors are equally what the author named.
///
/// Only the `id=` half of the selector is read. Whether the selector
/// carries anything else is the caller's question: lowering refuses such
/// a key, and in the redstone front end a binding written inside the
/// brackets is refused on its own.
///
/// # Errors
///
/// [`PatchTargetError`] when the selector has no readable `id=`, or when
/// that id is carried by no physical door or by more than one.
pub fn actuator_patch_target<'a>(
    patch: &Member,
    candidates: impl IntoIterator<Item = &'a Member>,
) -> Result<&'a Member, PatchTargetError> {
    let Some(id_value) = patch.selector.as_ref().and_then(|s| s.get("id")) else {
        return Err(PatchTargetError::MissingId);
    };
    let Some(id) = id_value.value.as_label_str() else {
        return Err(PatchTargetError::IdNotALabel(id_value.value.kind_name()));
    };
    let mut known: Vec<String> = Vec::new();
    let mut matched: Option<&'a Member> = None;
    let mut count: u32 = 0;
    for m in candidates {
        if !matches!(m.role, MemberRole::Door) || m.selector.is_some() {
            continue;
        }
        let Some(door_id) = m.id.as_deref() else {
            continue;
        };
        if !known.iter().any(|k| k == door_id) {
            known.push(door_id.to_owned());
        }
        if door_id == id {
            matched.get_or_insert(m);
            count = count.saturating_add(1);
        }
    }
    match matched {
        Some(door) if count == 1 => Ok(door),
        Some(_) => Err(PatchTargetError::Ambiguous {
            id: id.to_owned(),
            count,
        }),
        None => Err(PatchTargetError::NoSuchDoor {
            id: id.to_owned(),
            known,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::IntentModule;

    fn module(body: &str) -> IntentModule {
        let source = format!("struct s size=7x5\n{body}");
        crate::intent::lower(&crate::parse::parse(&source).expect("fixtures parse"))
    }

    /// Resolve the scope's last member, a patch, against every member
    /// before it.
    fn target_of(ir: &IntentModule) -> Result<&Member, PatchTargetError> {
        let members = &ir.structs[0].members;
        let (patch, rest) = members.split_last().expect("a patch");
        actuator_patch_target(patch, rest)
    }

    #[test]
    fn picks_the_one_door_that_carries_the_id() {
        let ir = module(concat!(
            "  door id=back side=back at=center\n",
            "  door id=front side=front at=center\n",
            "  door[id=front] opened_by=sig.a\n",
        ));
        let door = target_of(&ir).expect("resolves");
        assert_eq!(door.id.as_deref(), Some("front"));
        assert!(std::ptr::eq(door, &raw const ir.structs[0].members[1]));
    }

    /// A bracketed door is another patch, so even an `id=` written after
    /// its brackets is not a door the selector could pick.
    #[test]
    fn a_patch_is_not_a_target() {
        let ir = module(concat!(
            "  door[id=elsewhere] id=front opened_by=sig.b\n",
            "  door[id=front] opened_by=sig.a\n",
        ));
        assert_eq!(
            target_of(&ir),
            Err(PatchTargetError::NoSuchDoor {
                id: "front".into(),
                known: Vec::new(),
            }),
        );
    }

    #[test]
    fn lists_each_known_id_once_in_source_order() {
        let ir = module(concat!(
            "  door id=b side=back at=center\n",
            "  door id=a side=front at=center\n",
            "  door id=b side=left at=center\n",
            "  door[id=nope] opened_by=sig.a\n",
        ));
        let err = target_of(&ir).expect_err("no such door");
        assert_eq!(
            err,
            PatchTargetError::NoSuchDoor {
                id: "nope".into(),
                known: vec!["b".into(), "a".into()],
            },
        );
        assert_eq!(
            err.to_string(),
            "door actuator patch selects `id=nope` but no physical door with that id exists \
             (known door ids: b, a)",
        );
    }

    #[test]
    fn an_id_on_two_doors_is_ambiguous() {
        let ir = module(concat!(
            "  door id=front side=front at=center\n",
            "  door id=front side=back at=center\n",
            "  door[id=front] opened_by=sig.a\n",
        ));
        assert_eq!(
            target_of(&ir),
            Err(PatchTargetError::Ambiguous {
                id: "front".into(),
                count: 2,
            }),
        );
    }

    #[test]
    fn a_selector_without_a_readable_id_picks_nothing() {
        let ir =
            module("  door id=front side=front at=center\n  door[side=front] opened_by=sig.a\n");
        assert_eq!(target_of(&ir), Err(PatchTargetError::MissingId));
        let ir = module("  door id=front side=front at=center\n  door[id=3] opened_by=sig.a\n");
        assert_eq!(
            target_of(&ir),
            Err(PatchTargetError::IdNotALabel("integer"))
        );
    }
}
