//! The members each edition's build reads, by the `@edition` guards they stand under
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use std::borrow::Cow;
use std::collections::HashSet;

use crate::ast::ValueKind;
use crate::edition::Edition;
use crate::error::Span;

use super::{IntentModule, Member, MemberRole};

impl IntentModule {
    /// The module as `edition`'s build reads it, with each guarded member kept only under its
    /// guard's edition, so `None` keeps none of them, and each `trade` of a villager it drops
    /// dropped with it.
    #[must_use]
    pub fn branch_for(&self, edition: Option<Edition>) -> Cow<'_, Self> {
        if !self.has_edition_guards() {
            return Cow::Borrowed(self);
        }
        let mut branch = self.clone();
        for s in &mut branch.structs {
            keep_branch(&mut s.members, edition);
        }
        for d in &mut branch.defs {
            keep_branch(&mut d.members, edition);
        }
        for s in &mut branch.sites {
            keep_branch(&mut s.placements, edition);
        }
        Cow::Owned(branch)
    }

    /// Whether any member, at any depth, stands under an `@edition` guard.
    #[must_use]
    pub fn has_edition_guards(&self) -> bool {
        self.any_member(&|member| member.edition_guard.is_some())
    }

    /// Whether any member of a struct, def or site, at any depth, is one `holds` holds of.
    #[must_use]
    pub fn any_member(&self, holds: &dyn Fn(&Member) -> bool) -> bool {
        self.member_lists()
            .any(|members| any_member_of(members, holds))
    }

    /// The lines of the members under an `@edition` guard for `edition`, which only its build reads.
    #[must_use]
    pub fn lines_guarded_for(&self, edition: Edition) -> Vec<&Span> {
        self.guarded_members()
            .into_iter()
            .filter(|(guard, _)| *guard == edition)
            .map(|(_, member)| &member.span)
            .collect()
    }

    /// Every member under an `@edition` guard, at any depth, with its guard, in source order.
    #[must_use]
    pub fn guarded_members(&self) -> Vec<(Edition, &Member)> {
        let mut guarded = Vec::new();
        for members in self.member_lists() {
            collect_guarded(members, &mut guarded);
        }
        guarded.sort_by_key(|(_, member)| member.span.start);
        guarded
    }

    /// The top-level member list of every struct, def and site.
    fn member_lists(&self) -> impl Iterator<Item = &Vec<Member>> {
        self.structs
            .iter()
            .map(|s| &s.members)
            .chain(self.defs.iter().map(|d| &d.members))
            .chain(self.sites.iter().map(|s| &s.placements))
    }
}

fn any_member_of(members: &[Member], holds: &dyn Fn(&Member) -> bool) -> bool {
    members
        .iter()
        .any(|member| holds(member) || any_member_of(&member.children.members, holds))
}

fn keep_branch(members: &mut Vec<Member>, edition: Option<Edition>) {
    let mut dropped = Vec::new();
    drop_other_branches(members, edition, &mut dropped);
    let mut kept = Vec::new();
    collect_villager_ids(members, &mut kept);
    let traded_by_dropped: HashSet<String> = dropped
        .into_iter()
        .filter(|id| !kept.contains(id))
        .collect();
    if !traded_by_dropped.is_empty() {
        drop_trades_of(members, &traded_by_dropped);
    }
}

/// Drop each member of `members` under another edition's guard, at any depth, adding the `id=` of
/// each villager among them to `dropped`.
fn drop_other_branches(
    members: &mut Vec<Member>,
    edition: Option<Edition>,
    dropped: &mut Vec<String>,
) {
    members.retain(|member| {
        let keep = member
            .edition_guard
            .is_none_or(|guard| Some(guard) == edition);
        if !keep {
            collect_villager_ids(std::slice::from_ref(member), dropped);
        }
        keep
    });
    for member in members {
        drop_other_branches(&mut member.children.members, edition, dropped);
    }
}

fn collect_villager_ids(members: &[Member], into: &mut Vec<String>) {
    for member in members {
        if matches!(member.role, MemberRole::Villager)
            && let Some(id) = &member.id
        {
            into.push(id.clone());
        }
        collect_villager_ids(&member.children.members, into);
    }
}

fn drop_trades_of(members: &mut Vec<Member>, villagers: &HashSet<String>) {
    members.retain(|member| {
        !(matches!(member.role, MemberRole::Trade)
            && matches!(
                member.written("villager").map(|raw| &raw.value.kind),
                Some(ValueKind::Ident(name)) if villagers.contains(name)
            ))
    });
    for member in members {
        drop_trades_of(&mut member.children.members, villagers);
    }
}

fn collect_guarded<'a>(members: &'a [Member], into: &mut Vec<(Edition, &'a Member)>) {
    for member in members {
        if let Some(guard) = member.edition_guard {
            into.push((guard, member));
        }
        collect_guarded(&member.children.members, into);
    }
}
