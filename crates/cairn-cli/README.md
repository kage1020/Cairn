# cairn-cli

The `cairn` command-line interface. This is the only Cairn crate that ships an executable; it wires
the [`cairn-core`](../cairn-core/README.md) compiler, the format backends in
[`cairn-formats`](../cairn-formats/README.md), and (when described logically) the redstone synthesizer
in [`cairn-redstone`](../cairn-redstone/README.md) into the subcommands an author actually types.

## Status

Skeleton. The current binary recognizes `--version` / `--help` and prints a stub usage block; every
real subcommand returns a "not implemented yet" error.

## Subcommands (planned)

The CLI surface follows the spec; each subcommand maps onto a chapter so the help text and the
normative document can never drift.

| Subcommand | Purpose | Spec reference |
|---|---|---|
| `cairn compile <file.crn> --edition <e> --target <v>` | Compile a `.crn` source to NBT / schematic output for `(edition, version)` | [compilation §4.2](https://kage1020.github.io/Cairn/spec/compilation/), [versioning-editions](https://kage1020.github.io/Cairn/spec/versioning-editions/) |
| `cairn import <schematic> --mode raw\|l1` | Faithful transliteration of `.nbt` / `.litematic` / `.schem` / `.mcstructure` into raw-centric `.crn` | [ecosystem-interop §12.2](https://kage1020.github.io/Cairn/spec/ecosystem-interop/) |
| `cairn info <file.crn> --editions java,bedrock` | Report registry compatibility window, semantic-sensitive members, and recommended test targets | [versioning-editions §10.5](https://kage1020.github.io/Cairn/spec/versioning-editions/) |
| `cairn diff-blocks <schematic> <file.crn>` | Voxel XOR + state diff for the self-correction loop | [ecosystem-interop §12.2](https://kage1020.github.io/Cairn/spec/ecosystem-interop/), [evaluation](https://kage1020.github.io/Cairn/spec/evaluation/) |

## Targeting rules

Per [compilation §4.2](https://kage1020.github.io/Cairn/spec/compilation/):

- `--edition` is **required** for any subcommand that emits voxels. The same `1.21` means different
  things on Java and Bedrock, and Java's DataVersion is unrelated to Bedrock's `block_version`.
- `--target` accepts either the legacy semver form (`1.21.4`) or a date-based string; both are
  resolved through DataVersion as the canonical ordering key
  ([versioning-editions §10.1](https://kage1020.github.io/Cairn/spec/versioning-editions/)).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
