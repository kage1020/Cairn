---
title: "12. エコシステム連携と逆変換"
---

## 12.1 順方向の変換

block-array IR をシリアライズすると、`.nbt` / `.litematic` / `.schem` / `.mcstructure` が出力されます ([アーキテクチャ](/ja/spec/architecture/))。各形式は単なるシリアライザで、既存の形式は、ピボットの周りに追加するバックエンドという位置付けです。

## 12.2 逆方向の変換: コンパイラは写し取り、LLM がリフトする

コンパイラは、ボクセルを見て「これは壁だ」と判定するようなコンピュータビジョンは作りません。保守できなくなるからです。コンパイラが実装するのは、堅牢で忠実な写し取り、検証、ボクセル単位の差分です。意味を付けて引き上げる作業 (リフト) は、raw を中心にした DSL を LLM がリファクタリングする形で行います。言語を自分で使って確かめることにもなり、P5 の自己修正ループとも一致します。

```text
cairn import house.litematic --mode raw    → house.raw.crn     # fill/raw_block 中心
(LLM が house.raw.crn を意味 DSL にリファクタ) → house.lifted.crn
cairn compile house.lifted.crn --edition java --target 1.21.4
cairn diff-blocks house.litematic house.lifted.crn             # voxel XOR → 自己修正へ
```

compile → diff → patch のループは、次のように報告します。

```text
E_DECOMPILE_DIFF: block IoU = 0.962 < threshold 0.985
  missing bbox=(12,4,3)..(18,6,3) mat=glass_pane → likely window repeat too small
  Suggested patch: edit window[id=front_windows] set repeat=4
```

収束の閾値は、block IoU ≥ 0.985、`state_accuracy` ≥ 0.995、残った raw ≤ 5% です。完全な一致は求めません。残差は `raw_fill id=residual_* origin=imported` として明示的に残します。

## 12.3 写し取りの 3 段階

写し取りとリフトの境界は、名前を付けるかどうかです。

| 段階 | 内容 | 上限 |
|---|---|---|
| **L0: raw セル** | 1 行に 1 ボクセル。LLM の文脈には大きすぎるので、中間表現としてだけ使う。 | — |
| **L1: 空間的な圧縮** | fill への集約、AABB によるパレット圧縮、`resolved_state` から `intent_state` への逆変換 (`stair facing=east half=top`)、対称性と周期性の `raw_repeat` への畳み込み。**名前は付けない。** | コンパイラにできる上限。 |
| **L2: 意味のリフト** | fill → `wall`、repeat → `def` / `use`、具体的なブロック → `mat_slot` + `theme`。 | LLM にできる上限。 |

```
# L1: 名前を付けない。決定的
raw_repeat id=r03 count=5 step=3,0,0: raw_fill mat=@glass_pane from=0,2,0 to=1,3,0
# L2: LLM が名前を付け、意味を与える
window id=front_windows side=front mat_slot=glass repeat=5 ...
```

## 12.4 取り込み時の記録と落とし穴

取り込むときは、`(edition, version)` と来歴 (provenance) をblock-array IR に記録します (`.litematic` → java、`.mcstructure` → bedrock、`.schem` → java)。これによって、取り込んだものも再現できるようになります ([バージョンとエディション](/ja/spec/versioning-editions/))。

**取り込みを「作者の意図の復元」として見せてはいけません。** これが最大の落とし穴です。復元できるのはボクセルと一部の規則性だけで、CLI は `W_SEMANTIC_LOSS` でそのことを伝えます。

ほかの規則:

- 取り込みに由来する `raw_fill` は `origin=imported` / `residual` で区別し、設計用の第一級の DSL としては扱いません。
- Litematica の複数のリージョンとサブリージョンのオフセットは、平坦化せずに来歴として残し、リージョンは `site` または複数の struct に対応付けます。
- エンティティを含む schematic では、block IoU だけで成功と判定してはいけません。エンティティ用の指標を別に持ち、第一級のエンティティ ([エンティティ](/ja/spec/entities/)) だけを取り出します。チェストの中身とコマンドブロックは捨てます。
- 巨大な schematic (48³ を超えるものや村全体) を一度にリフトすると、LLM の文脈に収まりません。ストリーミングでパースしながら、チャンクへの分割 → チャンクごとの L1 → 部分ごとのリフト → `site` での結合、という流れをまとめて制御する必要があります。
- 1.13 のフラット化より前の、数値 ID の `.schematic` は v1 では扱いません ([目的とスコープ](/ja/spec/overview/))。
