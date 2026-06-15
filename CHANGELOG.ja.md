# 変更履歴

> 言語: **日本語** ([English](CHANGELOG.md))
>
> 英語版が source of truth です。

Cairn の重要な変更はここに記録されます。Cairn は日付ベースバージョニング (CalVer)
`YYYY.0M[.PATCH]` を採用します。これは「言語仕様 + リファレンスコンパイラ + 標準ライブラリ +
レジストリ/制約パック」をまとめたバンドルのバージョンであり、Minecraft のターゲットバージョンとは
別軸です。

## [Unreleased]

**2026.07.0** (M1 — *source parses*) に向けた作業。リファレンスコンパイラの最初の実行可能スライス
としてここに着地: `cairn-core` に lexer / parser / AST が入り、`cairn parse <file>` が
`examples/` 配下のすべてのファイルから AST を JSON として出力できる。

### 追加

- `cairn-core::lex` — インデントを認識する lexer。トークンにバイトスパンと 1 始まりの行/列位置を
  付与する。タブインデントと奇数スペースのインデントは拒否。
- `cairn-core::ast` — 表層レベル AST (`Module`, `Header`, `Item`, `ThemeRule`, `Command`,
  `Arg`, `Value`, `Extra`, `Expr`)。全型に `serde::Serialize` を derive。
- `cairn-core::parse` — ハンドロールの再帰下降パーサ。ヘッダ (`@cairn`, `@requires`,
  `@intended_targets`)、`theme` / `def` / `site` / `struct` ブロック、ネストされたコマンド、
  ブラケットセレクタ、センサーの `-> binding` 末尾、位置引数 (`connect a to b`)、
  `logic` / `assert truth|always` 特殊形をカバー。
- `cairn parse <file> [--format json|debug]` — `clap` derive で実装した CLI サブコマンド。
- エンドツーエンドのカバレッジ: lexer テスト 11 件、parser ユニットテスト 14 件、
  `examples/` 配下に対する `insta` スナップショット 4 件、すべての example をバイナリ経由で
  ラウンドトリップさせる CLI 統合テスト 6 件。

## 2026.06 (ドラフト)

最初の公開設計仕様。言語仕様はオープンに設計中で、リファレンスコンパイラはまだありません。

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
  します。`cairn-wasm` バインディングを将来取り込むためのプレイグラウンドプレースホルダ、`main`
  への push で自動デプロイする Cloudflare Git 連携付き。
