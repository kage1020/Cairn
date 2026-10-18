- *(cli)* A `W_INTENT_DEGRADED` line named its palette entry's id twice, bare after the
  structure's scope and again inside the sentence. It now names the entry once, as `id[states]`,
  the spelling `cairn info`'s note uses, then says what Bedrock lacks for the dropped `key` without
  repeating the value the entry's states already show. `cairn info`'s degraded notes drop the
  repeated value the same way. The diagnostic code is unchanged:

  ```text
  warning[W_INTENT_DEGRADED]: struct::roof_hip: minecraft:spruce_stairs[facing=north,half=bottom,shape=outer_left]: Bedrock has no `shape` state; its stairs render straight, so corners show visual gaps
  ```
