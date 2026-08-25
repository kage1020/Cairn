---
title: "仕様書 — 2026.06 (ドラフト)"
description: AI が Minecraft の建築物を読み書きするための中間言語 Cairn の正規仕様。
---

**Cairn** の正規仕様です。AI が Minecraft の建築物を読み書きするための中間言語で、NBT/SNBT の非効率さ
(バイナリ、1 ブロック 1 レコード) を回避し、建築知識 (壁、屋根、対称性) をボクセル世界と揃えます。
アプローチは **generation-first** であり、したがって意図的に lossy です。

## 読む順番

| # | 章 | 内容 |
|---|---|---|
| 1 | [目的とスコープ](overview) | 目的、スコープ、非目標 |
| 2 | [設計原則](principles) | P1–P5 |
| 3 | [アーキテクチャ](architecture) | 三層 IR、block-array ピボット |
| 4 | [コンパイルモデル](compilation) | フェーズ評価、ターゲット軸、屋根の立体化 |
| 5 | [構文](syntax) | 字句規則、`key=value`、セレクタ、ヘッダ |
| 6 | [ブロックステート](blockstate) | 導出と上書き、intent と resolved、waterlogged |
| 7 | [マテリアルとテーマ](materials-themes) | スロット、正準語彙、テーマ |
| 8 | [エンティティ](entities) | 二段のエンティティモデル、アンカー規約 |
| 9 | [コンポーネント・編集・複数建築](components-editing-sites) | `def`、編集、`site` |
| 10 | [バージョンとエディション](versioning-editions) | ターゲット戦略、ロック、可搬性 |
| 11 | [Lint](lint) | 診断コードと制約検証 |
| 12 | [エコシステム連携](ecosystem-interop) | インポート、逆方向変換 |
| 13 | [評価フレームワーク](evaluation) | 仕様をどう反復するか |
| 14 | [レッドストーン](redstone) | 論理回路 |
| 15 | [未決事項](open-issues) | まだ決まっていないこと |
| — | [互換性ティア](compatibility) | 公開面ごとの Stable / Evolving / Internal |
| — | [用語集](glossary) | 章をまたぐ定義語 |

## 規約

- 要求水準語 **MUST / SHOULD / MUST NOT / OPTIONAL** は RFC 2119 の意味で使います。
- 言語名は **Cairn**、CLI は `cairn`、ソースファイルは `.crn` です。
- 設計原則は `P1`–`P5` で参照します ([設計原則](principles))。

## 2 つのバージョン軸

Cairn 自身のリリースは日付ベースバージョニング (CalVer) `YYYY.M[.PATCH]` です。月次リリースなら
`2026.7`、月内パッチなら `2026.7.1` で、文字列として時系列順にソートされます。1 リリースは言語仕様、
リファレンスコンパイラ、標準ライブラリ、`(edition, version)` のレジストリ/制約カタログのバンドルです。

**Minecraft も日付ベースバージョンに移行した** ため、両者をフォーマットで見分けることはできません。
形ではなく、フィールド / フラグ / キーワードで区別します。

| | Cairn 自身のバージョン | Minecraft ターゲット |
|---|---|---|
| ロック | `cairn_version` | `mc_version` |
| ヘッダ | `@cairn` | `@requires` / `@intended_targets` |
| CLI | `cairn --version` | `--target` |

文中で曖昧になる場合は接頭辞を付けます (`cairn:2026.06` と `mc:1.21.4`)。

本ドキュメントは **2026.6 (ドラフト)** で、旧 `v0.2` ラベルを置き換えます。`.crn` ファイルは
`@cairn 2026.06` — それが書かれた対象の言語バージョン — を宣言してもかまいません。将来のコンパイラが
正しくパース/警告できるようにするための provenance であり、それだけです ([構文 §5.3](syntax))。
