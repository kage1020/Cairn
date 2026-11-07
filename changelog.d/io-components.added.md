- *(core)* `lever`, `button`, `daylight` and `observer` are sensors and `lamp`, `piston` and
  `dispenser` actuators. Each sets one block by a wall, anchored with the `at=`, `offset=` and `y=`
  of a `pressure_plate` and placed, refused or deferred wherever the plate would be. A lever or
  button hangs on its wall cell facing away from it. An observer, piston or dispenser faces away
  from its wall unless `facing=up|down|in|out|left|right` turns it, and `facing` is recorded in its
  `intent_state`; another word is `E_INVALID_MEMBER`, and a compass direction gets a note naming
  the word that faces that way from its wall. `sticky=true` builds a sticky piston. Every one is
  built unpowered.
- *(core)* A `button` reads `mat_slot=`, which must bind a `*_button` block, else
  `E_INCOMPATIBLE_MATERIAL` with `required: "button"`, and is built from `@button.default` without
  one. The other six take no material, so `mat_slot=` on them is `W_IGNORED_ARGUMENT`.
- *(core)* `lamp[id=…] lit_by=`, `piston[id=…] powered_by=` and `dispenser[id=…] fired_by=` are
  actuator patches, as `door[id=…] opened_by=` is. A component that is not placed says in a note
  that its binding still reaches the netlist.
- *(redstone)* Each new sensor's `->` tail feeds an input pad and each actuator key reads an output
  pad, and `E_LOGIC_UNRESOLVED_PATCH` and `E_LOGIC_DUPLICATE_BINDING` hold for every actuator.
- *(formats)* Both built-in packs declare the tier-1 tokens `lever.default`, `button.default`,
  `daylight.default`, `observer.default`, `lamp.default`, `piston.default`, `piston.sticky` and
  `dispenser.default`, and a Bedrock build writes each component's states through the state map.
- *(vscode)* The grammar highlights the seven keywords.
- `examples/control-room.crn` sets every sensor and actuator against the walls of one room and
  wires them through the logic layer.
