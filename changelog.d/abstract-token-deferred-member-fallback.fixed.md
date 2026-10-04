- *(core)* With no registry pack, `W_ABSTRACT_TOKEN_DEFERRED` told every member whose slot binds an
  abstract token that "the cell falls back to air", which is true only of `floor` and `walls`. A
  `window` is not cut and its wall stays, and a `roof`, an eave `stair` or a `pressure_plate` is
  built from its default material; the warning now says which. What is built is unchanged.
