---
title: "Glossary"
---

Defined terms used throughout the specification. Where a term is defined in detail in a chapter,
that chapter is linked.

This page is **not normative on its own** — the linked chapter is the source of truth — but
implementations and authors SHOULD use these exact spellings. The vocabulary is closed by design
([principles P3](principles)); inventing parallel terminology defeats the lint loop.

## Architecture and IR

- **Block-array IR.** The universal pivot at the bottom of the three-layer IR: a voxel grid +
  palette + block entities + entities, neutral to format, edition, and version. Every format's
  frontend/backend, plus diff/IoU/serialization, meet here. See
  [architecture §3.1](architecture).
- **Intent IR.** The top layer: named members carrying `id` / `class` / `role` / `mat_slot` /
  `intent_state` / `resolved_state`. Independent type with invariants; not produced directly by
  schematic ingestion. See [architecture §3.2](architecture).
- **Semantic / Component-Theme IR.** The middle layer that resolves themes, components (`def`),
  and multi-building (`site`) into the Intent IR. See
  [components-editing-sites](components-editing-sites),
  [materials-themes](materials-themes).
- **Logic IR / Netlist IR / Placement IR.** The three redstone sub-layers between Intent IR and
  block-array IR. Delay is **not** carried in Logic/Netlist — only the Placement IR has it.
  See [redstone §14.8](redstone), [architecture §3.3](architecture).
- **`semantic_level`.** An imported artifact's progress label: `raw` (one voxel per line) →
  `grouped` (L1 spatial compression) → `lifted` (L2 semantic naming). The compiler reaches L1;
  L2 is the LLM's job. See [ecosystem-interop §12.3](ecosystem-interop).

## Members and blockstate

- **Member.** A named element of the Intent IR. Types: `block`, `block_entity`, `entity`. The
  IR carries the type, but the author writes one selector grammar across all of them
  ([principles P4](principles)).
- **`id` / `class` / `role`.** Identity, group, and architectural function tags on a member. Used
  for selectors and stable addresses.
- **`mat_slot`.** A material injection point on a member; bound by a `theme`. Structure carries
  slots, themes carry block bindings. See [materials-themes §7.1](materials-themes).
- **`intent_state`.** The author's blockstate intent. Edit diffs look **only** here. See
  [blockstate §6.2](blockstate).
- **`resolved_state`.** Compiler-derived blockstate (orientation, connections, waterlogged). Never
  hand-written.
- **Override-promotion.** Writing a blockstate that *could* be intent promotes it from
  `resolved_state` to `intent_state`. The spec rule is "derive by default; any blockstate that
  can be intent is overridable." See [blockstate §6.1](blockstate).
- **Anchor / bbox.** Every primitive carries an `anchor` (reference point), a declared bbox, an
  actual bbox, and a host face in the IR — needed because primitives like paintings, item frames,
  and arch windows have a declared size that differs from the occupied AABB. See
  [entities §8.2](entities).

## Materials and themes

- **Canonical token.** The value bound to a slot or theme selector: a *meaning* token, not a raw
  block ID. The backend resolves the per-`(edition, version)` ID and state names. See
  [materials-themes §7.2](materials-themes).
- **Canonical block token.** A specific block meaning: `@oak_planks`, `@water_cauldron`,
  `@oak_log[axis=x]`. Silent meaning-breaking downgrades (`@water_cauldron` → `cauldron`) are
  forbidden.
- **Abstract material token.** An aesthetic choice: `@floor.wood.broadleaf`, `@roof.dark_wood`.
  Theme policy MAY downgrade these (e.g. oak↔birch).
- **`theme`.** A CSS-like binding of slot/selector values to canonical tokens. Separates
  *structure* (where the walls are) from *style* (which blocks).
- **`def`.** A slot-bearing Component definition (a reusable struct). Recursion forbidden;
  parameterization allowed. The minimum version of a composite is the max of its parts.
  See [components-editing-sites §9.1](components-editing-sites).
- **`site`.** A multi-building container that places `def`-derived structures by topological
  relations (`east_of`, `gap=`, `connect`), not absolute coordinates. See
  [components-editing-sites §9.3](components-editing-sites).

## Compilation

- **Phase.** A fixed evaluation slot the compiler sorts each command into:
  `massing → envelope → openings → fixtures → logic_synth → logic_place → logic_route → raw`.
  Source order is irrelevant; phases enforce semantics
  ([compilation §4.1](compilation)).
- **Last-wins (local).** Within the same phase, a later command overrides an earlier one. The old
  whole-program last-wins ("paint model") is dropped. See [principles P2](principles).
- **Target axes.** `(edition, version)`: the only layer that knows them is the backend. The DSL
  source never names them. `--edition` is required, `--target` alone is forbidden. See
  [compilation §4.2](compilation), [versioning-editions](versioning-editions).
- **DataVersion.** Mojang's monotonically increasing integer key for a Java version. Cairn uses it
  as the canonical ordering key so the semver→date-based version transition does not break
  `since/until` or `@requires`. See [versioning-editions §10.1](versioning-editions).

## Headers and provenance

- **`@cairn`.** Header declaring the **Cairn language version** the file was written against
  (CalVer `YYYY.0M[.PATCH]`). Provenance only; optional.
- **`@requires`.** A capability floor on the Minecraft target (e.g. `version>=1.20`). Hard error
  on conflict with the inferred value.
- **`@intended_targets`.** A hint about which Minecraft versions the file was designed for. Not a
  verification record — the record lives in the lock.
- **Lock (`*.cairn.lock`).** Compiler-generated reproducibility record. Carries `source_hash`,
  `cairn_version`, `target(mc_version + data_version)`, `registry_pack_hash`,
  `constraint_catalog_hash`, `resolved_ir_hash`, and `verified: true`. See
  [versioning-editions §10.6](versioning-editions).
- **Provenance stamp.** `(edition, version)` recorded onto the block-array IR on import, mapped
  from the format itself (`.litematic` → java, `.mcstructure` → bedrock,
  `.schem` → java). See [ecosystem-interop §12.4](ecosystem-interop).

## Redstone

- **Logical cell / edition cell / physical tile.** The three-tier cell library. The Logic IR
  selects logical cells; the per-edition cell library lowers them to physical tiles. Confines the
  Java/Bedrock difference to the library. See [redstone §14.6](redstone).
- **Combinational vs sequential.** v1 ships closed-set combinational gates and curated sequential
  macros (`latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter`). Arbitrary
  FSMs / CPUs are out of scope for v1.
- **Truth / latency / temporal assertion.** The three verification kinds. See
  [redstone §14.7](redstone). Temporal is bounded `eventually within N` only, not full LTL.
- **QC / BUD.** Quasi-connectivity / block-update-detector behaviors. **Not absorbed** by the cell
  library; a circuit that depends on update-order semantics is an `E_NO_PORTABLE_IMPL` compile
  error.

## Lint and evaluation

- **Self-correction loop.** The compile → diagnostics → patch → recompile cycle that lifts
  precision out of one-shot generation ([principles P5](principles)). Diagnostic messages MUST
  be in "what is wrong / valid candidates in the target / suggested fix" form
  ([lint](lint)).
- **Fail-loud.** Silent substitution and implicit dropping of unknown IDs / out-of-domain states
  are forbidden. Errors return the closed set of valid candidates plus a suggested DSL fix.
  See [versioning-editions §10.4](versioning-editions).
- **`semantic_sensitivity`.** A constraint-catalog field distinguishing "the ID stays valid but
  meaning/behavior/appearance changed at this version" from `since/until`. Cauldron split @1.17,
  wall connection bool→none/low/tall @1.16, item format @1.20.5 are examples. See
  [versioning-editions §10.5](versioning-editions).
- **Block IoU.** Voxel intersection-over-union used for the import self-correction loop.
  Convergence threshold ≥ 0.985. See [ecosystem-interop §12.2](ecosystem-interop),
  [evaluation §13.2](evaluation).
- **Zero-shot Compile Rate / Fix Convergence Rate / Token Efficiency / Edit Stability.** The four
  primary spec-iteration metrics. See [evaluation §13.1](evaluation).

## Editions and versions

- **Edition.** `java` or `bedrock`. Compile-time only; never present in the DSL semantic layer.
  See [versioning-editions §10.7](versioning-editions).
- **Cairn version vs MC target.** Two **separate axes** distinguished by field/flag/keyword, never
  by format. `cairn:2026.06` vs `mc:1.21.4` in disambiguating prose. See
  [Specification overview](/spec/), [versioning-editions](versioning-editions).
- **Recompile, don't transcode.** The language spec does **not** guarantee NBT portability across
  version or edition. To target a new version, recompile the DSL; do not convert the NBT. See
  [versioning-editions §10.2](versioning-editions).

## Ecosystem interop

- **Raw / L1 / L2.** The three tiers of faithful transliteration on import. The compiler reaches
  L1 (spatial compression, no naming); L2 is the LLM's job (naming → `wall`, `mat_slot`,
  `theme`). See [ecosystem-interop §12.3](ecosystem-interop).
- **`raw_fill` / `raw_block` / `raw_repeat`.** Escape-hatch primitives used by faithful
  transliteration. Import-origin instances carry `origin=imported` so they are not treated as
  first-class design DSL.
