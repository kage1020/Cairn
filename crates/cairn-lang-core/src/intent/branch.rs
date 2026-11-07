//! The members each edition's build reads, by the `@edition` guards they stand under
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use std::borrow::Cow;

use crate::edition::Edition;

use super::{EditionGuard, IntentModule, Member};

impl IntentModule {
    /// The module as `edition`'s build reads it, with each guarded member kept only under its
    /// guard's edition, so `None` keeps none of them.
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
        !self.guarded_members().is_empty()
    }

    /// Every member under an `@edition` guard, at any depth, with its guard, in source order.
    #[must_use]
    pub fn guarded_members(&self) -> Vec<(&EditionGuard, &Member)> {
        let mut guarded = Vec::new();
        let lists = self
            .structs
            .iter()
            .map(|s| &s.members)
            .chain(self.defs.iter().map(|d| &d.members))
            .chain(self.sites.iter().map(|s| &s.placements));
        for members in lists {
            collect_guarded(members, &mut guarded);
        }
        guarded.sort_by_key(|(_, member)| member.span.start);
        guarded
    }
}

fn keep_branch(members: &mut Vec<Member>, edition: Option<Edition>) {
    members.retain(|member| {
        member
            .edition_guard
            .as_ref()
            .is_none_or(|guard| Some(guard.edition) == edition)
    });
    for member in members {
        keep_branch(&mut member.children.members, edition);
    }
}

fn collect_guarded<'a>(members: &'a [Member], into: &mut Vec<(&'a EditionGuard, &'a Member)>) {
    for member in members {
        if let Some(guard) = &member.edition_guard {
            into.push((guard, member));
        }
        collect_guarded(&member.children.members, into);
    }
}
