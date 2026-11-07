- *(core)* `Member::intent_state` holds only the blockstate keys the member's keyword names, a
  `stair`'s `half`, `facing`, `shape` and `waterlogged`. Every other key the line writes moves to
  the new `Member::arguments`, an `Arguments` map, and `Member::written` and
  `Member::written_fields` read both. `MemberRole::state_arguments` names the keys that are
  blockstate. `Member::resolved_state` and the `ResolvedState` type are removed: lowering records
  what a member resolved to in `BlockArrayIr::members`, as `ResolvedMember` and `ResolvedBlock`.
- *(cli)* The IR a lock's `resolved_ir_hash` covers now carries that record, so the next build of
  any source writes a different `resolved_ir_hash` than its lock holds.
