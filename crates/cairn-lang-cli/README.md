# cairn-lang-cli

The `cairn` command-line interface. One of the two Cairn crates that ship an executable — `cairn` here, `cairn-lsp` in [`cairn-lang-lsp`](../cairn-lang-lsp/README.md) — it wires the [`cairn-lang-core`](../cairn-lang-core/README.md) compiler, the format backends in [`cairn-lang-formats`](../cairn-lang-formats/README.md), and the redstone synthesizer in [`cairn-lang-redstone`](../cairn-lang-redstone/README.md) into the subcommands an author actually types.

```sh
cargo install cairn-lang-cli
cairn compile cottage.crn --edition java --target 1.21.4
```

Prebuilt archives are attached to each [release](https://github.com/kage1020/Cairn/releases) and carry `cairn-lsp` alongside `cairn`.

## Subcommands

| Subcommand | What it does |
|---|---|
| `cairn parse <file.crn>` | Lex and parse, printing the AST. `--format json\|debug`; the JSON gives each member line of a `struct` or `def` body its address, and lists each `edit` line with its target and the keys it sets. Runs no check passes. |
| `cairn check <file.crn>` | Run the validation passes and print diagnostics. `--format text\|json`. Writes nothing. |
| `cairn info <file.crn>` | Report the version range the file declares, per-edition portability, buildable targets, declared intended targets, and semantic-sensitive members. `--editions java,bedrock`, `--format text\|json`. |
| `cairn lower <file.crn>` | Lower all the way to the block-array IR and print it. `--edition`, `--format ascii\|json\|debug`. A debugging surface for the universal voxel pivot; the ASCII dump lists each member that built a cell by its address. Without `--edition` it pins no edition, so it leaves out what `@edition` guards hold and every struct's circuit. |
| `cairn compile <file.crn> --edition <e>` | Write the structure artifacts and a lockfile. `--target`, `--structure-format`, `--out`, `--lock`. |
| `cairn import <file>` | Write a `.nbt`, `.litematic`, `.schem` or `.mcstructure` file as a `.crn` of raw members that compiles back to its blocks, stamped with the target it compiles for. `--mode raw`, `--out`. See below. |
| `cairn diff-blocks <structure> <file.crn>` | Build the source at the structure file's stamp and compare the two cell by cell: the metrics, the regions that differ with their likely causes, and a suggested patch of `edit` lines. `--format text\|json`. Exits 1 with `E_DECOMPILE_DIFF` until the lift converges. See below. |
| `cairn synth <file.crn>` | Print one redstone pipeline stage as JSON. Experimental; see below. |
| `cairn docs [page] [title]` | Print a page of the documentation site as Markdown, from the copy embedded in the binary, or list the pages. `--toc` lists a page's headings. See below. |

`compile` writes gzipped `.nbt` for `--edition java` and uncompressed `.mcstructure` for `--edition bedrock`, one file per scope, plus `<source>.lock` next to the source. `--structure-format litematic` writes Java's as Litematica `.litematic` instead: a file per struct, and one per site with a region per placement and walkway ([ecosystem-interop "Litematica"](https://cairn.kage1020.com/spec/ecosystem-interop/)). The format changes the files and nothing else: the naming rule, the collision guards and the lockfile are the same, and a site named after a struct is refused as the one `.litematic` both would write. `--structure-format schem` writes Sponge Schematic version 3 `.schem` files, one per scope as `.nbt` does, each offset by its origin in its site ([ecosystem-interop "Sponge Schematic"](https://cairn.kage1020.com/spec/ecosystem-interop/)). Bedrock has one format, so `--structure-format` with `--edition bedrock` is refused with exit 2 before the file is read. A Bedrock blockstate goes through the registry pack's state map for the target version: a state the map has no form for is a hard error (`E_STATE_UNMAPPED`), and intent the version can only approximate (stair `shape` before 1.26.60) is dropped with a `W_INTENT_DEGRADED` warning rather than in silence.

A `cairn check` with no `--target` still lowers, with no target, and reports the lowering-stage findings every target that reads their line raises, such as a roof bound outside the stair family, weighed as if every block id some shipped version declares were declared. `check --edition E --target V` pins the same `(edition, version)` pair a compile pins, which is what lets it reach the lowering-stage findings only a target can answer — `E_UNKNOWN_ID` above all — without building anything. That is the shape a CI gate wants, since an unpinned `cairn check` can go green on a source `cairn compile` refuses. What it does not take on is anything `compile` writes: no artifact, no lockfile, and no `@requires` floor enforcement (`E_VERSION_CAP`), since holding `--target` to the floors belongs to the command that certifies a build.

## Targeting rules

Per [compilation "Target axes"](https://cairn.kage1020.com/spec/compilation/):

- `--edition` is **required** by `compile`, and every subcommand that takes `--target` refuses it without `--edition`. The same `1.21` means different things on Java and Bedrock, and Java's DataVersion is unrelated to Bedrock's `block_version`.
- `--target` is an opaque label resolved through the pinned edition's data table, with DataVersion as the canonical ordering key ([versioning-editions "The target is a compile-time parameter"](https://cairn.kage1020.com/spec/versioning-editions/)). `latest` aliases the row that table names as latest, which is not necessarily the newest row it carries.
- `compile --target` defaults to `latest`. `check --target` has no default, so `cairn check --edition java` still runs the unpinned gate rather than refusing ids on a version nobody chose.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success. Warnings do not change it. |
| `1` | A parse failure, an `Error`-severity diagnostic, an I/O error, a run-level refusal such as a `--target` the edition does not ship or a structure file `cairn import` cannot read, stamp or write as raw members, or a lift `cairn diff-blocks` finds has not converged. |
| `2` | The source file could not be located, or the flags are unusable (`--target` without `--edition`, an empty `--editions`, a `cairn import` or `cairn diff-blocks` of a file whose extension names no format it reads, or an import's `--out` naming that file), or `cairn docs` was asked for a page or part the binary does not carry, for a title more than one part has, or for a `#fragment` together with a title or `--toc`. |

A reader of stdout that stops early (`cairn lower f.crn | head -1`) does not change the code: output written after stdout has been closed is dropped, and the command exits as it would have with stdout open. This holds for stdout only: diagnostics go to stderr, and a stderr whose reader has gone still ends the run with a panic.

A run-level refusal is reported on stderr and by the exit code in both output formats, never as an element of the `--format json` array: it is not a finding at a span in the file, and giving it a line number would put a position on a fact that has none. A `check --format json` consumer reads the verdict from the exit code and treats stderr as unstructured prose: an exit of `1` over an array with no `"severity": "error"` element is a run-level refusal, and nothing else reads that way ([lint "Machine-readable payload"](https://cairn.kage1020.com/spec/lint/)).

## `cairn import`

`cairn import house.litematic` writes `house.raw.crn` beside the file, or the path `--out` names: the compiler's half of the reverse direction, a faithful transliteration with no name given to anything ([ecosystem-interop "Reverse direction: the compiler transliterates, an LLM lifts"](https://cairn.kage1020.com/spec/ecosystem-interop/)). Each structure the file holds is a struct of `raw_block`, `raw_fill` and `raw_repeat` lines marked `origin=imported`, a Litematica region a struct of its own and a structure wider or deeper than 48 blocks a `site` of chunks. The source is stamped with the shipped target the file's saved version maps onto, and compiled against it before it is written, so `cairn compile house.raw.crn --edition E --target V`, with the pair the closing note names, builds the blocks the file holds. A file saved past every shipped target is refused, as is one whose blocks that target does not declare.

Every run warns with `W_SEMANTIC_LOSS`: once that the import recovers blocks rather than intent, and once for each thing in the file a raw member cannot carry, such as a chest's contents, an entity or a structure void ([ecosystem-interop "Import stamping and pitfalls"](https://cairn.kage1020.com/spec/ecosystem-interop/)). Naming what the members build is a refactor of the written source, not a mode of the command, so `raw` is its one `--mode`.

The command is a thin wrapper over [`cairn_lang_formats::import`](../cairn-lang-formats/README.md), which decodes, stamps and transliterates; it reads the file, writes the source and prints what the library returns.

## `cairn diff-blocks`

`cairn diff-blocks house.litematic house.lifted.crn` closes the lift loop ([ecosystem-interop "Diffing a lift"](https://cairn.kage1020.com/spec/ecosystem-interop/)). It reads the structure file as `cairn import` does, builds the source at the target the file is stamped with, and compares the two cell by cell. The report gives the block IoU, the `state_accuracy` and the share of the build raw members still build, each against its threshold (≥ 0.985, ≥ 0.995, ≤ 5%), then every region of cells that differ with its block and the member that likely caused it ([ecosystem-interop "Regions and their causes"](https://cairn.kage1020.com/spec/ecosystem-interop/)), then up to three `edit` lines that bring the build nearer the file, each found by building the source with it appended ([ecosystem-interop "The suggested patch"](https://cairn.kage1020.com/spec/ecosystem-interop/)). Until all three metrics meet their thresholds the run exits 1 with `E_DECOMPILE_DIFF` on stderr. `--format json` prints the whole report, every region listed, as one document.

The command builds the source with the passes `cairn compile` runs, and the comparison, the regions and the patch search are [`cairn_lang_core::voxel_diff`](../cairn-lang-core/README.md), which an evaluation harness can call for the same metrics.

## `cairn docs`

Every Markdown page of [the documentation site](https://cairn.kage1020.com/), the specification among them, is compiled into the binary by [`cairn-lang-docs`](../../website/rust/README.md), so `cairn docs` shows the pages of the release that is running, with no network. A page is named by the path the site serves it at or by its file (`spec/index`, `spec/lint.md`), so a link to any page it lists can be passed as written, `index.md` and fragment included:

```sh
cairn docs                                     # every page, with its title
cairn docs spec/syntax                         # one page, as Markdown
cairn docs spec/syntax --toc                   # its headings, each a link `cairn docs` takes
cairn docs /spec/#two-version-axes             # the section a link opens
cairn docs spec/lint "Error vs warning"        # the section a citation names
cairn docs ja/spec/syntax                      # the Japanese copy
```

The title is quoted the way `--help` and the code cite the spec: a heading without its number, a term a list item on the page defines in bold (`- **Term.**`), or the page's own title. A heading or the page's title is also taken with its number. A title that more than one part of the page has is refused with each part listed, and the `#fragment` of the section you mean picks one. `cargo xtask lint spec-citations` holds every citation in the repository to what `cairn docs` can print.

Git Bash on Windows rewrites an argument that starts with `/` into a Windows path before `cairn` sees it, so leave the leading slash off there: `cairn docs spec/#two-version-axes`.

## `cairn synth` (experimental)

`cairn synth --experimental-logic-synth --stage <s>` prints one stage of the redstone pipeline as JSON: `logic`, `netlist`, `edition`, `placement`, `route`, `delay`, or `legalize`. `--edition` is required from `edition` onwards and refused before it, because the earlier stages are edition-neutral by contract. Each circuit of the four layout stages carries a `"stage"` key echoing the flag that produced it, so a consumer reads the stage off the output instead of inferring it from which keys are present.

The output shape is **not** covered by the stable compatibility tier and may change at any time. The opt-in flag exists so a caller cannot come to depend on it by accident. `compile`, `check`, `info` and `lower --edition` run the same pipeline: `compile` lays each legalized circuit under its struct, which rises over it, and each of them reports the circuit's findings.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
