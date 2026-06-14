---
title: "Cairn — Minecraft 建築 DSL 仕様 2026.06 (ドラフト)"
---

**Cairn** (ケルン: 場所を示すために意図的に積み上げられた石) は、AI が Minecraft の建築物を読み書きする
ための中間言語の正規仕様です。NBT/SNBT の非効率さ (バイナリ、1ブロック1レコードのフラット列) を回避し、
建築知識 (壁、屋根、対称性) とボクセル世界とを揃えます。アプローチは **generation-first (lossy)** です。

## 読む順番

| # | ファイル | 内容 |
|---|---|---|
| 1 | [overview.ja.md](overview) | 目的、スコープ、非目標 |
| 2 | [principles.ja.md](principles) | 設計原則 P1–P5 |
| 3 | [architecture.ja.md](architecture) | 三層 IR + block-array 普遍ピボット |
| 4 | [compilation.ja.md](compilation) | フェーズ評価、ターゲット軸 |
| 5 | [syntax.ja.md](syntax) | 字句、key=value、セレクタ、ヘッダ |
| 6 | [blockstate.ja.md](blockstate) | 導出 + 上書き、intent/resolved、waterlogged |
| 7 | [materials-themes.ja.md](materials-themes) | スロット、正規語彙、テーマ |
| 8 | [entities.ja.md](entities) | 二段モデル、アンカー規約 |
| 9 | [components-editing-sites.ja.md](components-editing-sites) | def、編集、複数建築 |
| 10 | [versioning-editions.ja.md](versioning-editions) | バージョン/エディション戦略、ロック |
| 11 | [lint.ja.md](lint) | Lint と制約検証 |
| 12 | [ecosystem-interop.ja.md](ecosystem-interop) | エコシステム連携、逆方向変換 |
| 13 | [evaluation.ja.md](evaluation) | 評価フレームワーク |
| 14 | [redstone.ja.md](redstone) | レッドストーン (論理回路) |
| 15 | [open-issues.ja.md](open-issues) | 未決事項 |
| — | [glossary.ja.md](glossary) | 用語集 |

## 用語と規約

- 要求水準語: **MUST / SHOULD / MUST NOT / OPTIONAL** (RFC 2119 の意味)。
- 言語名は **Cairn**、CLI ツールは `cairn`、ソースファイル拡張子は `.crn`。
- 設計原則は `P1`–`P5` で参照 ([principles.ja.md](principles))。

## バージョニング

Cairn 自身のリリースは **日付ベースバージョニング (CalVer)** `YYYY.0M[.PATCH]` を採用します。
- 例: `2026.06` (月次リリース)、`2026.06.1` (月内パッチ)。文字列として時系列順にソートされます。
- 本ドキュメントは **2026.06 (ドラフト)** で、旧 `v0.2` ラベルを置き換えます。
- 1 リリースは「言語仕様 + リファレンスコンパイラ + 標準ライブラリ + `(edition,version)`
  レジストリ/制約カタログ」のバンドルです。`cairn --version` とロックの `cairn_version` に現れます
  ([versioning-editions.ja.md](versioning-editions))。

**Minecraft のターゲットバージョンとは別軸** (混同しないでください):
- **Cairn version** `2026.06` — Cairn ツール自身のリリース。
- **MC target** — 出力先 Minecraft (`--edition java --target <version>`;
  [versioning-editions.ja.md](versioning-editions))。

**Minecraft 自身も最新リリースから日付ベースバージョンに移行したため、両者をフォーマットで見分ける
ことはできません**。バージョンは常に **フィールド/フラグ/キーワード** で区別します:
- ロック: `cairn_version` vs `mc_version`
- ヘッダ: `@cairn` vs `@requires` / `@intended_targets`
- CLI: `cairn --version` (Cairn 自身) vs `--target` (MC)

文中で曖昧になる場合は接頭辞を付けます: `cairn:2026.06` / `mc:<version>`。

`.crn` ファイルは `@cairn 2026.06` (それが書かれた Cairn 言語のバージョン) を宣言してもかまいません。
これは MC バージョン用ヘッダ `@requires` / `@intended_targets` とは別軸で、将来のコンパイラが正しく
パース/警告できるようにするための provenance として存在します ([syntax.ja.md](syntax))。
