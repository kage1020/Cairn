- *(core)* A top-level `edit TARGET set key=value …` line changes the members of `struct` and
  `def` bodies before lowering, without rewriting their lines. The target is a selector in the
  grammar of a theme's selector rows, `window[class=vent][level=floor2]`, or an address,
  `window@front[0]`. Each edit selects among the members as their lines write them, the edits
  apply in source order, and each key lands where the member's line would have put it, never in
  `resolved_state`. An edit reaches no `site` row and no actuator patch. Every later pass reads
  the edited members, and a lowering finding on the line of a member an edit changed notes each
  edit that changed it.
- *(core)* `E_EDIT_UNMATCHED` refuses an edit that selects no member, naming the addresses its
  keyword's members carry when the target is an address. `E_EDIT_UNKNOWN_KEY` refuses a key the
  edited keyword does not take, and `id=`, without dropping the edit's other keys.
- *(core)* `W_EDIT_RIPPLE` reports a member no edit selects whose `resolved_state` the edits
  change, such as the chest an edit parts from its pair or the roof that taller walls lift, with
  the first cell that changed and the edits behind it. Commands that lower with a registry pack
  see connection changes too; `cairn check` without `--target` and the language server see the
  changes in where blocks stand.
- *(core)* A theme selector row may chain its brackets, `window[class=small][side=front]`, and
  select by `level=NAME`, the `level` a member stands under.
- *(cli)* `cairn parse --format json` lists each `edit` line with its target and the keys it sets.
- *(lsp)* Completion offers `edit` at the start of a top-level line, the member keywords after it,
  `set` after its target, and the slots of the edited keyword after `mat_slot=`.
- *(tree-sitter)* The grammar parses `edit` lines, with `member_address` and `address_step`
  nodes, and highlights `edit` and `set` as keywords; the VS Code grammar highlights them too.
