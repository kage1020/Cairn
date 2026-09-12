# Cairn

> Language: **English** ([日本語](README.ja.md))

Cairn is a description language for Minecraft builds. You write what you want — a 9×7 cottage with cobblestone walls, a gable roof, windows across the front — and the compiler works out the blocks: which way each roof stair faces, where the openings land, the coordinate math, and the block IDs each edition and version actually uses.

A cairn is a stack of stones raised to mark a place. So is a Minecraft build.

## Install

```sh
cargo install cairn-lang-cli
```

That installs the `cairn` binary. Prebuilt archives for Linux, macOS, and Windows (`x86_64` and `aarch64`) are attached to every [release](https://github.com/kage1020/Cairn/releases); each contains `cairn` and the `cairn-lsp` language server, and each carries a sigstore bundle except the Windows `aarch64` one, which cosign ships no binary for. From a checkout, `cargo build --release` puts both in `target/release/`.

## Quick start

Write `cottage.crn`:

```
@cairn 2026.09
@requires version>=1.20

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  slot glass -> @glass_pane
  window[class=small] -> frame=@spruce_wood

struct cottage size=9x7
  floor  mat_slot=floor
  walls  class=outer mat_slot=wall height=4
  door   side=front at=center
  window class=small side=front offset=2 y=2 size=2x2 sym=true mat_slot=glass
  roof   kind=gable mat_slot=roof overhang=1
```

Compile it:

```sh
cairn compile cottage.crn --edition java --target 1.21.4
```

You get `cottage.nbt`, a vanilla structure file a structure block can load, and `cottage.crn.lock`, which records the source hash, the resolved target, and the registry pack the build used.

The same file builds for Bedrock:

```sh
cairn compile cottage.crn --edition bedrock --target 1.21.60
```

This writes `cottage.mcstructure`. Nothing in the source changed — the edition is a flag, not a dialect. Where Bedrock genuinely cannot express something (stair corner shapes, for one), the compile says so rather than quietly dropping it.

## Before you build

`cairn check` runs the same analysis without writing anything. Pin an edition and a target and it checks block ids too, naming the block you meant:

```
$ cairn check cottage.crn --edition java --target 1.21.4
cottage.crn:6:17: error[E_UNKNOWN_ID]: `minecraft:cobblestoen` is not a block in `java 1.21.4`
  note: `java 1.21.4` spells the nearest block `minecraft:cobblestone`
…
```

Diagnostics name what is wrong and what a valid value looks like, so a failed build tells you what to type next. `--format json` emits the same findings machine-readable, which is what makes a write-check-fix loop practical — by hand or by tooling.

`cairn info` answers the version question before you pick a target:

```
$ cairn info cottage.crn
registry compatibility:  1.20 .. latest
edition portability:     Java: portable: 6  degraded: 0  unsupported: 0   Bedrock: portable: 6  degraded: 0  unsupported: 0
buildable targets:       Java: 1.20.4, 1.21, 1.21.4   Bedrock: 1.21.0, 1.21.40, 1.21.60
intended targets:        (none declared)
semantic-sensitive:      (none)
```

## What works today

- `cairn parse`, `check`, `info`, `lower`, and `compile`.
- Java `.nbt` and Bedrock `.mcstructure` output from one source.
- Members: floors, walls, doors, windows, roofs (`gable`, `shed`, `hip`, `flat`), stairs, pressure plates, and `level` grouping.
- Themes with slots, selectors, and per-edition variants.
- A per-source lockfile, and registry packs carrying per-version block IDs and rename aliases.
- `cairn-lsp` with diagnostics and completion, plus a [VS Code extension](editors/vscode/).
- A tree-sitter grammar, for editors that want highlighting without the language server.

**Experimental.** `cairn synth --experimental-logic-synth` takes a `logic` graph through synthesis, netlist construction, edition selection, placement, routing, delay insertion, and crossing legalization, printing each stage as JSON. Redstone does not reach compiled artifacts yet, and the output shape is free to change.

**Not yet.** `.litematic` and `.schem` writers, importing existing schematics, redstone in compiled output and the tick simulator that verifies it, and the browser playground (the `cairn-lang-wasm` crate is still a placeholder with no exports).

## Key ideas

- **Declare intent, not blockstate.** A gable roof knows which way its stairs face and whether each one sits top or bottom half; you never write `facing=` for it. Blockstate is derived, and you override only when the value *is* the intent.
- **Order doesn't matter.** Write members in any order; the compiler sorts them into fixed phases (massing → envelope → openings → fixtures → redstone → raw).
- **Themes separate "where" from "what".** Structure carries `mat_slot`s; a theme binds those slots to materials, the way a stylesheet binds classes.
- **Recompile, don't transcode.** The portable artifact is the `.crn` source. Structure files are build output for one edition and version, like a binary.
- **Fail loud.** An unknown block, an unresolvable slot, or a target that cannot express your intent is an error or a named degradation, never a silent substitution.

## Documentation

<https://cairn.kage1020.com/> is the canonical home for the project's prose, mirrored at `/ja/<path>/` in Japanese.

- [Tutorial](https://cairn.kage1020.com/tutorial/) — the shortest path from install to a structure file in your world.
- [Examples](examples/) — `cottage`, `themed-tower`, `village`, `redstone-door`, and the smaller files that cover one member each.
- [Specification](https://cairn.kage1020.com/spec/) — the normative reference, with a [glossary](https://cairn.kage1020.com/spec/glossary/) that cuts across all of it.
- [Developer Guide](https://cairn.kage1020.com/development/) — workspace layout, dependency rules, build/test/lint commands.
- [Roadmap](https://cairn.kage1020.com/roadmap/) — what each month is aiming at.

Site source lives in [`website/src/content/docs/`](website/README.md) and is reviewed like code.

## Versioning

Releases use date-based versioning, `YYYY.M[.PATCH]`, covering the language, the compiler, and the registry packs as one bundle. That is a different axis from the Minecraft version you pass to `--target`; the two are always told apart by flag or keyword, never by format.

What a version bump is allowed to break is set by [Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/): every surface is **Stable**, **Evolving**, or **Internal**. Monthly minors are the only window for an `Evolving` break, and a `Stable` break gets one release of `W_DEPRECATED` warning first.

## Contributing

Bug reports, examples that expose a gap, spec critique, and code are all welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[Apache License 2.0](LICENSE) © kage1020 and the Cairn authors.
