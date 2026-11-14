#!/bin/sh
# Build the playground's compiler module into src/playground/pkg and hold it to its size budget.
#
# `--install` first installs what the build needs and the host lacks: rustup with the toolchain
# rust-toolchain.toml pins, the wasm32 target, and wasm-pack. A deploy host starts with none of
# them; on a developer's machine the script names what is missing instead.
set -eu

# The largest the module may be, in bytes: its size, 2,689,460, with a sixth on top for growth.
WASM_SIZE_BUDGET=3145728
WASM_PACK_VERSION=0.15.0

website=$(cd "$(dirname "$0")/.." && pwd)
root=$(dirname "$website")
out="$website/src/playground/pkg"

install=false
for arg in "$@"; do
  case "$arg" in
    --install) install=true ;;
    *) echo "error: unknown argument \`$arg\`; the only one is \`--install\`" >&2; exit 2 ;;
  esac
done

missing() {
  echo "error: \`$1\` is not installed. $2, or run \`pnpm run build:wasm --install\`" >&2
  exit 1
}

if ! command -v rustup >/dev/null 2>&1; then
  if [ "$install" = false ]; then missing rustup "Install it from https://rustup.rs"; fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --profile minimal --default-toolchain none
  PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
fi

cd "$root"
if [ "$install" = true ]; then rustup toolchain install; fi
rustup target add wasm32-unknown-unknown

if ! command -v wasm-pack >/dev/null 2>&1; then
  if [ "$install" = false ]; then
    missing wasm-pack "Run \`cargo install --locked wasm-pack@$WASM_PACK_VERSION\`"
  fi
  cargo install --locked "wasm-pack@$WASM_PACK_VERSION"
fi

wasm-pack build crates/cairn-lang-wasm --target web --release --no-pack --out-dir "$out"

module="$out/cairn_lang_wasm_bg.wasm"
size=$(wc -c <"$module" | tr -d ' ')
if [ "$size" -gt "$WASM_SIZE_BUDGET" ]; then
  echo "error: $module is $size bytes, over its $WASM_SIZE_BUDGET-byte budget. Find what grew \
before raising WASM_SIZE_BUDGET in website/scripts/build-wasm.sh" >&2
  exit 1
fi
echo "$module: $size of $WASM_SIZE_BUDGET budgeted bytes"
