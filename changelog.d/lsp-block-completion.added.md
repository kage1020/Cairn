- *(lsp)* An `@` token completes from every canonical id a built-in pack's blocks table declares,
  Java's and Bedrock's, so an id no materials catalog maps to, such as `@border_block`, is offered
  as well. `E_UNKNOWN_ID` still checks a pinned target against its own version's table, so an id
  offered can be unknown there. Each id's detail names the editions and versions that declare it,
  such as `java 1.21.4, 26.3; bedrock 1.21.60, 1.26.60` for `pale_oak_planks`, and each abstract
  token's detail names what it resolves to in each edition, overrides included. Only the tokens
  matching what is typed before the cursor are offered: those starting with it, then those that do
  after a `_` or `.`, then those holding its characters in order, so `@fwb` finds
  `floor.wood.broadleaf`. The server answers the catalogs' abstract tokens first and cuts the
  answer at 100, marking it incomplete so the editor asks again as the token grows; an editor that
  sorts by its own match score, as VS Code does, may show them in another order.
- *(formats)* `BlocksIndex::ids_by_version` yields each version an index describes with its ids.
