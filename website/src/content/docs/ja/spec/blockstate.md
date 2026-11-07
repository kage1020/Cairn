---
title: "6. ブロックステート"
---

## 6.1 既定では導出し、上書きで昇格する

**変更の余地あり。** 下に挙げる上書きのうち、組み上げられるのは `stair` の `facing`、`half`、`shape` だけです。そのほかの上書き、`beam`・`chest`・`note_block` のキーワード、座標の組はまだ組み上げられません ([互換性ティア](/ja/spec/compatibility/#変更の余地あり))。

既定では、コンパイラが位置と隣接するブロックからブロックステートを導出します。階段の向き、ドアの向き、ガラス板・フェンス・壁のつながり、チェストの左右などです。

現時点で導出するのは、このうち階段の向きと形だけです。`roof` は、置かれた位置から階段の `facing`、`half`、`shape` を得ます。軒の `stair` が位置から得るのは、置かれた面に応じた東西南北の `facing` だけで、`half` と `shape` は行に書いた値か、既定の `top` と `straight` です。`door` は開口を切り抜くだけで、ドアのブロックは置きません。ガラス板・フェンス・壁のつながり、チェストの組み合わせ、下で述べる松明と看板の入れ替えもまだ導出しないので、`cairn lower examples/cottage.crn` はつながりの状態を持たない `minecraft:glass_pane` を出力します。

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

下の例には、パーサがまだ受け付けない構文が含まれています。受け付けるのは `stair` の行だけです。`beam`、`chest`、`note_block` は `E_UNKNOWN_KEYWORD` に、`at=2,1,2` のような座標の組は `E_PARSE` になります。

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
  intent_state:   { half: top, facing: out, shape: outer_left }  # 作者の意図
  resolved_state:                                                # 導出した値。塗りに由来するステートもここ
    - state: { id: minecraft:oak_stairs, properties: { facing: south, half: top, shape: outer_left } }
      runs: [{ from: [0, 4, 0], len: 1 }]
```

Minecraft 自身の用語である「blockstate」と紛れないよう、あえて別の名前にしています。導出や塗りに由来する resolved のステートを、作者が書いた intent のステートと混ぜないことで、編集が安定します。

メンバーの `intent_state` には、そのキーワードが名前を挙げるブロックステートのキーだけが入ります。`stair` なら `half`、`facing`、`shape`、`waterlogged` です。ほかのキーワードが挙げるキーは、まだありません。行に書いたそれ以外の `key=value` は、メンバーの引数になります。例外は `id`、`class`、`mat_slot` で、これらはそれぞれ専用のフィールドになります。キーがどちらに入るかはキーワードで決まります。`stair` の `shape=` は意図ですが、`window` の `shape=` は開口の輪郭を表す引数です ([エンティティ §8.2](/ja/spec/entities/#82-アンカーの規約))。

lowering は、組み上げる block-array の隣に、各メンバーの `resolved_state` を記録します。本体を塗り終えたときにメンバーのセルが持つすべてのステートと、それを持つセルの組です。セルは `x` 方向の連なりにまとめ、それぞれの最初のセルと長さで、block-array の順に並べます。セルは最後に塗ったメンバーのものになるので、ドアが切り抜いたセルは切り抜かれた壁ではなくドアに記録され、セルが 1 つも残らないメンバーは何も記録しません。`cairn lower --format json` は、この記録を `members` の下に、`structures` と同じキーで出力します。これで 2 つのビルドをメンバーごとに比べられます。本体はメンバーを、それぞれが持つ最初のセルの順に並べます。そのため、セルを共有しない行を並べ替えても、格子と同じく記録も変わりません ([コンパイルモデル §4.8](/ja/spec/compilation/#48-同一フェーズ内の衝突とパレット))。記録は本体を lowering するたびに作ります。ブロック ID は、テーマのエディション別の変種や固定したバージョンの上書きも含めて、その lowering のターゲットが解決したものです。プロパティの名前は正規の語彙のものです。

メンバーは `type` の代わりに、キーワードから分類した `role` を持ちます。

`bed` は IR の型を整理しておくために、エンティティではなくブロックのメンバーとして扱います。

## 6.3 `waterlogged`

**変更の余地あり。** この節の内容は、まだどれも組み上げられません ([互換性ティア](/ja/spec/compatibility/#変更の余地あり))。`waterlogged` を設定するパスも読むパスもありません。`stair` に書いた `waterlogged=false` はまだ届いていないキーです。コンパイラは `W_IGNORED_ARGUMENT` で報告し ([Lint §11.3](/ja/spec/lint/#113-エラーと警告の区分))、このキーを書かなかったときと同じ階段を作ります。下の例では、`fill`、`trapdoor`、`water` は `E_UNKNOWN_KEYWORD` に、`from=1,1,1` のような座標の組は `E_PARSE` になります。

既定では、塗りから決まります。`fill fluid=water` が水浸しにできるブロックと重なると、コンパイラが `waterlogged` を立てます。

`waterlogged=auto|true|false` の 3 値で指定することもできます。水槽の中に空気の空間を残す (明示的な `false`)、水源と流水を区別する、水浸しにできるブロックの一覧がバージョンによって違うことに対応する、といった用途のためです。流水は `flow=` と `level=` で明示します。

[§6.1](#61-既定では導出し上書きで昇格する) の上書きと同じく、`waterlogged=` は、ブロックを 1 つ置き、そのブロックのステートを行に書くメンバーに書きます。コンパイラが知っているキーワードのうち、そうしたメンバーは `stair` だけです。`roof` と `window` は置くブロックのステートをすべて導出し、上書きを取らないので、どちらに書いた `waterlogged=` も `E_UNKNOWN_ARGUMENT` になります。

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # 重なったフェンス・階段・看板は自動で waterlogged になる
trapdoor id=shutter at=.. waterlogged=false          # 水槽の中の空気の窓
water id=stream from=.. flow=east level=4
```
