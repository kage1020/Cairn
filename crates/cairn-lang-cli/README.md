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
| `cairn parse <file.crn>` | Lex and parse, printing the AST. `--format json\|debug`. Runs no check passes. |
| `cairn check <file.crn>` | Run the validation passes and print diagnostics. `--format text\|json`. Writes nothing. |
| `cairn info <file.crn>` | Report the registry-compatible version range, per-edition portability, buildable targets, declared intended targets, and semantic-sensitive members. `--editions java,bedrock`, `--format text\|json`. |
| `cairn lower <file.crn>` | Lower all the way to the block-array IR and print it. `--format ascii\|json\|debug`. A debugging surface for the universal voxel pivot. |
| `cairn compile <file.crn> --edition <e>` | Write the structure artifacts and a lockfile. `--target`, `--out`, `--lock`. |
| `cairn synth <file.crn>` | Print one redstone pipeline stage as JSON. Experimental; see below. |

`compile` writes gzipped `.nbt` for `--edition java` and uncompressed `.mcstructure` for `--edition bedrock`, one file per scope, plus `<source>.lock` next to the source. A Bedrock blockstate property the backend cannot translate is a hard error; intent it can only approximate (stair `shape`) is dropped with a `W_INTENT_DEGRADED` warning rather than in silence.

`check --edition E --target V` pins the same `(edition, version)` pair a compile pins, which is what lets it reach the lowering-stage findings — `E_UNKNOWN_ID` above all — without building anything. That is the shape a CI gate wants: `cairn check` going green on a source `cairn compile` refuses is the gap the flag closes. What it does not take on is anything `compile` writes: no artifact, no lockfile, and no `@requires` floor enforcement (`E_VERSION_CAP`), since holding `--target` to the floors belongs to the command that certifies a build.

## Targeting rules

Per [compilation §4.2](https://cairn.kage1020.com/spec/compilation/):

- `--edition` is **required** for any subcommand that emits voxels, and `--target` is refused without it. The same `1.21` means different things on Java and Bedrock, and Java's DataVersion is unrelated to Bedrock's `block_version`.
- `--target` is an opaque label resolved through the pinned edition's data table, with DataVersion as the canonical ordering key ([versioning-editions §10.1](https://cairn.kage1020.com/spec/versioning-editions/)). `latest` aliases the row that table names as latest, which is not necessarily the newest row it carries.
- `compile --target` defaults to `latest`. `check --target` has no default, so `cairn check --edition java` still runs the unpinned gate rather than refusing ids on a version nobody chose.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success. Warnings do not change it. |
| `1` | A parse failure, an `Error`-severity diagnostic, an I/O error, or a run-level refusal such as a `--target` the edition does not ship. |
| `2` | The source file could not be located, or the flags are unusable (`--target` without `--edition`, an empty `--editions`). |

A run-level refusal is reported on stderr and by the exit code in both output formats, never as an element of the `--format json` array: it is not a finding at a span in the file, and giving it a line number would put a position on a fact that has none.

## `cairn synth` (experimental)

`cairn synth --experimental-logic-synth --stage <s>` prints one stage of the redstone pipeline as JSON: `logic`, `netlist`, `edition`, `placement`, `route`, `delay`, or `crossing`. `--edition` is required from `edition` onwards and refused before it, because the earlier stages are edition-neutral by contract. Every cell of the placement-derived stages carries a `"stage"` key echoing the flag that produced it, so a consumer reads the stage off the output instead of inferring it from which optional keys are present.

The output shape is **not** covered by the stable compatibility tier and may change at any time. The opt-in flag exists so a caller cannot come to depend on it by accident. Nothing from this pipeline reaches a compiled artifact yet.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
