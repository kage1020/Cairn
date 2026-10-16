- *(cli)* A `W_INTENT_DEGRADED` line named its palette entry twice, by its id and again by its
  Java state. It now names the entry once, as `id[states]` after the structure's scope, the
  spelling `cairn info`'s note uses, then says what was lost. `roof-hip`'s first line goes from 244
  characters to 212; the code is unchanged:

  ```text
  warning[W_INTENT_DEGRADED]: struct::roof_hip: minecraft:spruce_stairs[facing=north,half=bottom,shape=outer_left]: shape=outer_left has no Bedrock state; Bedrock stairs render straight, so corners show visual gaps
  ```
