//! Which physical member an actuator patch acts on.
//!
//! `door[id=front] opened_by=sig.x` is written on a line of its own,
//! before or after the door it binds, and its brackets pick that door by
//! `id=`. Two passes need the answer: block-array lowering, which
//! recognises the patch and builds nothing for it, and the redstone front
//! end, which turns the binding into a netlist port. Asked separately they
//! disagreed — lowering deferred `door[id=nope]` while the front end gave
//! it a port and a pad — so both read it from [`actuator_patch_target`].
//!
//! Beside [`Member`] rather than in `block_array`, because which member a
//! selector picks is a fact about the member lines and touches no voxel —
//! the reason [`super::SENSOR_HOSTS`] sits in the keyword table.

use std::fmt;

use super::{Member, MemberRole};

/// Why an actuator patch's `[selector]` picks no physical member.
///
/// The [`fmt::Display`] form is the reason clause both passes print, so
/// the two cannot describe the same line differently. It states what is
/// wrong and stops there: the repair depends on what the caller can offer
/// the author, so each caller writes its own.
///
/// Every variant carries `keyword`, the keyword of the patch line, which is
/// also the role of the members it may pick.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatchTargetError {
    /// The selector has no `id=` to pick a member by.
    #[non_exhaustive]
    MissingId {
        /// The patch line's keyword.
        keyword: String,
    },
    /// The `id=` value is not an identifier or a string.
    #[non_exhaustive]
    IdNotALabel {
        /// The patch line's keyword.
        keyword: String,
        /// The value's kind name, as `Value::kind_name` spells it.
        kind: &'static str,
    },
    /// No physical member of the patch's role carries this id.
    #[non_exhaustive]
    NoSuchId {
        /// The patch line's keyword.
        keyword: String,
        /// The id the selector names.
        id: String,
        /// Every id a physical member of the role carries in the scope,
        /// in source order, each once.
        known: Vec<String>,
        /// How many physical members of the role carry no `id=` at all.
        /// The selector can never pick one, and adding the id to one is
        /// the repair when `known` is empty.
        unlabelled: u32,
    },
    /// The id is declared on more than one physical member of the role, so
    /// the patch has no single member to act on.
    #[non_exhaustive]
    Ambiguous {
        /// The patch line's keyword.
        keyword: String,
        /// The id the selector names.
        id: String,
        /// How many physical members carry it; at least 2.
        count: u32,
    },
}

impl PatchTargetError {
    /// Stable machine-readable name for this failure, for a consumer
    /// choosing a repair without reading the prose.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MissingId { .. } => "missing_id",
            Self::IdNotALabel { .. } => "id_not_a_label",
            Self::NoSuchId { .. } => "no_such_id",
            Self::Ambiguous { .. } => "ambiguous",
        }
    }

    /// The patch line's keyword, which is also the role of the members it
    /// may pick.
    #[must_use]
    pub fn keyword(&self) -> &str {
        match self {
            Self::MissingId { keyword }
            | Self::IdNotALabel { keyword, .. }
            | Self::NoSuchId { keyword, .. }
            | Self::Ambiguous { keyword, .. } => keyword,
        }
    }
}

impl fmt::Display for PatchTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingId { keyword } => write!(
                f,
                "{keyword} actuator patch has no `[id=<label>]` selector naming the physical \
                 {keyword} to bind against",
            ),
            Self::IdNotALabel { keyword, kind } => write!(
                f,
                "{keyword} actuator patch `[id=]` selector must be an identifier or string \
                 label, got {kind}",
            ),
            Self::NoSuchId {
                keyword,
                id,
                known,
                unlabelled,
            } => {
                write!(
                    f,
                    "{keyword} actuator patch selects `id={id}` but no physical {keyword} with \
                     that id exists (",
                )?;
                match (known.is_empty(), *unlabelled) {
                    (true, 0) => write!(
                        f,
                        "no physical {keyword} members are declared in this scope"
                    )?,
                    (true, 1) => write!(
                        f,
                        "1 physical {keyword} is declared in this scope, without an `id=`"
                    )?,
                    (true, n) => write!(
                        f,
                        "{n} physical {keyword}s are declared in this scope, none with an `id=`"
                    )?,
                    (false, n) => {
                        write!(f, "known {keyword} ids: {}", known.join(", "))?;
                        if n > 0 {
                            write!(f, "; {n} more without an `id=`")?;
                        }
                    }
                }
                f.write_str(")")
            }
            Self::Ambiguous { keyword, id, count } => write!(
                f,
                "{keyword} actuator patch selects `id={id}` but the same id is declared on \
                 {count} physical {keyword}s in this scope",
            ),
        }
    }
}

/// The one physical member `patch`'s `[selector]` picks out of
/// `candidates`.
///
/// A physical member is one of the patch's own role written without
/// brackets — `door[id=front]` picks among `door` lines — and a bracketed
/// one is another patch, not something a patch can act on. `candidates`
/// is the scope's members as the caller sees them — lowering passes its
/// flattened view, so a door written under `level y=N` is selectable.
/// Where the patch line sits among them does not matter. An id declared on
/// two members is [`PatchTargetError::Ambiguous`] rather than "first hit
/// wins", because the two are equally what the author named.
///
/// Only the `id=` half of the selector is read. Whether the selector
/// carries anything else is the caller's question: lowering refuses such
/// a key, and in the redstone front end a binding written inside the
/// brackets is refused on its own.
///
/// # Errors
///
/// [`PatchTargetError`] when the selector has no readable `id=`, or when
/// that id is carried by no physical member of the role or by more than
/// one.
pub fn actuator_patch_target<'a>(
    patch: &Member,
    candidates: impl IntoIterator<Item = &'a Member>,
) -> Result<&'a Member, PatchTargetError> {
    let keyword = || patch.role.keyword().to_owned();
    let Some(id_value) = patch.selector.as_ref().and_then(|s| s.get("id")) else {
        return Err(PatchTargetError::MissingId { keyword: keyword() });
    };
    let Some(id) = id_value.value.as_label_str() else {
        return Err(PatchTargetError::IdNotALabel {
            keyword: keyword(),
            kind: id_value.value.kind_name(),
        });
    };
    let mut known: Vec<String> = Vec::new();
    let mut unlabelled: u32 = 0;
    let mut matched: Option<&'a Member> = None;
    let mut count: u32 = 0;
    for m in candidates {
        if !is_physical(m, &patch.role) {
            continue;
        }
        let Some(member_id) = m.id.as_deref() else {
            unlabelled = unlabelled.saturating_add(1);
            continue;
        };
        if !known.iter().any(|k| k == member_id) {
            known.push(member_id.to_owned());
        }
        if member_id == id {
            matched.get_or_insert(m);
            count = count.saturating_add(1);
        }
    }
    match matched {
        Some(member) if count == 1 => Ok(member),
        Some(_) => Err(PatchTargetError::Ambiguous {
            keyword: keyword(),
            id: id.to_owned(),
            count,
        }),
        None => Err(PatchTargetError::NoSuchId {
            keyword: keyword(),
            id: id.to_owned(),
            known,
            unlabelled,
        }),
    }
}

/// Whether `m` is a member a patch of `role` can act on: that role, and
/// declared rather than patched.
fn is_physical(m: &Member, role: &MemberRole) -> bool {
    m.role == *role && m.selector.is_none()
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

    fn door() -> String {
        "door".to_owned()
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

    /// A patch written above its door picks it the same way: the lookup
    /// reads the whole scope, not the lines before the patch.
    #[test]
    fn a_patch_above_its_door_picks_it() {
        let ir = module(concat!(
            "  door[id=front] opened_by=sig.a\n",
            "  door id=front side=front at=center\n",
        ));
        let members = &ir.structs[0].members;
        let door = actuator_patch_target(&members[0], members).expect("resolves");
        assert!(std::ptr::eq(door, &raw const members[1]));
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
            Err(PatchTargetError::NoSuchId {
                keyword: door(),
                id: "front".into(),
                known: Vec::new(),
                unlabelled: 0,
            }),
        );
    }

    /// The patch's own keyword is the role it picks among: a member of
    /// another role carrying the id is not a candidate.
    #[test]
    fn only_members_of_the_patch_role_are_candidates() {
        let ir = module(concat!(
            "  pressure_plate id=front at=front.outside offset=0 y=0\n",
            "  door[id=front] opened_by=sig.a\n",
        ));
        let err = target_of(&ir).expect_err("no door carries the id");
        assert_eq!(err.kind(), "no_such_id");
        assert_eq!(
            err.to_string(),
            "door actuator patch selects `id=front` but no physical door with that id exists \
             (no physical door members are declared in this scope)",
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
            PatchTargetError::NoSuchId {
                keyword: door(),
                id: "nope".into(),
                known: vec!["b".into(), "a".into()],
                unlabelled: 0,
            },
        );
        assert_eq!(
            err.to_string(),
            "door actuator patch selects `id=nope` but no physical door with that id exists \
             (known door ids: b, a)",
        );
    }

    /// A door without an `id=` is still a door: the reason counts it
    /// rather than saying none is declared.
    #[test]
    fn counts_doors_that_carry_no_id() {
        let ir = module(concat!(
            "  door side=front at=center\n",
            "  door[id=front] opened_by=sig.a\n",
        ));
        let err = target_of(&ir).expect_err("no door carries the id");
        assert_eq!(
            err.to_string(),
            "door actuator patch selects `id=front` but no physical door with that id exists \
             (1 physical door is declared in this scope, without an `id=`)",
        );
        let ir = module(concat!(
            "  door side=front at=center\n",
            "  door side=back at=center\n",
            "  door[id=front] opened_by=sig.a\n",
        ));
        assert_eq!(
            target_of(&ir)
                .expect_err("no door carries the id")
                .to_string(),
            "door actuator patch selects `id=front` but no physical door with that id exists \
             (2 physical doors are declared in this scope, none with an `id=`)",
        );
    }

    /// Beside a labelled door, an unlabelled one is mentioned after the
    /// known ids rather than leaving no trace.
    #[test]
    fn mentions_unlabelled_doors_beside_known_ids() {
        let ir = module(concat!(
            "  door side=front at=center\n",
            "  door id=other side=back at=center\n",
            "  door[id=front] opened_by=sig.a\n",
        ));
        assert_eq!(
            target_of(&ir)
                .expect_err("no door carries the id")
                .to_string(),
            "door actuator patch selects `id=front` but no physical door with that id exists \
             (known door ids: other; 1 more without an `id=`)",
        );
    }

    #[test]
    fn an_id_on_two_doors_is_ambiguous() {
        let ir = module(concat!(
            "  door id=front side=front at=center\n",
            "  door id=front side=back at=center\n",
            "  door[id=front] opened_by=sig.a\n",
        ));
        let err = target_of(&ir).expect_err("two doors carry the id");
        assert_eq!(
            err,
            PatchTargetError::Ambiguous {
                keyword: door(),
                id: "front".into(),
                count: 2,
            },
        );
        assert_eq!(err.kind(), "ambiguous");
        assert_eq!(
            err.to_string(),
            "door actuator patch selects `id=front` but the same id is declared on 2 physical \
             doors in this scope",
        );
    }

    #[test]
    fn a_selector_without_a_readable_id_picks_nothing() {
        let ir =
            module("  door id=front side=front at=center\n  door[side=front] opened_by=sig.a\n");
        let err = target_of(&ir).expect_err("no id");
        assert_eq!(err, PatchTargetError::MissingId { keyword: door() });
        assert_eq!(err.kind(), "missing_id");
        let ir = module("  door id=front side=front at=center\n  door[id=3] opened_by=sig.a\n");
        let err = target_of(&ir).expect_err("not a label");
        assert_eq!(
            err,
            PatchTargetError::IdNotALabel {
                keyword: door(),
                kind: "integer",
            },
        );
        assert_eq!(err.kind(), "id_not_a_label");
        assert_eq!(err.keyword(), "door");
    }
}
