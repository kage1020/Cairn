---
title: "12. エコシステム連携と逆方向変換"
---

## 12.1 順方向
`block-array IR → serialize` で `.nbt` / `.litematic` / `.schem` / `.mcstructure` を出力します
([アーキテクチャ](architecture))。各フォーマットは単なるシリアライザで、既存フォーマット
は block-array IR の周りに置かれる追加バックエンドです。

## 12.2 逆方向: コンパイラは写し取りのみ、リフトは LLM に
コンパイラは voxel → 「これは壁/屋根」のコンピュータビジョンを作りません (保守不能になります)。
**コンパイラは堅牢な忠実写し取り + 検証 + voxel-diff を実装し、意味リフトは raw 中心 DSL に対する
LLM リファクタリングとして行う** (言語のドッグフーディング)。これは P5 の自己修正ループと評価フレーム
ワーク ([評価フレームワーク](evaluation)) と整合します。

```text
cairn import house.litematic --mode raw    → house.raw.crn     # fill/raw_block 中心 (忠実写し取り)
(LLM が house.raw.crn を意味 DSL にリファクタ) → house.lifted.crn
cairn compile house.lifted.crn --edition java --target 1.21.4
cairn diff-blocks house.litematic house.lifted.crn           # voxel XOR → 自己修正へ
```

compile→diff→patch の自己修正ループ:

```text
E_DECOMPILE_DIFF: block IoU = 0.962 < threshold 0.985
  missing bbox=(12,4,3)..(18,6,3) mat=glass_pane → likely window repeat too small
  Suggested patch: edit window[id=front_windows] set repeat=4
```

収束閾値: block IoU ≥ 0.985 / state_accuracy ≥ 0.995 / residual raw ≤ 5%。完全一致は要求しません。
残差は明示的に `raw_fill id=residual_* origin=imported` として保持します。

## 12.3 忠実写し取りの三段階
「命名」が写し取りとリフトの境界です。

- **L0 raw cells**: 1 行 1 ボクセル。大きすぎて LLM 文脈を圧迫するので中間表現のみ。
- **L1 spatial-compressed (コンパイラの上限)**: fill 集約、AABB パレット圧縮、
  **resolved_state → intent_state 逆変換** (`stair facing=east half=top`)、対称性/周期性を `raw_repeat`
  への構造圧縮。**ただし命名はしない**。
- **L2 semantic-lifted (LLM の上限)**: fill→`wall`、repeat→`def/use`、具体ブロック→`mat_slot`+`theme`。

```
# L1 (命名なし、決定論的)
raw_repeat id=r03 count=5 step=3,0,0: raw_fill mat=@glass_pane from=0,2,0 to=1,3,0
# L2 (LLM が命名し意味を与える)
window id=front_windows side=front mat_slot=glass repeat=5 ...
```

## 12.4 取り込み時のスタンプと落とし穴
- 取り込み時に `(edition, version)` と provenance を block-array IR にスタンプします (`.litematic`→java、
  `.mcstructure`→bedrock、`.schem`→java)。これは再現性/バージョン認識に繋がります
  ([バージョンとエディション](versioning-editions))。
- **取り込みを「作者の意図を復元する」として提示してはいけません** (最大の落とし穴)。回復できるのは
  ボクセルと一部の規則性だけです。CLI/UI で明示します: `W_SEMANTIC_LOSS`。
- 取り込み起源の `raw_fill` は `origin=imported` / `residual` で隔離し、ファーストクラスの設計 DSL
  として扱いません。
- Litematica の複数 region / サブリージョン offset はフラット化せず provenance として保持し、region を
  `site` / 複数 struct にマッピングします。
- エンティティを含む schematic では、block IoU だけで成功と判定してはいけません。エンティティ指標を
  別に持ち、ファーストクラスのエンティティ ([エンティティ](entities)) のみを取り出します
  (チェスト中身/コマンドブロックは捨てる)。
- 巨大 schematic (48³ 超 / 村全体) を一度にリフトすると LLM 文脈が爆発します。**チャンク分割 → チャンク
  ごとの L1 → パートごとのリフト → `site` での結合** をオーケストレーションする必要があります (ストリ
  ーミングパース)。
- レガシーな数値 ID `.schematic` (1.13 以前のフラット化前) は v1 では未対応です ([目的とスコープ](overview))。
