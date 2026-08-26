---
title: "12. エコシステム連携と逆方向変換"
---

## 12.1 順方向

block-array IR をシリアライズすると `.nbt` / `.litematic` / `.schem` / `.mcstructure` が出ます
([アーキテクチャ](architecture))。各フォーマットは単なるシリアライザで、既存フォーマットはピボットの
周りに置かれる追加バックエンドです。

## 12.2 逆方向: コンパイラは写し取り、リフトは LLM

コンパイラは「ボクセルを見てこれは壁だと判定する」コンピュータビジョンを作りません。保守不能になるか
らです。コンパイラが実装するのは堅牢な忠実写し取り、検証、voxel-diff です。意味のリフトは raw 中心の
DSL に対する LLM のリファクタリングとして行います。言語のドッグフーディングであり、P5 の自己修正ルー
プと整合します。

```text
cairn import house.litematic --mode raw    → house.raw.crn     # fill/raw_block 中心
(LLM が house.raw.crn を意味 DSL にリファクタ) → house.lifted.crn
cairn compile house.lifted.crn --edition java --target 1.21.4
cairn diff-blocks house.litematic house.lifted.crn             # voxel XOR → 自己修正へ
```

compile → diff → patch のループはこう報告します。

```text
E_DECOMPILE_DIFF: block IoU = 0.962 < threshold 0.985
  missing bbox=(12,4,3)..(18,6,3) mat=glass_pane → likely window repeat too small
  Suggested patch: edit window[id=front_windows] set repeat=4
```

収束閾値は block IoU ≥ 0.985、`state_accuracy` ≥ 0.995、残存 raw ≤ 5% です。完全一致は要求しません。
残差は `raw_fill id=residual_* origin=imported` として明示的に保持します。

## 12.3 写し取りの 3 段階

命名が、写し取りとリフトの境界です。

| 段 | 内容 | 上限 |
|---|---|---|
| **L0 — raw cells** | 1 行 1 ボクセル。LLM の文脈には大きすぎるので中間表現のみ。 | — |
| **L1 — 空間圧縮** | fill 集約、AABB パレット圧縮、`resolved_state` → `intent_state` の逆変換 (`stair facing=east half=top`)、対称性と周期を `raw_repeat` へ畳む。**命名はしません。** | コンパイラの上限。 |
| **L2 — 意味リフト** | fill → `wall`、repeat → `def` / `use`、具体ブロック → `mat_slot` + `theme`。 | LLM の上限。 |

```
# L1 — 命名なし、決定論的
raw_repeat id=r03 count=5 step=3,0,0: raw_fill mat=@glass_pane from=0,2,0 to=1,3,0
# L2 — LLM が命名し、意味を与える
window id=front_windows side=front mat_slot=glass repeat=5 ...
```

## 12.4 取り込み時のスタンプと落とし穴

取り込み時に `(edition, version)` と provenance を block-array IR にスタンプします (`.litematic` →
java、`.mcstructure` → bedrock、`.schem` → java)。これが取り込みを再現性につなぎます
([バージョンとエディション](versioning-editions))。

**取り込みを「作者の意図の復元」として提示してはいけません。** これが最大の落とし穴です。回復できる
のはボクセルと一部の規則性だけであり、CLI は `W_SEMANTIC_LOSS` でそう告げます。

他の規則:

- 取り込み起源の `raw_fill` は `origin=imported` / `residual` で隔離し、ファーストクラスの設計 DSL
  としては扱いません。
- Litematica の複数 region とサブリージョンの offset はフラット化せず provenance として保持し、
  region は `site` または複数の struct に対応付けます。
- エンティティを含む schematic では、block IoU だけで成功と判定してはいけません。エンティティの指標
  を別に持ち、ファーストクラスのエンティティ ([エンティティ](entities)) だけを取り出します。チェスト
  の中身とコマンドブロックは捨てます。
- 巨大な schematic (48³ 超、村全体) を一度にリフトすると LLM の文脈が破綻します。チャンク分割 →
  チャンクごとの L1 → パートごとのリフト → `site` での結合を、ストリーミングパースの上でオーケスト
  レーションする必要があります。
- 1.13 のフラット化以前の数値 ID `.schematic` は v1 では未対応です ([目的とスコープ](overview))。
