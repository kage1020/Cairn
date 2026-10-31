---
title: "チュートリアル"
description: 1 棟のコテージから始めて、テーマ付きの塔、レッドストーン仕掛けの門番小屋、村へと進みます。
---

[サンプル](/ja/examples/)を 1 つずつ順にたどり、考え方を 1 つずつ紹介します。各節は前の節の内容を前提にしているので、上から順に読んでください。

> このページの `cairn compile` は、現在のコンパイラで実行できます。ただし、レッドストーンはまだコンパイル結果に含まれないので、3 節の門番小屋は回路なしで作られます。

## 1. コテージを建てる

[`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn) は、これだけで完結した Cairn のビルドです。

```
@cairn 2026.06
@requires version>=1.20

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  slot glass -> @glass_pane

struct cottage size=9x7
  floor  mat_slot=floor
  walls  class=outer mat_slot=wall height=4
  door   side=front at=center
  window class=small side=front offset=2 y=2 size=2x2 sym=true mat_slot=glass
  roof   kind=gable mat_slot=roof overhang=1
```

どちらのエディション向けにもコンパイルできます。

```sh
cairn compile examples/cottage.crn --edition java    --target 1.21.4
cairn compile examples/cottage.crn --edition bedrock --target 1.21.40
```

**Minecraft のバージョンはソースでは決めません。** バージョンは `--target` で指定し、`--edition` も必ず一緒に指定します。ソースに書けるのは、ターゲットが満たすべき下限を示す `@requires`、作者が想定しているバージョンをヒントとして示す `@intended_targets`、このファイルが対象とする言語バージョンを示す `@cairn` です。

**struct は「どこに」を、theme は「何で」を書きます。** `mat_slot=wall` はブロック名ではなく、素材を差し込む場所を表します。そこにブロックを割り当てるのが theme です。`theme medieval` を別のテーマに差し替えても、形は変わりません。CSS の考え方をブロックに当てはめたものです。

**位置は意味で指定します。** `side=front`、`offset=2`、`y=2`、`at=center` は、どれも壁を基準にした位置です。絶対座標を書く場面はどこにもありません。

**書く順番は問いません。** `window` の行を `walls` より前に移しても、壁の開口としてきちんと切り抜かれます。コンパイラは評価の前にコマンドを固定のフェーズに振り分けるので、読みやすい順に書いて構いません。

**ブロックステートは導出されます。** 屋根の階段の `facing`、`half`、`shape` は、どれも書いていません。コンパイラが、それぞれの階段が屋根のどこにあるかから求めます。ドアの向きやガラス板のつながりも同じように導出する予定ですが、今の `door` は開口を切り抜くだけです。

## 2. 抽象マテリアルと階

[`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn) は、同じような形に 3 つの考え方を加えます。

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
    window class=arrow_slit side=front repeat=3 step=2 y=2 size=1x2
    stair  id=eave kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left
```

**トークンには 2 種類あります。** `@oak_planks` は *正規ブロックトークン* です。特定のブロックを意味し、黙って別のブロックにダウングレードされることはありません。`@floor.wood.broadleaf` は *抽象マテリアルトークン* です。見た目の好みを表すもので、ターゲットに応じてテーマがオークにもシラカバにも解決できます。

**`level y=5` は、上の階に独自の `y=0` を与えます。** 2 階の窓の `y=2` は、地面からではなく、その階の床から測った高さになります。

**ブロックステートを書くと、意図 (intent) に昇格します。** `stair id=eave` の行は `half=top facing=out shape=outer_left` を明示しています。これらの値はコンパイラではなく、あなたが決めたものになります。ルールは「既定では導出し、意図になりうるブロックステートはすべて上書きできる」です。

続きは[マテリアルとテーマ](/ja/spec/materials-themes/)と[ブロックステート](/ja/spec/blockstate/)で。

## 3. レッドストーンを信号のグラフとして書く

ダストやリピーターを置く代わりに、何が何に依存するかを宣言します ([`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn))。

```
struct gatehouse size=7x5
  floor mat_slot=wall
  walls class=outer mat_slot=wall height=3
  roof  kind=flat mat_slot=wall overhang=1
  door  id=front side=front at=center

  pressure_plate id=plate at=front.outside offset=3 y=0 -> sig.step
  pressure_plate id=inner at=inside.front  offset=3 y=1 -> sig.exit

  logic sig.open = sig.step or sig.exit
  door[id=front] opened_by=sig.open

  circuit region=floor void=2

  assert truth(sig.step, sig.exit -> sig.open) { 00->0; 01->1; 10->1; 11->1 }
  assert always(sig.step -> eventually sig.open within 2)
```

**壁の外のプレートは、屋根の軒の出の下に立ちます。** `at=front.outside` は、`offset=3` で壁の前にあるセル、つまりドアのすぐ外側を取ります。このセルは、屋根の `overhang=1` が足す輪の中にあります。軒の出がなければ struct は壁で終わるので、プレートの置き場がなく、`E_UNPLACEABLE_PLATE` で拒否されます。`at=inside.front` はすぐ内側のセル、`y=1` の床の上を取ります。

**センサーが信号を出し、アクチュエーターが受け取ります。** `-> sig.step` はセンサーの出力、`opened_by=sig.open` はアクチュエーターの入力です。その間の依存関係を書くのが `logic` です。

**tick の計算は書きません。** 論理式は時間をいっさい持ちません。数値が tick を表すのは、アサーションの `within 2` だけです。遅延は、回路を配置して配線するまで分からないからです。

**`circuit region=…` は配置配線のための領域を確保します。** 配線が収まらなければ、修正案付きの `E_ROUTE_CONGESTION` が出ます。黙って領域からはみ出すことはありません。今のところ配線を行うのは実験的な `cairn synth --experimental-logic-synth` だけなので、`cairn compile` はこのエラーを出しません。

**アサーションは 3 種類です。** 組み合わせ論理を検証する `truth(…)`、遅延の上限を検証する `latency(in -> out) <= N`、時間の範囲を区切った時相論理を検証する `always(in -> eventually out within N)` です。完全な LTL は意図的に持たず、tick 単位のシミュレータで安く判定できるものだけに絞っています。ただし、このシミュレータはまだなく、`latency` もまだパーサが受け付けません。

**エディションの違いは、言語ではなくセルライブラリが吸収します。** 同じ論理が、Java では `ComparatorAND` セルに、Bedrock では `TorchAND` セルになります。疑似接続 (quasi-connectivity) やブロック更新の順序に依存する回路は、気付かないうちに壊れる仕掛けを残すのではなく、コンパイルエラーにする方針です。ただし、回路がそれに依存しているかを検査するものは、まだありません。

続きは[レッドストーン](/ja/spec/redstone/)で。

## 4. 複数の建物を並べる

コテージが 1 棟できたら、あとは使い回すだけです ([`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn))。

```
def cottage size=9x7:
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

**`def` は再利用できるコンポーネントです。** `theme` や `site` と同じスロットの仕組みの上に作られているので、編集、テーマの適用、複数の建物の配置のどこでも、同じ方法で参照できます。

**配置は位置関係で書きます。** `east_of=home1 gap=4` は制約で、それを座標に変換するのはコンパイラの仕事です。これで、LLM がもっとも間違えやすい座標計算のミスがまるごとなくなります。

**struct はポートを公開します。** `home1.entry` は `def` で宣言したドアのメンバーで、`connect` は 2 つのポートを通路で結びます。

**48³ というストラクチャーブロックの制限はなくなります。** 1 つのストラクチャーブロックに収まらない村も、`site` の上で複数の `def` を組み合わせれば作れます。

続きは[コンポーネント・編集・複数建築](/ja/spec/components-editing-sites/)で。

## 次に読むもの

| したいこと | 読む場所 |
|---|---|
| 全体を書き直さずに一部だけ変える (予定) | [編集モデル §9.2](/ja/spec/components-editing-sites#92-編集モデル)。例: `edit window[class=vent] set shape=arch` |
| ファイルがどの Minecraft バージョンで動くかを知る | [バージョンとエディション §10.5](/ja/spec/versioning-editions#105-どのバージョン向けかの答えは-3-つある)。`cairn info` が報告する内容 |
| 既存の schematic を Cairn に取り込む (予定) | [エコシステム連携](/ja/spec/ecosystem-interop/)。写し取り、引き上げ、voxel-diff |
| 他の種類の屋根を試す | [サンプル](/ja/examples/)の `roof-shed` / `roof-hip` / `roof-flat` |
| 用語を調べる | [用語集](/ja/spec/glossary/) |
