- *(core)* With no registry pack, `W_ABSTRACT_TOKEN_DEFERRED` told every member whose slot binds an
  abstract token that "the cell falls back to air". It now says what the member does instead: a
  `floor` falls back to air; a `walls` is not built and takes up no rows; a `window` is not cut, so
  its wall stays and a port on it is refused; a `roof`, an eave `stair` or a `pressure_plate` is
  built from its default block.
- *(core)* A `roof kind=shed` with no usable `slope_to=` draws nothing, yet still resolved its
  `mat_slot=`. An unresolved binding drew a `W_DEFERRED_MEMBER` saying it falls back to
  `minecraft:spruce_stairs`, and with a pack a bad one raised an error such as
  `E_UNKNOWN_ABSTRACT_TOKEN` or `E_INCOMPATIBLE_MATERIAL`. It now resolves its material only once
  it will draw, as a `window` and an eave `stair` already did, so the `W_DEFERRED_MEMBER` saying
  why it is not drawn is the one finding lowering gives it.
