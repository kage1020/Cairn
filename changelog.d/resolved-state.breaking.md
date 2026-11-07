- *(core)* `Member::intent_state` holds only the blockstate keys the member's keyword names, a
  `stair`'s `half`, `facing`, `shape` and `waterlogged`. Every other key the line writes moves to
  the new `Member::arguments`, an `Arguments` map, and `Member::written` and
  `Member::written_fields` read both. `MemberRole::state_arguments` names the keys that are
  blockstate. `IntentState` and `Arguments` keep their entries private and dereference to a map
  only for reading, so no caller can put a key in the wrong one. `Member::resolved_state` and the
  `ResolvedState` type are removed: lowering records what a member resolved to in
  `BlockArrayIr::members`, as `ResolvedMember` and `ResolvedBlock`.
- *(core)* `MemberRole::arguments` is renamed `MemberRole::own_keys`, so the role's vocabulary,
  blockstate keys included, no longer shares a name with `Member::arguments`, which holds none.
- *(cli)* The IR a lock's `resolved_ir_hash` covers now carries that record, so the next build of
  any source writes a different `resolved_ir_hash` than its lock holds.
