# cairn-lang-wasm

WebAssembly bindings for the Cairn compiler. Lets the [website
playground](../../website/README.md) (and any other browser-hosted tool) parse, compile, and
serialize Cairn sources without a server, sharing exactly the same
[`cairn-lang-core`](../cairn-lang-core/README.md) implementation as the CLI.

## Status

Skeleton. The crate currently re-exports [`cairn_version`](src/lib.rs); the parser/compiler
bindings will come online as `cairn-lang-core` lands them.

## Build

The crate is configured as both `cdylib` and `rlib`, so the standard
[`wasm-pack`](https://rustwasm.github.io/wasm-pack/) toolchain works:

```sh
wasm-pack build crates/cairn-lang-wasm --target web --release
```

The artifact is consumed by the website playground; integration is documented in
[`website/README.md`](../../website/README.md) once it is bootstrapped.

## API shape

A minimal browser-friendly surface is planned:

| Export | Purpose |
|---|---|
| `cairn_version()` | Returns the date-based Cairn release version. |
| `compile(source, edition, target)` | Returns `{ ok, diagnostics, ir }` for the playground. |
| `info(source, editions[])` | Mirrors `cairn info` ([versioning-editions §10.5](https://cairn.kage1020.com/spec/versioning-editions/)). |
| `import_raw(bytes, format)` | Faithful transliteration to raw-centric `.crn` ([ecosystem-interop §12.2](https://cairn.kage1020.com/spec/ecosystem-interop/)). |

Because the playground is a teaching surface as much as a compile surface, every export returns
diagnostics in the same "what is wrong / valid candidates / suggested fix" shape used by the CLI
and LSP ([lint](https://cairn.kage1020.com/spec/lint/)).

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
