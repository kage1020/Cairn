# Cairn

> Language: **English** ([日本語](README.ja.md))

Cairn is a description language for Minecraft builds. You write what you want — a 9×7 cottage with cobblestone walls, a gable roof, windows across the front — and the compiler works out the blocks: which way each roof stair faces, where the openings land, the coordinate math, and the block IDs each edition and version actually uses.

A cairn is a stack of stones raised to mark a place. So is a Minecraft build.

## Install

```sh
cargo install cairn-lang-cli
```

That installs the `cairn` binary.

Every [release](https://github.com/kage1020/Cairn/releases) also attaches prebuilt archives for Linux, macOS, and Windows (`x86_64` and `aarch64`). Each archive contains `cairn` and the `cairn-lsp` language server. All but the Windows `aarch64` archive carry a sigstore bundle; cosign ships no binary for that platform.

From a checkout, `cargo build --release` puts both binaries in `target/release/`.

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

You get two files:

- `cottage.nbt`, a vanilla structure file a structure block can load.
- `cottage.crn.lock`, which records the source hash, the resolved target, and the registry pack the build used.

The same file builds for Bedrock:

```sh
cairn compile cottage.crn --edition bedrock --target 1.21.60
```

This writes `cottage.mcstructure`. Nothing in the source changed — the edition is a flag, not a dialect. Where Bedrock genuinely cannot express something (stair corner shapes, for one), the compile says so rather than quietly dropping it.

## Before you build

`cairn check` runs the same analysis without writing anything. Give it an edition and a target and it checks block IDs too, naming the block you probably meant:

```
$ cairn check cottage.crn --edition java --target 1.21.4
cottage.crn:6:17: error[E_UNKNOWN_ID]: `minecraft:cobblestoen` is not a block in `java 1.21.4`
  note: `java 1.21.4` spells the nearest block `minecraft:cobblestone`
…
```

Diagnostics name what is wrong and what a valid value looks like, so a failed build tells you what to type next. `--format json` emits the same findings in machine-readable form, so the write-check-fix loop works as well for tools as it does by hand.

`cairn info` tells you which versions a file builds for before you pick a target:

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
- Themes with slots and per-edition variants.
- A per-source lockfile, and registry packs carrying per-version block IDs and rename aliases.
- `cairn-lsp` with diagnostics and completion, plus a [VS Code extension](editors/vscode/).
- A tree-sitter grammar, for editors that want highlighting without the language server.

**Experimental.** `cairn synth --experimental-logic-synth` takes a `logic` graph through synthesis, netlist construction, edition selection, placement, routing, delay insertion, and crossing legalization, printing each stage as JSON. Redstone does not reach compiled artifacts yet, and the output shape is free to change.

**Not yet.**

- Theme selector bindings. `window[class=small] -> frame=@spruce_wood` matches its windows but paints nothing, and `cairn check` says so.
- `.litematic` and `.schem` writers, and importing existing schematics.
- Redstone in compiled output, and the tick simulator that verifies it.
- The browser playground. The `cairn-lang-wasm` crate is still a placeholder with no exports.

## Key ideas

- **Declare intent, not blockstate.** A gable roof knows which way its stairs face and whether each one sits top or bottom half; you never write `facing=` for it. Blockstate is derived, and you override only when the value *is* the intent.
- **Order doesn't matter.** Write members in any order; the compiler sorts them into fixed phases (massing → envelope → openings → fixtures → redstone → raw).
- **Themes separate "where" from "what".** Structure carries `mat_slot`s; a theme binds those slots to materials, the way a stylesheet binds classes.
- **Recompile, don't transcode.** The portable artifact is the `.crn` source. Structure files are build output for one edition and version, like a binary.
- **Fail loud.** An unknown block, an unresolvable slot, or a target that cannot express your intent is an error or a named degradation, never a silent substitution.

## Documentation

The project's prose lives at <https://cairn.kage1020.com/>. A Japanese translation mirrors each page at `/ja/<path>/`.

- [Tutorial](https://cairn.kage1020.com/tutorial/) — the shortest path from install to a structure file in your world.
- [Examples](examples/) — `cottage`, `themed-tower`, `village`, `redstone-door`, and the smaller files that cover one member each.
- [Specification](https://cairn.kage1020.com/spec/) — the normative reference, with a [glossary](https://cairn.kage1020.com/spec/glossary/) that cuts across all of it.
- [Developer Guide](https://cairn.kage1020.com/development/) — workspace layout, dependency rules, build/test/lint commands.
- [Roadmap](https://cairn.kage1020.com/roadmap/) — what each month is aiming at.

Site source lives in [`website/src/content/docs/`](website/README.md) and is reviewed like code.

The same pages ship with the compiler, and the site also serves them in a form language models read well:

- `cairn docs` lists the site's pages, all but the home page and the playground, as they were when the installed `cairn` was built. `cairn docs spec/syntax` prints one page as Markdown, and `cairn docs spec/lint "Error vs warning"` prints only the section a citation names. It needs no network, and the pages always match what that release implements.
- [`/llms.txt`](https://cairn.kage1020.com/llms.txt) indexes the site for language models. Each page is also served as Markdown at its URL followed by `index.md` ([`/spec/syntax/index.md`](https://cairn.kage1020.com/spec/syntax/index.md)), and [`/llms-full.txt`](https://cairn.kage1020.com/llms-full.txt) holds every English page in one file.

## Versioning

Releases use date-based versioning, `YYYY.M[.PATCH]`, and version the language, the compiler, and the registry packs together as one bundle. This is a separate axis from the Minecraft version you pass to `--target`. The two are always told apart by flag or keyword, never by format.

[Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/) set what a version bump may break. Every surface is **Stable**, **Evolving**, or **Internal**. Only a monthly minor release may break an `Evolving` surface, and a `Stable` surface warns with `W_DEPRECATED` for one release before it breaks.

## Contributing

Bug reports, examples that expose a gap, spec critique, and code are all welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[Apache License 2.0](LICENSE) © kage1020 and the Cairn authors.
