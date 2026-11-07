//! One version's block ids, sorted, beside the states recorded for each, kept index for index.

use std::sync::Arc;

use super::BlockStates;

/// One version's ids, sorted and namespaced, and at the same index the states each one takes.
#[derive(Debug, Clone, Default)]
pub(super) struct VersionTable {
    ids: Vec<String>,
    states: Vec<Option<Arc<BlockStates>>>,
}

impl VersionTable {
    /// Where `id` is, or where it would go.
    pub(super) fn position(&self, id: &str) -> Result<usize, usize> {
        self.ids.binary_search_by(|probe| probe.as_str().cmp(id))
    }

    /// The ids, sorted.
    pub(super) fn ids(&self) -> &[String] {
        &self.ids
    }

    /// The states the id at `at` takes, `None` where the table records none.
    pub(super) fn states(&self, at: usize) -> Option<&BlockStates> {
        self.states[at].as_deref()
    }

    /// Puts `id` at `at`, the place [`Self::position`] gave for it.
    pub(super) fn insert(&mut self, at: usize, id: String, states: Option<&BlockStates>) {
        self.ids.insert(at, id);
        self.states.insert(at, states.cloned().map(Arc::new));
    }

    /// Drops the id at `at` and its states.
    pub(super) fn remove(&mut self, at: usize) {
        self.ids.remove(at);
        self.states.remove(at);
    }

    /// Gives the id at `at` the states `states`.
    pub(super) fn replace_states(&mut self, at: usize, states: &BlockStates) {
        self.states[at] = Some(Arc::new(states.clone()));
    }

    /// Whether the table declares no id.
    pub(super) fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}
