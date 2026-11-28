- *(core)* The `requires` floors a build inherits from its defs were read off the `place use=`
  lines as written, so an `edit` that set a body placement's `use=` changed which def was built
  but not which floor applied: `compile` built a target the edited def rules out and `info` listed
  it as buildable, or a def edited away still refused the build. The defs a body places are now
  read after every edit, and `E_VERSION_CAP` adds a note at the edit that placed the floored def.
