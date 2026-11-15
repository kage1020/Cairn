- *(core)* Every member of a `struct` or `def` body gets an address from where it stands and what
  its line writes, not from the order its lines are written in: `window@front[0]`,
  `level[1].window@front[0]`. A segment is the member's keyword, the wall its `side=` or dotted
  `at=` names, and its place among the members of that keyword on that wall under the same parent,
  counted along the wall (`offset=`, or the column an `at=` door anchor names), then up (`y=`), then
  by the cell a coordinate `at=` names or the box `from=` and `to=` span, and last by what the lines
  write other than `id=`, `class=` and a `[selector]`, compared as text. Adding an `id=`, a
  `class=` or a `[selector]` renumbers nothing, and an actuator patch and a `site` row take no
  address. `Member::address` holds it, and `IntentModule::addressed_members` lists every addressed
  member as an `AddressedMember`, with the item it stands in.
- *(cli)* `cairn parse --format json` writes each member line's `address`, and the ASCII dump of
  `cairn lower` lists each member that built a cell by its address, with its `id=` when it has one.
