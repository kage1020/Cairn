# 変更履歴

> 言語: **日本語** ([English](CHANGELOG.md))
>
> 英語版が source of truth です。

書式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に従います (release-plz が
リリースエントリを綺麗に追記できるようにするため)。Cairn は日付ベースバージョニング (CalVer)
`YYYY.0M[.PATCH]` を採用します。これは「言語仕様 + リファレンスコンパイラ + 標準ライブラリ +
レジストリ/制約パック」をまとめたバンドルのバージョンであり、Minecraft のターゲットバージョンとは
別軸です。

## [Unreleased]

最初の公開ナンバー付きリリースは **`2026.07.0`** (予定) です。それまでの間、本節はそのリリースに
向けてリポジトリに積まれた内容を記録します。`cairn-lang-*` クレートはまだ crates.io に公開されて
おらず、`[workspace.package].publish` は `false` のため `0.0.0` プレースホルダが外部に漏れる
ことはありません。`2026.07.0` のリリース PR で publish を `true` にフリップします。

### Added

- コアモデル: 意図を宣言し、コンパイラがブロックステート、座標、物理を解決する。
- 三層 IR (Intent → Semantic/Theme → block-array pivot)、フェーズ順評価。
- 構文: 先頭キーワード + 必須の `key=value`、セレクタ、任意ヘッダ (`@cairn`, `@requires`,
  `@intended_targets`)。
- ブロックステート: デフォルトは導出、override-promotion、`intent_state` / `resolved_state`。
- マテリアル & テーマ: `mat_slot` スロット、二段の正規語彙、CSS 的なテーマバインディング。
- エンティティ: ファーストクラスの装飾エンティティと汎用 `spawn`、アンカー規約。
- コンポーネント、編集 (安定アドレス + パッチ文法)、複数建築の `site` 配置。
- バージョニング & エディション: `(edition, version)` のコンパイル時ターゲット、recompile-don't-
  transcode、近い妥当値を伴う fail-loud、DataVersion を正規順序キーとする (Minecraft の日付ベース
  バージョン移行を吸収)、provenance + lockfile。
- Java/Bedrock を 1 ソースから、エディションごとのバックエンドと QC フリーの安全セルライブラリで。
- レッドストーン: 論理サブ言語 (signal graph → 合成 → place-and-route)、組み合わせ + 厳選された
  順序マクロ、ヘッドレス tick simulator による検証。
- エコシステム連携: 主要フォーマットへの書き出し、忠実な写し取りと LLM によるリフトの import。
- 評価: ヘッドレスな幾何/レッドストーン simulator が定量的な仕様反復を駆動する。
- ドキュメント: クレート別 README、
  [開発者ガイド](https://cairn.kage1020.com/development/)、
  [チュートリアル](https://cairn.kage1020.com/tutorial/)、
  [実用例](https://cairn.kage1020.com/examples/)、横断
  [用語集](https://cairn.kage1020.com/spec/glossary/)。
- ユーザー向け文書の日本語ミラー (README、CONTRIBUTING、CHANGELOG、仕様各章、用語集、
  チュートリアル、サンプル目次)。英語が source of truth。
- [`website/`](website/README.md) のドキュメントサイト (Astro + Starlight、英語 + 日本語)。
  Cloudflare Pages の <https://cairn.kage1020.com/> にデプロイ。仕様書、チュートリアル、開発者
  ガイド、サンプル目次は [`website/src/content/docs/`](website/src/content/docs/) で直接編集
  します。`cairn-lang-wasm` バインディングを将来取り込むためのプレイグラウンドプレースホルダ、
  `main` への push で自動デプロイする Cloudflare Git 連携付き。
- リリース戦略: 月次 minor (`YYYY.0M.0`) は毎月 1 日 04:17 UTC の GitHub Actions cron、
  patch (`YYYY.0M.N`) は適格コミットの `canary` push で随時。リリース PR
  (`release-plz-*` → `canary`) は人間レビューを経てマージされ、release-plz が publish を行い、
  workflow が `main` を `canary` に fast-forward することで `main` は公開済み状態のみを映す。
- ワークスペースのバージョンは `[workspace.package].version` と `[workspace.dependencies]` で
  一元管理。バイナリは Linux/macOS/Windows × `x86_64`/`aarch64` でクロスコンパイル、sigstore
  keyless で署名し GitHub Release に添付する。
- クレート接頭辞: `cairn-lang-*` (`cairn-lang-core`、`cairn-lang-cli`、`cairn-lang-nbt`、
  `cairn-lang-formats`、`cairn-lang-redstone`、`cairn-lang-lsp`、`cairn-lang-wasm`)。
  `cargo install cairn-lang-cli` でインストールされるユーザー向けバイナリ名は引き続き `cairn`。
- [spec/compatibility](https://cairn.kage1020.com/ja/spec/compatibility/) に互換性ティアを記載:
  公開面はすべて **Stable**、**Evolving**、**Internal** のいずれかに属し、各面がいつ Stable に
  昇格するかをマイルストーン別の表で明示する。
- [ロードマップ](https://cairn.kage1020.com/ja/roadmap/) を公開。M1〜M6 のマイルストーンと
  `2027.06.0` までの月別スコープを掲載。

### Added (M1 — *source parses* の実行可能スライス)

- `cairn-lang-core::lex` — インデントを認識する lexer。トークンにバイトスパンと
  1 始まりの行/列位置を付与する。タブインデントと奇数スペースのインデントは拒否。
- `cairn-lang-core::ast` — 表層レベル AST (`Module`, `Header`, `Item`, `ThemeRule`,
  `Command`, `Arg`, `Value`, `Extra`, `Expr`)。全型に `serde::Serialize` を derive。
- `cairn-lang-core::parse` — ハンドロールの再帰下降パーサ。ヘッダ (`@cairn`, `@requires`,
  `@intended_targets`)、`theme` / `def` / `site` / `struct` ブロック、ネストされたコマンド、
  ブラケットセレクタ、センサーの `-> binding` 末尾、位置引数 (`connect a to b`)、
  `logic` / `assert truth|always` 特殊形をカバー。
- `cairn parse <file> [--format json|debug]` — `clap` derive で実装した CLI サブコマンド。
  エラー出力は `gcc`/`clang` スタイル (`error: file:line:col: メッセージ`) で、エディタの
  ジャンプ機能から直接エラー位置を開ける。
- エンドツーエンドのカバレッジ: lexer テスト 17 件、parser ユニットテスト 27 件、
  `examples/` 配下に対する `insta` スナップショット 4 件、すべての example をバイナリ経由で
  ラウンドトリップさせる CLI 統合テスト 6 件。

### 堅牢化

- Lexer は `\n` / `\r\n` / 単独 `\r` を等価に 1 つの論理改行として扱う (Windows で
  `core.autocrlf=true` の checkout でも Linux と同じく字句解析できる)。
- 列カウンタはバイトではなく Unicode スカラー値 (`char`) で進む。文字列リテラル内の
  日本語が後続トークンの列番号を破壊しない。
- `UnexpectedChar` は実際の `char` (マルチバイト UTF-8 含む) を報告する。
  以前のバイトを単純に `char` キャストしていた挙動を廃止。
- 1 コマンド行に `-> binding` 末尾は 1 つまで。2 回目の `->` は黙って上書きせず
  ハードエラー。
- `@cairn` / `@requires` / `@intended_targets` は空値を拒否、
  `@intended_targets` はリスト後の末尾トークンも拒否。
- パーサのエラーメッセージは `TokenKind` の人間向け Display を使用
  (`expected `=`, got identifier `foo``)。Rust `Debug` の生表記は露出しない。
- `ast` / `lex` / `error` の公開 enum はすべて `#[non_exhaustive]` 化。後続マイルストーンで
  variant を追加しても下流クレートの破壊的変更にならない。
- `LexError` / `ParseError` に `position()` / `user_message()` アクセサを追加。CLI や
  将来の LSP が Display 文字列を再パースせずに診断を組み立てられる。

### Changed（AST 表面 — `cairn parse` の JSON / YAML 出力に影響）

- `TruthRow.output` の JSON シリアライゼーションが整数 `0` / `1` から論理値 `true` / `false`
  に変更。`cairn parse --format json` の出力をツールから読み込み、当該フィールドを整数前提で
  扱っているコードは更新が必要。
- `Position.line` / `Position.col`、`Value::Size.w` / `Value::Size.h`、`assert always(...)`
  の `within` バウンドは Rust 側で `NonZeroU32` 化。ワイヤ上の表現は引き続き素の整数なので
  JSON / YAML 形状は変わらない。
- `@cairn` / `@requires` ヘッダの値は Rust 側で `RawVersion` / `RawRequirement` ニュータイプに
  ラップ。`serde(transparent)` なので外部消費側から見ると素の文字列のままで形状変化なし。
