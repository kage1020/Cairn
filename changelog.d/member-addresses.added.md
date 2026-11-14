- *(core)* Every member of a `struct` or `def` body gets an address from where it stands, never
  from the order its lines are written in: `window@front[0]`, `level[1].window@front[0]`. A segment
  is the member's keyword, the wall its `side=` or dotted `at=` names, and its place among the
  members of that keyword on that wall under the same parent, counted along the wall (`offset=`,
  or the column a door's `at=` names), then up (`y=`), then by the cell or box `at=`, `from=` and
  `to=` name. A member with an `id=` takes its place in the count as one without does; an actuator
  patch and a `site` row take no address. `IntentModule::addressed_members` lists every addressed
  member with the item it stands in, and `Member::address` holds it.
- *(cli)* `cairn parse --format json` writes each member line's `address`, and the ASCII dump of
  `cairn lower` lists each member that built a cell by its address, with its `id=` when it has one.
