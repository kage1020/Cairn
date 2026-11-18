- *(core,formats)* A canonical token is read through the target's registry pack: a meaning token
  picks the block a split made (`@cauldron[fluid=water]` as `water_cauldron`), and a token the
  pinned target does not declare is written as the target spells it before it is refused, a block
  renamed across the edition under the target's spelling (`@chain` as Java 26.3's `iron_chain`)
  and, on Bedrock, a Java block the state map writes as a block of its own as that block
  (`@oak_sign` as `standing_sign`, `@stone_bricks` at 1.21.0 as `stonebrick`). A check without a
  target and the language server weigh such a token as the block it is written as. Raw blocks and
  catalog tokens are written as before.
- *(formats)* Both built-in packs carry a hand-written `meanings` component, each canonical token
  whose block split into several with the meaning token that picks one; `PackView::respelling`
  says which id a pinned version writes a token under when it is not the token's own.
- *(cli,core)* `cairn info` names, per requested edition, each canonical token a version the floors
  admit writes under another id, as a `respelled` row and a `respelled` field of `--format json`,
  with a `note:` per id written that names the versions and the table that maps it.
