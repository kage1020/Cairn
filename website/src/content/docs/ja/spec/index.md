---
title: "仕様書 2026.06 (ドラフト)"
description: AI が Minecraft の建築物を読み書きするための中間言語、Cairn の規範的な仕様。
---

これは **Cairn** の規範的な仕様です。Cairn は、AI が Minecraft の建築物を読み書きするための中間言語です。バイナリで 1 ブロックごとに 1 レコードを持つ NBT/SNBT の非効率さを避け、壁・屋根・対称性といった建築の知識をボクセルの世界に対応付けます。方針は **generation-first** で、そのため意図的に情報を落とす (lossy) 設計です。

## 読む順番

| # | 章 | 内容 |
|---|---|---|
| 1 | [目的とスコープ](/ja/spec/overview/) | 目的、スコープ、目標としないもの |
| 2 | [設計原則](/ja/spec/principles/) | P1–P5 |
| 3 | [アーキテクチャ](/ja/spec/architecture/) | 三層の IR、block-array のピボット |
| 4 | [コンパイルモデル](/ja/spec/compilation/) | フェーズ評価、ターゲット軸、屋根の展開 |
| 5 | [構文](/ja/spec/syntax/) | 字句規則、`key=value`、セレクタ、ヘッダ |
| 6 | [ブロックステート](/ja/spec/blockstate/) | 導出と上書き、intent と resolved、waterlogged |
| 7 | [マテリアルとテーマ](/ja/spec/materials-themes/) | スロット、正準語彙、テーマ |
| 8 | [エンティティ](/ja/spec/entities/) | 二段階のエンティティモデル、アンカーの規約 |
| 9 | [コンポーネント・編集・複数建築](/ja/spec/components-editing-sites/) | `def`、編集、`site` |
| 10 | [バージョンとエディション](/ja/spec/versioning-editions/) | ターゲットの扱い、ロック、エディション間の可搬性 |
| 11 | [Lint](/ja/spec/lint/) | 診断コードと制約の検証 |
| 12 | [エコシステム連携と逆変換](/ja/spec/ecosystem-interop/) | 既存形式への出力と、取り込み・逆変換 |
| 13 | [評価フレームワーク](/ja/spec/evaluation/) | 仕様をどう改善していくか |
| 14 | [レッドストーン](/ja/spec/redstone/) | 論理回路 |
| 15 | [未決事項](/ja/spec/open-issues/) | まだ決まっていないこと |
| — | [互換性ティア](/ja/spec/compatibility/) | 公開面ごとの Stable / Evolving / Internal |
| — | [用語集](/ja/spec/glossary/) | 章をまたいで使う用語の定義 |

## 規約

- 要求の強さを表す **MUST / SHOULD / MUST NOT / OPTIONAL** は、RFC 2119 の意味で使います。
- 言語の名前は **Cairn**、CLI は `cairn`、ソースファイルの拡張子は `.crn` です。
- 設計原則は `P1`–`P5` で参照します ([設計原則](/ja/spec/principles/))。
- この仕様の外から参照するときは、章をファイル名で、節を番号を除いた英語の見出しで書きます (`spec/lint` "Error vs warning")。`cairn docs` はこの形の引用をそのまま受け付け、インストール済みのコンパイラが実装している仕様からその節を表示します (`cairn docs spec/lint "Error vs warning"`)。日本語版は `ja/` の下にあり、`cairn docs ja/spec/lint "エラーと警告の区分"` のように引けます。

## 2 つのバージョン軸

Cairn 自身のリリースには、日付ベースのバージョン (CalVer) `YYYY.M[.PATCH]` を使います。月次リリースなら `2026.7`、その月のパッチなら `2026.7.1` です。文字列として並べると時系列順になります。1 つのリリースには、言語仕様、リファレンスコンパイラ、標準ライブラリ、`(edition, version)` ごとのレジストリと制約カタログがまとめて含まれます。

**Minecraft も日付ベースのバージョンに移行した** ため、2 つのバージョンは書式では見分けられません。そこで、書式ではなく、フィールド・フラグ・キーワードで区別します。

| | Cairn 自身のバージョン | Minecraft のターゲット |
|---|---|---|
| ロック | `cairn_version` | `mc_version` |
| ヘッダ | `@cairn` | `@requires` / `@intended_targets` |
| CLI | `cairn --version` | `--target` |

文章の中で紛らわしいときは、接頭辞を付けて `cairn:2026.06`、`mc:1.21.4` のように書きます。

この文書のバージョンは **2026.6 (ドラフト)** で、以前の `v0.2` という呼び方に代わるものです。`.crn` ファイルは、自分が対象とする言語のバージョンを `@cairn 2026.06` のように宣言してもかまいません (MAY)。この値は来歴 (provenance) を記録するためだけのもので、将来のコンパイラが正しくパースし、適切に警告できるようにするためにあります。そのため、どのパスもこの値で処理を分けませんが、`CalVer` として読めるかどうかは検査します ([構文 §5.3](/ja/spec/syntax/#53-ヘッダ))。
