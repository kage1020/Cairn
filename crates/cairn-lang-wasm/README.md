# cairn-lang-wasm

WebAssembly bindings for the Cairn compiler. The documentation site's [playground](https://cairn.kage1020.com/playground/) runs them: a source compiles in the browser, with no server, through the same [`cairn-lang-core`](../cairn-lang-core/README.md) and [`cairn-lang-formats`](../cairn-lang-formats/README.md) code the `cairn` CLI runs, and the built-in registry packs are embedded in the module.

## Status

The four exports below ship, and CI builds the module and tests them in Node on every pull request. Their JavaScript API is Evolving ([compatibility "Evolving"](https://cairn.kage1020.com/spec/compatibility/)): a monthly minor may change it, and the CHANGELOG says so. The crate's Rust API, the `report` module the exports wrap, is Internal.

`import_raw(bytes, format)`, a faithful transliteration of a structure file to raw-centric `.crn` ([ecosystem-interop "Reverse direction: the compiler transliterates, an LLM lifts"](https://cairn.kage1020.com/spec/ecosystem-interop/)), waits on the readers `cairn-lang-formats` does not have yet.

## Build

```sh
wasm-pack build crates/cairn-lang-wasm --target web --release
```

`pnpm run build:wasm` in [`website/`](../../website/README.md) runs the same build into the site, and fails when the module outgrows the size budget [`website/scripts/build-wasm.sh`](../../website/scripts/build-wasm.sh) sets. `wasm-opt` is off in `Cargo.toml`: on this module it saves about 5%, and nothing once the module is gzipped, and `wasm-pack` would download it for every build.

```sh
cargo test -p cairn-lang-wasm                  # what each export answers, as Rust values
wasm-pack test --node crates/cairn-lang-wasm   # the exports as a JavaScript caller meets them
```

## API

| Export | Answers |
|---|---|
| `cairn_version()` | The Cairn release version, as a string. |
| `compile(source, edition, target)` | `cairn compile --edition <edition> --target <target>` over `source`, written to memory: `{ ok, diagnostics, run_level, ir, artifacts }`. |
| `info(source, editions)` | `cairn info --editions <editions>` over `source`: `{ ok, diagnostics, run_level, axes }`, where `axes` is the report `cairn info --format json` prints. |
| `targets(edition)` | `{ latest, versions }`: every version a `target` may name, in release order, and the one `latest` names. |

The fields:

- `ok` is whether `cairn compile`, or `cairn info`, would exit 0.
- `diagnostics` holds every finding the run reports, warnings included, each in the shape [lint "Machine-readable payload"](https://cairn.kage1020.com/spec/lint/) gives a finding. A source that does not parse answers its one `E_PARSE`.
- `run_level` holds what refuses the run without being a finding at a span: an unshipped `target`, a lost scope (`E_PARTIAL_BUILD`), a `@requires` floor the target is below (`E_VERSION_CAP`), two structures written to one file, and a palette a registry pack should have refused. The warnings that name no span, such as `W_INTENT_DEGRADED`, are here too, in the order they arose. Each carries `severity` and `primary`, and `code`, `line`, `col` and `notes` where it has them.
- `ir` is the block-array IR the structure files are written from, once no finding is an error, even when the run is refused; `null` otherwise. Its layout is Internal.
- `artifacts` holds one `{ name, bytes }` per structure, `bytes` a `Uint8Array` of the file `cairn compile` writes, when the build is `ok`. No lockfile is written.

An `edition` that is not `java` or `bedrock`, or an `editions` list that is empty or names something else, throws an `Error` whose message says what is valid. A Rust panic aborts, which traps the instance: a caller drops it and loads a fresh one.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for the parser, the checks and the lowering.
- [`cairn-lang-formats`](../cairn-lang-formats/README.md) for the build both front ends run, the writers and the registry packs.
- `wasm-bindgen` and `js-sys` for the JavaScript boundary, and `serde_json` for the answers' shape.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
