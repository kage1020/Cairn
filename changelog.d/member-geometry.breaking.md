- *(core)* `ResolvedMember` has a new public field, `geometry`, so a struct literal of it must
  name one.
- *(core)* A window whose `anchor=` names a cell other than `bottom_left` is cut where that cell
  lands at `offset=` and `y=`; it was cut as if the key were absent, by its bottom-left cell.
- *(core)* A window's `anchor=` that names none of its nine cells, or that puts the window's first
  column before its wall's left end or its lowest row below the base row, and a roof's `footprint=`
  or `bounds=` other than `struct` or `expand`, are `E_INVALID_MEMBER`, and the member is not built.
  Each was `W_IGNORED_ARGUMENT` with the member built as if the key were absent.
  `MemberRole::unread_arguments` no longer lists the three keys.
- *(cli)* The IR a lock's `resolved_ir_hash` covers now carries each member's geometry, so the next
  build of any source writes a different `resolved_ir_hash` than its lock holds.
