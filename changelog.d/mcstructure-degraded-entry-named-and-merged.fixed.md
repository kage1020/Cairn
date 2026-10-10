- *(formats)* A Bedrock build printed identical `W_INTENT_DEGRADED` lines, and its `.mcstructure`
  palette held one block several times. `roof-hip`'s four degraded stairs are four states of one
  id, and the warning named the id alone, so it printed two pairs of identical lines: the two
  stairs of a pair differ only in `facing`, which the warning left out. Each warning now names the
  Java state it came from, as `cairn info`'s note does:
  `minecraft:spruce_stairs[facing=north,half=bottom,shape=outer_left]`. And once `shape` is
  dropped, several Java states translate to one Bedrock state: those entries now share the first
  one's `block_palette` slot, with `block_indices` written through the merge and air kept at `0`,
  so the written palette is a set like the one it is built from. The Java palette and `info`'s
  per-entry counts are unchanged.
