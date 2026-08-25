---
title: "6. ブロックステート"
---

## 6.1 既定では導出し、上書きで昇格する

既定ではコンパイラが位置と隣接からブロックステートを導出します。階段の向き、ドアの向き、ガラスペイン
やフェンスや壁の接続、チェストの左右などです。

建築的な意図になりうるブロックステートはすべて上書きでき、**書いた瞬間に intent に昇格します**。
「導出できるものは作者に書かせない」という強い読み方は採りません。規則は「既定では導出し、intent に
なりうるブロックステートはすべて上書き可能」です。

上書き可能でなければならないもの (それぞれが意図になりうるため):

| 分類 | ステート |
|---|---|
| 階段 | `facing` (椅子や装飾としての階段)、`half=top` (逆さ = 軒)、`shape` |
| 家具 | `chest size=single` (隣接からの自動マージは禁止)、`bed facing`、`door hinge` / `open` |
| 向き | `log` / `pillar axis` (水平の梁)、`trapdoor open` / `half`、`glazed_terracotta` の回転 |
| 個数 | `snow layers`、`candle count` |
| レッドストーン | `redstone_dust connect`、`repeater delay`、`observer` / `piston` / `dispenser facing`、`note` / `instrument` |

見落としやすい 2 つは intent ではなく導出に属します。`torch` ↔ `wall_torch` と `sign` ↔ `wall_sign` は
取り付け面によって自動的に置き換わります。

```
stair id=eave   kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left  # 軒
beam  id=lintel kind=pillar mat_slot=frame at=front.top axis=x                             # 水平の梁
chest id=store  at=inside.back size=single
note_block at=2,1,2 instrument=bit note=12
```

## 6.2 `intent_state` と `resolved_state`

```yaml
member:
  id: eave
  type: block | block_entity | entity      # IR では型付けする。区別はコンパイラの仕事
  primitive: stairs
  intent_state:   { half: top, shape: outer_left }       # 作者の意図。編集の差分はここだけを見る
  resolved_state: { facing: north, waterlogged: false }  # 導出結果。ペイント由来のステートもここ
```

Minecraft の用語である blockstate と衝突しないよう、意図的に別の名前にしています。導出やペイント由来の
resolved を、作者が書いた intent と混ぜないことが、編集の安定性を作ります。

`bed` は IR の型をきれいに保つため、エンティティではなくブロックメンバとして扱います。

## 6.3 `waterlogged`

既定はペイント由来です。`fill fluid=water` が waterlog 可能なブロックに重なると、コンパイラが
`waterlogged` を立てます。

3 値の `waterlogged=auto|true|false` も使えます。水槽の中に空気の穴を残す (明示的な `false`)、水源と
流水を区別する、waterlog 可能テーブルのバージョン差に対応する、といった用途のためです。流水は `flow=`
と `level=` で明示します。

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # 重なったフェンス/階段/看板は自動 waterlogged
trapdoor id=shutter at=.. waterlogged=false          # 水槽の中の空気の窓
water id=stream from=.. flow=east level=4
```
