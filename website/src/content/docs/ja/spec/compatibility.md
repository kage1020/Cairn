---
title: "互換性ティア"
---

Cairn は [日付ベースのバージョニング](/ja/spec/versioning-editions#101-ターゲットはコンパイル時パラメータ)
(`YYYY.M[.PATCH]`) の下で単一のリリーストレインを出します。CalVer には semver の「メジャー」軸が
ないため、**何を壊してよくて何を壊してはいけないかの範囲**はバージョン番号ではなくこの文書で規定
します。

プロジェクトの公開面はすべて **Stable**、**Evolving**、**Internal** の 3 ティアのどれか 1 つに
属します。ティアがルールを定め、バージョン番号はいつ起こったかを記録するだけです。

## C.1 ティアの定義

### Stable

契約: **breaking change は 1 リリース前に `W_DEPRECATED` で予告し、最短でも次の月次 minor まで
除去しない**。

- Stable に到達した breaking change は次月の CHANGELOG から参照しなければならない (MUST)。
- リネームは deprecation 期間中、旧名が機能し続けなければならない (MUST)。
- デフォルト値は warning 付きで変更してよい (MAY); 意味論は変更してはならない (MUST NOT)。
- 下流の利用者は minor で pin (`cairn ~= 2026.7`) でき、少なくとも次の月次 minor までは入力
  が変わらずコンパイルできることを期待してよい。

Stable な面:

- spec の normative な `.crn` 構文 (キーワード、ヘッダー、ブロック種別、blockstate プリミティブ、
  theme/material プリミティブ、エディションガード)。
- `build.cairn.lock` のファイルフォーマット (フィールド、ハッシュ入力、
  [§10.6](/ja/spec/versioning-editions#106-provenance-とロック-再現性) が定める `verified` の意味論)。
- `cairn compile`、`cairn check`、`cairn info` のフラグ名、引数の形、JSON 出力スキーマ、プロセス
  終了コード。
- 正規マテリアル語彙の tier-1 トークン (ユーザーがソースに書く名前)。
- spec に記載されたエラー/警告コード (`E_*`、`W_*`)。

### Evolving

契約: **breaking change は任意の月次 minor で出してよく、CHANGELOG に列挙する**。deprecation
期間の約束はない。

- 利用者は minor を上げる前に CHANGELOG を読むべき (SHOULD)。
- patch は Evolving の breaking を入れてはならない (MUST NOT); 月次 minor だけが許される。
- ある面が Evolving から Stable に昇格するときは、この文書での移動と CHANGELOG の両方で示す。
  Stable → Evolving への降格は禁止で、spec 改訂を要する。

Evolving な面:

- ドラフトと明記された spec 章・節 ([未決事項](/ja/spec/open-issues) と、本文中で「変更余地」と
  注記された節)。
- 新規導入された `cairn` サブコマンドの最初の 3 ヶ月分の月次 minor。
- registry pack と constraint catalog のファイルレイアウト (それらの *ハッシュ* は Stable な
  lock 意味論に入るが、*内部構造* は Evolving)。
- 正規語彙の tier-2 トークン (tier-1 が解決される実装レベル名)。
- CLI の人間可読な diagnostic フォーマットと文言 (*コード*は Stable、*文言*は違う)。
- `cairn-lang-cli` の `--features` セット (cargo features) とワークスペース crate 群の `[features]`。

### Internal

契約: **何も約束しない。任意のリリースで何でも変わりうる。Internal に依存する利用者は自分で pin
する責任を負う**。

Internal な面:

- ワークスペース全 crate の Rust API (`cairn-lang-core`、`cairn-lang-nbt`、`cairn-lang-formats`、
  `cairn-lang-redstone`、`cairn-lang-lsp`、`cairn-lang-wasm`)。これらは含まれる最初の月次 minor から crates.io
  に publish するが、CLI の推移的公開依存ではない項目はすべて `#[doc(hidden)]` を付ける。
- コンパイラの中間表現 (Intent IR、Semantic IR、block-array pivot のレイアウト)。
- 増分ビルドキャッシュのオンディスク形状 (ワークスペース内の `target/` 相当)。
- 言語サーバーの内部プロトコル (VS Code 等との LSP オンワイヤ仕様は Stable; `cairn-lang-lsp` の
  内部分割の仕方は Stable ではない)。

## C.2 マイルストーンごとのティア表

ある面のティアは固定ではなく、プロジェクトが [ロードマップ](/ja/roadmap/) を進むにつれて Stable
に昇格していきます。本表が正本です。

| 面 | 現在 (M1 前) | M2 (minimal build) | M3 (examples work) | M5 (DX) | M6 (redstone) |
|---|---|---|---|---|---|
| `.crn` 構文 (5〜9 章) | Evolving | Evolving | **Stable** | Stable | Stable |
| `build.cairn.lock` フォーマット | Evolving | **Stable** | Stable | Stable | Stable |
| `cairn compile/check/info` フラグ | Evolving | Evolving | **Stable** | Stable | Stable |
| diagnostic コード (`E_*`、`W_*`) | Evolving | Evolving | **Stable** | Stable | Stable |
| tier-1 マテリアル語彙 | Evolving | Evolving | **Stable** | Stable | Stable |
| LSP ワイヤプロトコル (LSP 標準) | — | — | — | **Stable** | Stable |
| レッドストーン DSL (14 章) | Evolving | Evolving | Evolving | Evolving | **Stable** |
| tier-2 語彙、registry pack レイアウト | Evolving | Evolving | Evolving | Evolving | Evolving |
| Rust API (全 crate) | Internal | Internal | Internal | Internal | Internal |

表の読み方: **ある行が初めて Stable になる列が、ソフトな約束**です。それより前の列が Evolving で
あるということは、その列が達成されるまで deprecation 期間なしで変更する権利をプロジェクトが
留保することを意味します。

Rust API の行は計画されたロードマップ中に昇格しません。Cairn は Rust ライブラリではなく `cairn`
CLI バイナリと言語サーバーを通じて消費される設計です。下流の利用者が安定した埋め込み API を望む
場合、プロジェクトはそれを別途トラックする新しい面として扱います。

## C.3 breaking はどう告知されるか

ティアに関わらず、breaking change は CHANGELOG の `Breaking changes` 節に必ず載せます (MUST)。
Stable な面ではこれが 2 回目の登場で、1 回目は前リリースの `Deprecations` 節です。

```text
## 2026.11.0 — 2026-11-01

### Breaking changes
- `cairn compile --java-target` を削除 (2026.10.0 で deprecated)。
  `--target` と `--edition java` を使ってください。

### Deprecations
- 矢印形のない `slot` キーワードを deprecate。`W_DEPRECATED_SLOT_ARROW` を出します。
  最短でも 2026.12.0 までは削除しません。
```

`W_DEPRECATED` と `E_BREAKING` のコード自体は Stable です。新しいコードの追加は breaking では
なく、既存コードの意味変更が breaking です。

## C.4 ティアに乗らないもの

次の 2 種類はこのマトリクスの外側に置かれます:

- **spec に挙動を合わせるバグ修正**は、利用者がバグった挙動に依存していたとしても breaking
  ではない。契約を定めるのは実装ではなく spec。
- **出力 `.nbt` / `.litematic` / `.schem` / `.mcstructure` のビット一致**はどのティアでも約束
  しない。2 つのリリースが同じソースから構造的に異なるファイルを生成してよく、肝心なのは結果が
  ターゲット `(edition, version)` に対して valid であることと lockfile の `resolved_ir_hash`
  と一致することです。[§10.6](/ja/spec/versioning-editions#106-provenance-とロック-再現性) を
  参照。
