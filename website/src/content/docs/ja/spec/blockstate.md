---
title: "6. ブロックステート"
---

## 6.1 既定では導出し、上書きで昇格する

既定では、コンパイラが位置と隣接するブロックからブロックステートを導出します。階段の向き、ドアの向き、ガラス板・フェンス・壁のつながり、チェストの左右などです。

建築上の意図になりうるブロックステートは、どれも上書きできます。そして **書いた時点で、そのステートは意図 (intent) に昇格します**。「導出できるものは作者に書かせない」という厳しい考え方は採りません。ルールは「既定では導出し、意図になりうるブロックステートはすべて上書きできる」です。

次のものは、どれも意図になりうるので、上書きできなければなりません (MUST)。

| 分類 | ステート |
|---|---|
| 階段 | `facing` (椅子や飾りとしての階段)、`half=top` (逆さにした軒)、`shape` |
| 家具 | `chest size=single` (隣接による自動の結合は禁止)、`bed facing`、`door hinge` / `open` |
| 向き | `log` / `pillar axis` (水平の梁)、`trapdoor open` / `half`、`glazed_terracotta` の回転 |
| 個数 | `snow layers`、`candle count` |
| レッドストーン | `redstone_dust connect`、`repeater delay`、`observer` / `piston` / `dispenser facing`、`note` / `instrument` |

見落としやすいものが 2 つあります。これらは意図ではなく導出に属します。`torch` と `wall_torch`、`sign` と `wall_sign` は、取り付ける面に応じて自動的に入れ替わります。

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
  type: block | block_entity | entity      # IR では型を持つ。区別するのはコンパイラの仕事
  primitive: stairs
  intent_state:   { half: top, shape: outer_left }       # 作者の意図。編集の差分はここだけを見る
  resolved_state: { facing: north, waterlogged: false }  # 導出した値。塗りに由来するステートもここ
```

Minecraft 自身の用語である「blockstate」と紛れないよう、あえて別の名前にしています。導出や塗りに由来する resolved のステートを、作者が書いた intent のステートと混ぜないことで、編集が安定します。

`bed` は IR の型を整理しておくために、エンティティではなくブロックのメンバーとして扱います。

## 6.3 `waterlogged`

既定では、塗りから決まります。`fill fluid=water` が水浸しにできるブロックと重なると、コンパイラが `waterlogged` を立てます。

`waterlogged=auto|true|false` の 3 値で指定することもできます。水槽の中に空気の空間を残す (明示的な `false`)、水源と流水を区別する、水浸しにできるブロックの一覧がバージョンによって違うことに対応する、といった用途のためです。流水は `flow=` と `level=` で明示します。

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # 重なったフェンス・階段・看板は自動で waterlogged になる
trapdoor id=shutter at=.. waterlogged=false          # 水槽の中の空気の窓
water id=stream from=.. flow=east level=4
```
