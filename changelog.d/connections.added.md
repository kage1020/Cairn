- *(core)* Once every member of a body is painted, each glass pane, run of bars, fence, wall and
  fence gate takes its connection states from the blocks beside it, as the game sets them when the
  block is placed: a pane's and a fence's four sides, a wall's `none` / `low` / `tall` sides and
  its `up` post, and a fence gate's `in_wall`. A state the block's material or line writes is
  kept, and the derived states are recorded in the member's `resolved_state`.
  `TargetRegistry::joins` says what a block is to these rules; a target that cannot say leaves the
  block as painted.
- *(formats)* Both built-in packs carry a `connections` component, per version: which blocks are
  panes, walls, fences, wooden or not, and fence gates, which a face never joins, which raise a
  wall's post, which faces of each state are whole, and which parts of a wall below each state's
  bottom covers. It is extracted from the block tags of the Java release each version follows and
  the block models of that release's client jar. `PackView::joins` answers from it, at a pinned
  version or, unpinned, with what every version agrees on, and reads a Bedrock id as the canonical
  blocks its state map names that take its states. The loader refuses an empty component or row, a
  lookup that reads or maps nothing, a default no rule reads or a fence gate's unreadable one, and
  on Java a block or state value the version's block table does not give.
