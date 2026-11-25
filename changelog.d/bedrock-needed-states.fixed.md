- *(formats)* A Bedrock import writes a state at its default when leaving it out builds another
  block, as one Bedrock block can be the form of several Java blocks. A redstone torch, torch or
  soul torch on a wall facing north, a wall head or wall hanging sign facing north, a stem attached
  to the north, the lower half of tall seagrass, and at 1.21.0 an upright quartz or purpur pillar
  and a `skull` facing north were written by their id alone, under a `W_SEMANTIC_LOSS` that said
  they had no canonical state, and the import rebuilt a standing torch, a floor head or a plain
  block in their place. They are now written as `minecraft:redstone_torch[facing=north]` and the
  like, and rebuild as the file holds them, so `crossbar` and `control-room` survive a round trip
  at every Bedrock target.
- *(formats)* `cairn diff-blocks` scores a Bedrock structure 1.0 against the source that built it,
  where such a block counted as differing states and the suggested patch turned it into the other
  block. A lift that leaves out a state its block needs now differs from the file, since it builds
  the other block.
