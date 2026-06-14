# Cairn

> Language: **English** ([日本語](README.ja.md))

**Cairn** is a description language for Minecraft builds. You declare *intent* — walls, roofs,
windows, symmetry, themes, even redstone logic — and the compiler resolves the voxels: blockstates,
orientations, coordinate math, signal routing, and per-edition/per-version block IDs.

A cairn is a deliberately stacked pile of stones that marks a place. That is exactly what a Minecraft
build is: intentionally placed blocks. The name is the thesis.

> Status: **design specification, draft `2026.06`.** The language is being designed in the open;
> a reference compiler is not yet implemented. The normative specification, tutorial, and
> developer guide live on the [documentation site](https://cairn.kage1020.com/).

## Why

Minecraft's NBT/SNBT is inefficient for AI to read and write (binary, one-record-per-block) and is
misaligned with how humans and AI reason about architecture (walls, roofs, symmetry). Cairn is an
**intermediate language that aligns architectural intent with the voxel world**. It is the "eyes and
hands" an AI uses to see and build.

The approach is **generation-first (lossy)**: full round-trip fidelity with NBT is given up in favor
of letting an AI generate and edit builds accurately. The portable artifact is always the Cairn
source; emitted NBT/schematics are per-target build outputs (like a binary).

## Example

```
@requires version>=1.20

theme medieval:
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  window[class=small] -> frame=@spruce_wood

struct cottage size=9x7
  floor  mat_slot=floor
  walls  class=outer mat_slot=wall height=4
  door   side=front at=center
  window class=small side=front offset=2 y=2 size=2x2 sym=true
  roof   kind=gable mat_slot=roof overhang=1
```

```sh
cairn compile cottage.crn --edition java --target 1.21.4
```

## Key ideas

- **Declare intent, not blockstate.** Stair facing, door orientation, pane connections, and bed
  head/foot are derived by the compiler; you only override when the value *is* the intent.
- **Phase-ordered evaluation.** Write commands flat and order-free; the compiler sorts them into
  fixed phases (massing → envelope → openings → fixtures → redstone → raw).
- **CSS-like themes.** Structure carries `mat_slot`s; a `theme` binds slots and selectors to
  materials, separating "where" from "what."
- **Java and Bedrock from one source.** Edition is a compile-time target axis; the canonical material
  vocabulary and per-edition backends absorb ID/state differences. Recompile, don't transcode.
- **Logical redstone.** Describe a signal graph; the compiler synthesizes and place-and-routes the
  actual dust/repeaters/torches, per edition.
- **Lint as a first-class loop.** The compiler is also an architectural linter; precision is earned
  through a self-correcting loop, not one-shot generation.
- **Ecosystem interop.** Exports to `.nbt`, `.litematic`, `.schem`, `.mcstructure`; imports schematics
  as a faithful low-level transliteration that an LLM can then lift into idiomatic Cairn.

## Documentation

The site at <https://cairn.kage1020.com/> is the canonical home for the project's prose:

- [Specification](https://cairn.kage1020.com/spec/) — fifteen focused chapters plus a
  cross-cutting [glossary](https://cairn.kage1020.com/spec/glossary/).
- [Tutorial](https://cairn.kage1020.com/tutorial/) — walks through the worked
  [`examples/`](examples/) (`cottage`, `themed-tower`, `redstone-door`, `village`).
- [Developer Guide](https://cairn.kage1020.com/development/) — the Rust workspace layout,
  crate dependency graph, build/test/lint commands.
- 日本語版 — every page above is mirrored at `/ja/<path>/`.

The site Markdown source lives in [`website/src/content/docs/`](website/README.md); edits there
go through the same review flow as code. Cloudflare Pages auto-deploys every push to `main` via
its Git integration.

## Versioning

Cairn releases use **date-based versioning (CalVer)** `YYYY.0M[.PATCH]` (e.g. `2026.06`, `2026.06.1`).
This is the version of the language + reference compiler + standard library + registry/constraint
packs as a bundle. It is a **separate axis** from the Minecraft target version (`--target`); the two
are always distinguished by field/flag/keyword, never by format. See the spec for details.

The contract behind a version bump — what is safe to break, when — is set by
[Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/): every surface is **Stable**,
**Evolving**, or **Internal**, with monthly minors as the only window for `Evolving` breaks and
one release of `W_DEPRECATED` lead time before any `Stable` break.

## Roadmap

[Roadmap](https://cairn.kage1020.com/roadmap/) lists the M1–M6 milestones and the planned monthly
scope through `2027.06.0`:

- **M1** (`2026.07.0`) — source parses
- **M2** (`2026.10.0`) — minimal build (single room, Java, lockfile)
- **M3** (`2027.01.0`) — examples work end-to-end on Java
- **M4** (`2027.02.0`) — Java/Bedrock parity
- **M5** (`2027.03.0`) — `cairn-lsp` and VS Code extension
- **M6** (`2027.05.0`) — redstone logic, place-and-route, and tick simulator

Monthly minor releases are opened automatically by a GitHub Actions cron on the 1st of each month;
patches are opened on demand from `main` when qualifying commits land.

## Contributing

Cairn is at the design stage; discussion, critique, and concrete proposals are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) and our [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[Apache License 2.0](LICENSE) © kage1020 and the Cairn authors.
