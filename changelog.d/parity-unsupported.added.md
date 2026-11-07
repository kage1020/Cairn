- *(core)* `E_PARITY_UNSUPPORTED`: a semantic member's `mat_slot=`, or a `connect … path=`, that
  resolves to a block only the other edition has is refused at a pinned target, with the registry
  pack's reason and the two alternatives, a per-edition theme or a `raw_block` under the other
  edition's `@edition` guard. A `raw_block` or `raw_fill` that names such a block stays
  `E_UNKNOWN_ID`, offers no nearest id, and gains a note naming the guard that would keep it; its
  payload carries the other edition as `declared_by`.
- *(formats)* Registry packs gain an optional `absent` component: the other edition's blocks the
  edition has no counterpart for under any id, each with its reason. The loader refuses an id the
  edition declares under that or another spelling of its alias group. Java's built-in pack lists
  Bedrock's world-building, Education Edition and chemistry blocks, its item-frame blocks and the
  Nether reactor; Bedrock's lists Java's test-framework blocks.
- *(cli)* `cairn info`'s `edition portability` row answers an entry the edition's `absent` lists
  with that reason instead of a `did you mean`, and `--format json` gives it the reason
  `no_counterpart` with the sentence as `detail`.
