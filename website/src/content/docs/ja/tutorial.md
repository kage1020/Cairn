---
title: "チュートリアル"
description: 1 棟のコテージから、テーマ付きの塔、レッドストーンの門番小屋、そして村へ。
---

[サンプル](/ja/examples/) を、1 つずつ順に辿ります。上から読んでください。各節はその前の節を前提に
しています。

> リファレンスコンパイラはまだスケルトンなので、以下の `cairn compile` はまだ動きません。仕様どおりの
> 記述なので、CLI の感覚を掴むにはこれを読むのが正しい方法です。

## 1. コテージを建てる

これで完結した Cairn のビルドです
([`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn))。

```
@cairn 2026.06
@requires version>=1.20

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  slot glass -> @glass_pane
  window[class=small] -> frame=@spruce_wood

struct cottage size=9x7
  floor  mat_slot=floor
  walls  class=outer mat_slot=wall height=4
  door   side=front at=center
  window class=small side=front offset=2 y=2 size=2x2 sym=true mat_slot=glass
  roof   kind=gable mat_slot=roof overhang=1
```

どちらのエディションにもコンパイルできます。

```sh
cairn compile examples/cottage.crn --edition java    --target 1.21.4
cairn compile examples/cottage.crn --edition bedrock --target 1.21.40
```

**Minecraft のバージョンはソースに書きません。** `--target` で与え、`--edition` を必ず併記します。
ソースが宣言できるのは `@requires` (ターゲットが満たすべき下限) と `@cairn` (このファイルが書かれた
言語バージョン) だけです。

**struct が「どこ」を、theme が「何で」を言います。** `mat_slot=wall` はブロック名ではなく注入
ポイントで、それを束縛するのが theme です。`theme medieval` を別のものに差し替えてもジオメトリは
変わりません。CSS の発想をブロックに適用したものです。

**位置は意味的です。** `side=front` / `offset=2` / `y=2` / `at=center` は壁に沿った位置です。書く側に
絶対座標は一切出てきません。

**順序は関係ありません。** `window` は `roof` の後に書かれていますが、それでも壁の開口として切られ
ます。コンパイラは評価前にコマンドを固定フェーズに振り分けるので、読みやすい順に書けます。

**ブロックステートは導出されます。** ドアの `facing=south` も、壁の `north=tall` も、ガラスペインの
`connected` も誰も書きません。コンパイラが位置と隣接から求めます。

## 2. 抽象マテリアルとレベル

[`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn) は同じ形に
3 つの考えを足します。

```
theme keep_dark:
  slot floor -> @floor.wood.broadleaf   # 抽象トークン
  slot wall  -> @wall.stone.cobble
  slot trim  -> @wood.dark
  slot roof  -> @roof.dark_wood

struct keep size=11x9
  floor  id=base   mat_slot=floor
  walls  id=shell  mat_slot=wall height=5
  roof   id=roof   kind=gable mat_slot=roof overhang=1

  level id=floor1 y=0
    ...

  level id=floor2 y=5
    walls  id=upper class=outer mat_slot=wall height=4
    window class=arrow_slit side=front repeat=3 step=2 y=2 size=1x2 shape=slit
    stair  id=eave kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left
```

**トークンは 2 種類あります。** `@oak_planks` は *正準ブロックトークン* で、特定の意味を表し、黙って
格下げされることはありません。`@floor.wood.broadleaf` は *抽象マテリアルトークン* で、ターゲット次第で
テーマがオークにもシラカバにも解決してよい美的な選択です。

**`level y=5` は上階に自前の `y=0` を与えます。** 2 階の窓は地面からではなく、その階の床から `y=2` の
位置に留まります。

**ブロックステートを書くと intent に昇格します。** `stair id=eave` の行は `half=top facing=out
shape=outer_left` を明示しているので、これらの値はコンパイラのものではなくあなたのものです。規則は
「既定では導出し、intent になりうるブロックステートはすべて上書き可能」です。

続き: [マテリアルとテーマ](/ja/spec/materials-themes/)、[ブロックステート](/ja/spec/blockstate/)。

## 3. レッドストーンを信号グラフとして書く

ダストやリピータを置く代わりに、何が何に依存するかを宣言します
([`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn))。

```
struct gatehouse size=7x5
  floor mat_slot=wall
  walls class=outer mat_slot=wall height=3
  door  id=front side=front at=center mat_slot=door

  pressure_plate id=plate at=front.outside offset=0 y=0 -> sig.step
  pressure_plate id=inner at=inside.front  offset=0 y=0 -> sig.exit

  logic sig.open = sig.step or sig.exit
  door[id=front] opened_by=sig.open

  circuit region=floor void=2

  assert truth(sig.step, sig.exit -> sig.open) { 00->0; 01->1; 10->1; 11->1 }
  assert always(sig.step -> eventually sig.open within 2)
```

**センサが発し、アクチュエータが受けます。** `-> sig.step` はセンサの出力、`opened_by=sig.open` は
アクチュエータの入力です。その間の依存を書くのが `logic` です。

**tick 演算はありません。** 論理式は時間をまったく持ちません。アサーションの `within 2` だけが tick を
意味します。ディレイは回路を配置し配線するまで分からないからです。

**`circuit region=…` は place-and-route のための領域を確保します。** 収まらなければ
`E_ROUTE_CONGESTION` が修正案とともに出ます。黙って溢れることはありません。

**アサーションは 3 種類です。** 組み合わせ論理の `truth(…)`、有界ディレイの
`latency(in → out) <= N`、有界時相の `always(in -> eventually out within N)`。完全な LTL は意図的に
持ちません。tick シミュレータが安く判定できるものだけです。

**エディション差はセルライブラリにあり、言語にはありません。** 同じ論理が Java では `ComparatorAND`
セル、Bedrock では `TorchAND` になります。準接続やブロック更新順序に依存する回路は、サイレントな
地雷ではなくコンパイルエラーです。

続き: [レッドストーン](/ja/spec/redstone/)。

## 4. 複数の建物を置く

コテージが 1 棟建てば、あとは再利用です
([`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn))。

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=entry class=entry side=front at=center
  window id=front side=front y=2 offset=2 size=2x2 mat_slot=glass
  roof   id=roof  kind=gable mat_slot=roof overhang=1

site hamlet:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  place id=home3 use=cottage theme=medieval north_of=home1 gap=5

  connect home1.entry to home2.entry path=@gravel
  connect home1.entry to home3.entry path=@gravel
```

**`def` は再利用可能なコンポーネント** で、`theme` や `site` と同じスロット機構の上に立っています。
編集・テーマ適用・複数建築のどこでも参照の仕方が同じです。

**配置は関係で書きます。** `east_of=home1 gap=4` は制約であり、座標に落とすのはコンパイラの仕事です。
LLM の算術誤りという最悪のクラスがまるごと消えます。

**struct はポートを公開します。** `home1.entry` は `def` で宣言されたドアメンバで、`connect` が 2 つの
ポートを walkway で結びます。

**48³ のストラクチャブロック制限は消えます。** 1 つのストラクチャブロックに収まらない村は、`site` の
上に複数の `def` を合成したものにすぎません。

続き: [コンポーネント・編集・複数建築](/ja/spec/components-editing-sites/)。

## 次に読むもの

| したいこと | 読む場所 |
|---|---|
| 全部を書き直さずに一部を変える | [編集モデル §9.2](/ja/spec/components-editing-sites/) — `edit window[class=vent] set shape=arch` |
| どの Minecraft バージョンで動くか知る | [バージョンとエディション §10.5](/ja/spec/versioning-editions/) — `cairn info` の報告 |
| 既存の schematic を Cairn に取り込む | [エコシステム連携](/ja/spec/ecosystem-interop/) — 写し取り、リフト、voxel-diff |
| 他の屋根の種類を試す | `roof-shed` / `roof-hip` / `roof-flat` の[サンプル](/ja/examples/) |
| 用語を引く | [用語集](/ja/spec/glossary/) |
