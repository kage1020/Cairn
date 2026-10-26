---
title: "互換性ティア"
---

Cairn は [日付ベースのバージョニング](/ja/spec/#2-つのバージョン軸) (`YYYY.M.PATCH`) のもとで、単一のリリーストレインを出荷します。CalVer には semver の「メジャー」にあたる軸がないので、**リリースで何を壊してよく、何を壊してはいけないか** は、バージョン番号ではなくこの文書で定めます。

プロジェクトの公開面は、どれも **Stable**、**Evolving**、**Internal** の 3 つのティアのいずれか 1 つに属します。ルールを決めるのはティアで、バージョン番号はいつ変わったかを記録するだけです。

## C.1 ティアの定義

### Stable

契約: **破壊的変更は 1 リリース前に `W_DEPRECATED` で予告し、早くても次の月次マイナーリリースまでは削除しない。** `W_DEPRECATED` は予約済みのコードで、最初の非推奨化とともに追加します。今はまだ出力するものがありません。

- Stable な面に入った破壊的変更は、翌月の CHANGELOG から参照しなければなりません (MUST)。
- 名前を変更する場合、非推奨期間中は旧名も動作し続けなければなりません (MUST)。
- 既定値は警告付きで変更してもかまいません (MAY)。意味は変更してはいけません (MUST NOT)。
- 利用者はマイナーバージョンで固定 (`cairn ~= 2026.7`) でき、少なくとも次の月次マイナーリリースまでは、入力がそのままコンパイルできると期待できます。

Stable な面:

- 仕様が規範として定める `.crn` の構文 (キーワード、ヘッダ、ブロックの種類、ブロックステートのプリミティブ、テーマとマテリアルのプリミティブ、エディションガード)。ただしエディションガードはまだ構文に含まれておらず、`@edition` はパースできません ([§10.7](/ja/spec/versioning-editions/#代替を書くとき))。
- ロックファイル (`<source>.lock`) の形式 (フィールド、ハッシュの入力、[§10.6](/ja/spec/versioning-editions/#106-来歴-provenance-とロック) が定める `verified` の意味)。
- `cairn compile`、`cairn check`、`cairn info` のフラグ名、引数の形、JSON 出力のスキーマ、終了コード。
- tier-1 のマテリアルトークン (ユーザーがソースに書く名前)。ただし [§7.2](/ja/spec/materials-themes/#72-正規トークン) の正規トークンには、どれが tier-1 かの区別がまだありません。
- 仕様に記載されたエラーと警告のコード (`E_*`、`W_*`)。

### Evolving

契約: **破壊的変更はどの月次マイナーリリースで出してもよく、CHANGELOG に列挙する。** 非推奨期間は約束しない。

- 利用者は、マイナーバージョンを上げる前に CHANGELOG を読むべきです (SHOULD)。
- パッチリリースに Evolving な面の破壊的変更を含めてはいけません (MUST NOT)。許されるのは月次マイナーリリースだけです。
- ある面を Evolving から Stable に昇格させるときは、この文書での移動と CHANGELOG の両方で示します。Stable から Evolving への降格は禁止で、行うには仕様の改訂が必要です。

Evolving な面:

- ドラフトと明記された仕様の章や節 ([未決事項](/ja/spec/open-issues/) と、本文中で変更の余地があると注記された節)。
- 新しく追加された `cairn` サブコマンドの、最初の 3 か月分の月次マイナーリリース。
- レジストリパックと制約カタログのファイル構成 (それらの *ハッシュ* は Stable なロックの意味に含まれますが、*内部構造* は Evolving です)。制約カタログはまだ同梱しておらず、ロックにはそのハッシュとしてゼロを記録します。
- tier-2 のトークン (tier-1 の解決先となる実装レベルの名前)。
- CLI の人間向け診断の書式と文言 (*コード* は Stable ですが、*文言* は違います)。
- `cairn-lang-cli` の `--features` の組 (cargo features) と、ワークスペースの各クレートの `[features]`。

### Internal

契約: **何も約束しない。どのリリースで何が変わってもよい。Internal な面に依存する利用者は、自分でバージョンを固定する責任を負う。**

Internal な面:

- ワークスペースの全クレートの Rust API (`cairn-lang-core`、`cairn-lang-nbt`、`cairn-lang-formats`、`cairn-lang-redstone`、`cairn-lang-lsp`、`cairn-lang-wasm`、`cairn-lang-tree-sitter`、`cairn-lang-docs`)。これらは、含まれる最初の月次マイナーリリースから crates.io に公開しますが、CLI の推移的な公開依存でない項目にはすべて `#[doc(hidden)]` を付けます。まだ付けた項目はありません。
- コンパイラの中間表現 (Intent IR、Semantic IR、block-array ピボットのレイアウト)。
- 増分ビルドキャッシュのディスク上の形式 (プロジェクト内の `target/` に相当するもの)。
- 言語サーバーの内部プロトコル (VS Code などとの間の LSP の通信仕様は Stable ですが、`cairn-lang-lsp` の内部の分け方は Stable ではありません)。

## C.2 マイルストーンごとのティア表

ある面のティアは固定ではなく、プロジェクトが [ロードマップ](/ja/roadmap/) を進むにつれて Stable に昇格していきます。この表が基準です。

| 面 | M1 まで | M2 (minimal build) | M3 (examples work) | M5 (DX) | M6 (redstone) |
|---|---|---|---|---|---|
| `.crn` の構文 (5〜9 章) | Evolving | Evolving | **Stable** | Stable | Stable |
| ロックファイルの形式 | Evolving | **Stable** | Stable | Stable | Stable |
| `cairn compile/check/info` のフラグ | Evolving | Evolving | **Stable** | Stable | Stable |
| 診断コード (`E_*`、`W_*`) | Evolving | Evolving | **Stable** | Stable | Stable |
| tier-1 のマテリアルトークン | Evolving | Evolving | **Stable** | Stable | Stable |
| LSP の通信プロトコル (LSP 標準) | — | — | — | **Stable** | Stable |
| レッドストーン DSL (14 章) | Evolving | Evolving | Evolving | Evolving | **Stable** |
| tier-2 のトークン、レジストリパックの構成 | Evolving | Evolving | Evolving | Evolving | Evolving |
| Rust API (全クレート) | Internal | Internal | Internal | Internal | Internal |

表の読み方: **ある行が初めて Stable になる列が、緩やかな約束です。** その列のマイルストーンに到達するまで、プロジェクトは非推奨期間を設けずにその面を変更できます。

Rust API の行は、計画中のロードマップの範囲では昇格しません。Cairn は Rust ライブラリとしてではなく、`cairn` CLI のバイナリと言語サーバーを通じて使うように設計されています。利用者が安定した組み込み用 API を求める場合、プロジェクトはそれを別途管理する新しい面として扱います。

## C.3 破壊的変更の告知方法

ティアにかかわらず、破壊的変更は CHANGELOG の `Breaking changes` 節に必ず載せます (MUST)。Stable な面の場合、これは 2 回目の告知です。1 回目は前のリリースの `Deprecations` 節です。

```text
## 2026.11.0 — 2026-11-01

### Breaking changes
- `cairn compile --java-target` を削除 (2026.10.0 で非推奨)。
  `--target` と `--edition java` を使ってください。

### Deprecations
- 矢印のない形の `slot` キーワードを非推奨にしました。`W_DEPRECATED_SLOT_ARROW` を出します。
  早くても 2026.12.0 までは削除しません。
```

`W_DEPRECATED` と `E_BREAKING` というコード自体は Stable です。どちらもまだ存在せず、それを必要とする最初の非推奨化や破壊的変更とともに追加します。新しいコードの追加は破壊的変更ではありませんが、既存のコードの意味を変えることは破壊的変更です。

## C.4 ティアの対象外

次の 2 つは、この表の対象外です。

- **仕様に挙動を合わせるバグ修正** は、利用者がバグのある挙動に依存していたとしても、破壊的変更ではありません。契約を定めるのは実装ではなく仕様です。
- **出力する `.nbt` / `.litematic` / `.schem` / `.mcstructure` のビット単位の一致** は、どのティアでも約束しません。現在あるのは `.nbt` と `.mcstructure` の出力だけです ([§12.1](/ja/spec/ecosystem-interop/#121-順方向の変換))。2 つのリリースが同じソースから構造の異なるファイルを生成してもかまいません。重要なのは、結果がターゲットの `(edition, version)` に対して有効であることと、ロックファイルの `resolved_ir_hash` と一致することです。[§10.6](/ja/spec/versioning-editions/#106-来歴-provenance-とロック) を参照してください。
