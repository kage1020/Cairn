- *(formats)* The Bedrock writer maps every block's states, not only a stair's. The built-in Bedrock
  pack carries a state map per version, `states.json`, derived from GeyserMC's Java-to-Bedrock
  mappings and checked against that version's palette, and a hand-written `sensitivity.json` that
  says what dropping a state no Bedrock state reads costs. `@oak_log[axis=x]` is written as
  `pillar_axis=x`, a double `oak_slab` as `oak_double_slab`, and a Bedrock spelling such as
  `wooden_door` reads the states of the Java block mapped onto it, writing a door's `facing` as
  `direction` before 1.21.60 and as `minecraft:cardinal_direction` from it. Stairs keep their corner
  `shape` at 1.26.60, as `minecraft:corner`, and drop it with `W_INTENT_DEGRADED` only before it. A
  property an entry leaves out takes its Java block's default. `waterlogged=true` fills the
  `.mcstructure`'s second layer with water, and so does a block that always stands in water, such as
  kelp or seagrass.
- *(core)* `E_STATE_UNMAPPED` refuses, at a pinned target, a block state a member or a walkway's
  `path=` built that the target's state map has no form for, such as `@cherry_wood[axis=x]` at
  Bedrock 1.21.40, where the writer would otherwise fail on it. A block whose pair the version's
  palette refuses is refused even written bare. The note names the targetable versions whose map
  has a form for the state. `cairn check --target`, `cairn compile`
  and the `buildable targets` row of `cairn info` raise it, and a `TargetRegistry` answers it
  through `unmapped_state`.
- *(cli)* Each `dropped` intent in `cairn info --format json` names the versions that lose it,
  `mc_versions`, and the note on stderr lists them.
