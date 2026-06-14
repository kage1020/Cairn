---
title: "6. ブロックステートモデル"
---

## 6.1 デフォルトは導出 + 上書きによる昇格 (soft boundary)
- デフォルトではコンパイラが位置と隣接からブロックステートを **導出** します (階段の向き、ドアの向き、
  ガラスペイン/フェンス/壁の接続、チェストの左右など)。
- **建築的意図になり得るブロックステートはすべて上書き可能であり、書かれた瞬間に「意図」へ昇格し
  ます**。
- 「導出できるなら AI に書かせない」という強い読みは **採用しません**。正しいルールは
  「デフォルトは導出。意図になり得るブロックステートはすべて上書き可能」です。

意図になり得る (= 上書き可能な) 代表ケース:
- `stairs facing` (椅子/装飾としては facing 自体が意図)、`stairs half=top` (逆さま = 庇)、`stairs shape`
- `chest size=single` (隣接による自動マージは禁止)、`bed facing`、`door hinge/open`
- `log/pillar axis` (水平梁)、`trapdoor open/half`、`snow layers`、`candle count`、
  `glazed_terracotta` 回転
- `redstone_dust connect`、`repeater delay`、`observer/piston/dispenser facing`、`note/instrument`

導出側に属する見落としがちなケース: 取り付け面による `torch`↔`wall_torch`、`sign`↔`wall_sign` の自動
置換。

```
stair id=eave   kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left  # 庇
beam  id=lintel kind=pillar mat_slot=frame at=front.top axis=x                              # 水平梁
chest id=store  at=inside.back size=single
note_block at=2,1,2 instrument=bit note=12
```

## 6.2 IR 表現: intent_state / resolved_state を分離
```yaml
member:
  id: eave
  type: block | block_entity | entity      # 区別はコンパイラの仕事だが IR では型付け
  primitive: stairs
  intent_state:   { half: top, shape: outer_left }       # 作者の意図。編集 diff はここだけを見る
  resolved_state: { facing: north, waterlogged: false }  # 導出結果。paint 由来もここに入る
```
- Minecraft 用語の `blockstate` との衝突を避ける命名: `intent_state` / `resolved_state`。
- `bed` は **block メンバ** として扱います (IR の型を綺麗に保つため、エンティティにはしません)。
- 編集の安定性のため、resolved (導出/paint 起源) と intent (作者) を混在させません。

## 6.3 waterlogged
- デフォルトは **paint 由来**: `fill fluid=water` が waterloggable なブロックと重なると、コンパイラが
  waterlogged を立てます。
- 三値 `waterlogged=auto|true|false` を許可します: 水槽内に空気のポケットを残す (明示 false)、ソース
  と flowing の区別、waterloggable テーブルのバージョン差のため。
- 流れる水は `flow=` / `level=` で明示します。

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # 重なるフェンス/階段/看板は自動 waterlogged
trapdoor id=shutter at=.. waterlogged=false          # 水槽内の空気窓
water id=stream from=.. flow=east level=4
```
