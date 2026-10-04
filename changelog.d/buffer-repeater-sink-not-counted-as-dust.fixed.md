- *(redstone)* Delay insertion counted the sink at the end of a segment as a block of dust, so a
  segment from a fresh source carried one block less than a source at strength 15 does. A 16-step
  route, whose 15 blocks of dust reach the sink at strength 1, got a repeater it does not need,
  and a straight run took one every 15 steps. A sink, or a repeater, now reads the dust before it
  and may be reached over all 15 blocks: a 16-step segment takes no repeater, a 32-step one takes
  one rather than two, and on a straight run the repeater stands in place of every 16th block.
  This changes the repeaters `--experimental-logic-synth` places and the delay it reports.
