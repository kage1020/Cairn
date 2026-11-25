- *(formats)* A Bedrock import writes a state at its default when a line that leaves it out does
  not build the block, as one Bedrock block can be the form of several Java blocks. A redstone
  torch, torch or soul torch on a wall facing north, a wall head or wall hanging sign facing north,
  a stem attached to the north, the lower half of tall seagrass, and at 1.21.0 an upright quartz or
  purpur pillar and a `skull` on a wall facing north or on the floor were written by their id
  alone, under a `W_SEMANTIC_LOSS` that said they had no canonical state, and the import rebuilt a
  standing torch, a floor head, a plain block or a `skull` with no state in their place. They are
  now written as `minecraft:redstone_torch[facing=north]`, `minecraft:skull[rotation=0]` and the
  like, and rebuild as the file holds them, so `crossbar` and `control-room` survive a round trip
  at every Bedrock target. A line keeps the fewest such states that build its block; of a block
  with more than twelve states it would leave out, only the first 4096 sets are tried, and past
  them it keeps every one.
- *(formats)* `cairn diff-blocks` compares such a block in a Bedrock structure with the source that
  built it as agreeing, where it counted as differing states and the suggested patch turned it into
  the other block, so an example's Bedrock file now scores 1.0 in block IoU and state accuracy
  against the example. A lift that leaves out a state its block needs differs from the file, since
  it does not build that block, while one Bedrock does not read, such as a floor head's `rotation`
  at 1.21.0, agrees at any value the lift writes it at.
