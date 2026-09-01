# cairn-lang-wasm

WebAssembly bindings for the Cairn compiler. The intent is to let the [website
playground](../../website/README.md) (and any other browser-hosted tool) parse, compile, and
serialize Cairn sources without a server, sharing exactly the same
[`cairn-lang-core`](../cairn-lang-core/README.md) implementation as the CLI. None of that is
wired up yet — see **Status**.

## Status

Skeleton, and not yet buildable as WebAssembly. The crate holds one Rust function,
[`cairn_version`](src/lib.rs), and nothing that exposes it to a JavaScript caller: there is no
`wasm-bindgen` dependency, and the function carries neither `#[wasm_bindgen]` nor
`#[unsafe(no_mangle)] extern "C"`. The parser/compiler bindings will come online as
`cairn-lang-core` lands them, and the binding layer arrives with the first of them.

## Build

There is nothing to build for the browser yet. `wasm-pack` refuses a crate that does not depend
on `wasm-bindgen`, and a plain `cargo build --target wasm32-unknown-unknown` produces a module
with no callable export — `cairn_version` is a plain Rust symbol with no ABI a page can reach,
whatever the linker emits alongside it. The `cdylib` in `Cargo.toml` is the shape the crate will
need, not a shape it can be used in today.

When the binding layer lands, the command will be:

```sh
wasm-pack build crates/cairn-lang-wasm --target web --release
```

and the artifact will be consumed by the website playground; integration is documented in
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
