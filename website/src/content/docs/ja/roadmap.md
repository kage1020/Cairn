---
title: ロードマップ
description: 日付駆動の小さく頻繁なリリース。月次 minor と必要に応じた patch、「ソースが parse できる」から「レッドストーンが simulate できる」までの 6 マイルストーン。
---

Cairn は [日付ベースのバージョニング](/ja/spec/versioning-editions#101-ターゲットはコンパイル時パラメータ)
`YYYY.0M[.PATCH]` の下で **月次 minor リリース** を出します。各月は準備できたものを出し、任意の
「1.0」を待つために何かを抑え込むことはしません。月次スケジュールを横断する 6 つの名前付きマイル
ストーンが、カレンダーの刻みより耐久性のある約束を提供します。

> スケジュールは計画であって約束ではありません。コンパイラはオープンに実装されており、遅延は起こり
> ます。実際に何がリリースされたかの一次情報は
> [CHANGELOG](https://github.com/kage1020/Cairn/blob/main/CHANGELOG.ja.md) です。

## リリースサイクル

- **月次 minor** (`2026.MM.0`) は月初に出します。リリース PR は cron で自動生成され、人間レビュー
  を経てマージされます。
- **patch** (`2026.MM.N`, `N ≥ 1`) は関連コミットが入り次第 `main` から切り出します。代表的な
  トリガーは registry/constraint pack の更新、リグレッション、セキュリティ修正です。月あたりの
  patch 回数に上限はありません。
- **チャネル:** `stable` のみです。Cairn は別系統の nightly/beta を走らせません。まだ安定していない
  振る舞いはリリースチャネルではなく [互換性ティア](/ja/spec/compatibility) で示します。
- **バックポート:** ありません。最新リリースがサポート対象です。過去の `2026.MM.*` 系列にはそれ以上
  の patch を当てません。

## マイルストーン

各マイルストーンは「Cairn が信頼できる形で X を実現できる」というゲートで、対応する月のリリースが
出た時点で達成と見なします。バージョン番号と切り離して名前を付けることで、月次スケジュールがずれて
もロードマップの語彙が安定します。

| マイルストーン | 達成想定 | そこで得られるもの |
|---|---|---|
| **M1 — source parses** | 2026.07.0 | `examples/` のすべてのファイルに対して `cairn parse` が AST を返す |
| **M2 — minimal build** | 2026.10.0 | `cairn compile` が床と壁だけの単室構造に対して Java の `.nbt` と lockfile を書き出す |
| **M3 — examples work** | 2027.01.0 | `cottage`, `themed-tower`, `village` が `cairn compile --edition java` を通り、Minecraft でロードできる |
| **M4 — Java/Bedrock parity** | 2027.02.0 | 同じ DSL ソースから両エディションの有効な出力が出る。parity 表が埋まり、エディション別 theme fallback が動く |
| **M5 — developer experience** | 2027.03.0 | `cairn-lsp` が少なくとも 1 エディタ (VS Code) で diagnostics と completion を提供 |
| **M6 — redstone simulates** | 2027.05.0 | 論理レッドストーンの synthesis、place-and-route、tick simulator が揃う。`redstone-door` が verify される |

## 月別スコープ

下表は実装計画です。マイルストーンゲートを跨がない月でも月次 minor は出るので、マイルストーン表より
密になります。

| リリース | 追加スコープ |
|---|---|
| **2026.07.0** | `cairn-core` の lexer/parser、`cairn parse` サブコマンド (AST 表示のみ)。リリース自動化が稼働。 |
| **2026.08.0** | Intent IR、構文バリデーション、`cairn check`。 |
| **2026.09.0** | Semantic 層、materials/themes の基礎、`cairn info` が三軸を返す。 |
| **2026.10.0** | block-array pivot、Java backend (壁と床のみ)、lockfile (`build.cairn.lock`)。 |
| **2026.11.0** | `cottage.crn` が `--edition java` で end-to-end コンパイルできる。 |
| **2026.12.0** | registry pack 取り込み、fail-loud + nearest-valid 候補。 |
| **2027.01.0** | `examples/` 全部が Java で動く。**M3**。 |
| **2027.02.0** | Bedrock backend、parity 表、エディション別 theme fallback。**M4**。 |
| **2027.03.0** | `cairn-lsp` 最小版 (diagnostics + completion)、VS Code 拡張。**M5**。 |
| **2027.04.0** | レッドストーン論理層、組み合わせ回路の合成と place-and-route。 |
| **2027.05.0** | レッドストーン tick simulator、sequential macros、`redstone-door` 検証。**M6**。 |
| **2027.06.0** | `cairn-wasm` + ブラウザ playground (docs サイト上でライブコンパイル)。 |

`2027.06.0` より先のスケジュールは意図的に描いていません。M6 まで到達すれば仕様駆動の部分はほぼ
完了し、その先は実利用のフィードバックを元にロードマップを引き直します。

## スケジュールはどう守られているか

リリース戦略そのものが自動化されています:

1. **月次 minor PR** は毎月 1 日 `04:17 UTC` の GitHub Actions cron で立ち上がります (GHA cron が
   遅延・スキップしやすい時刻ちょうどを意図的に外しています)。
2. **バージョン**は既存タグから計算します: `v2026.MM.*` タグがまだなければ次は `2026.MM.0`、
   既にあれば次の patch を採ります。
3. **release-plz** がバージョン bump と changelog を生成し、`--version-overrides` で算出した
   バージョンを適用して PR を開きます。
4. **人間レビュー**は月次 minor で必須です。`main` への push から派生する patch も同じ PR フロー
   を辿り、マージで publish パイプラインが発火します。
5. **Publish** は `v*` タグで起動します: Linux/macOS/Windows × `x86_64`/`aarch64` の
   クロスコンパイル、sigstore による署名、GitHub Release への添付、ワークスペース全 crate の
   `cargo publish`。

このスケジュールを通じての互換性は、別途 [互換性ティア](/ja/spec/compatibility) が規定します。
`.crn` 構文と lockfile は **Stable** ルールで進化し、Rust API は M3 まで **Internal**
(`#[doc(hidden)]`)、その他のほとんどは当面 **Evolving** に置かれます。
