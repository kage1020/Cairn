---
title: "14. レッドストーン (論理回路)"
---

Cairn はレッドストーンを **論理レベル** で記述します。信号グラフを宣言すると、コンパイラが実際の
ダスト・リピータ・トーチ・コンパレータを合成し、配置し、配線してボクセルに落とします。

P1 の効果が最も大きいのがここです。信号減衰、クロストーク、ディレイ計算は、AI がボクセル建築以上に
苦手とする物理ですが、いずれも論理記述から決定論的に導出されます。

論理層のファーストクラスの対象は挙動ではなく **信号依存グラフ** です。時間は言語コアに持ちません
([§14.4](#144-時間モデル))。

## 14.1 2 つの層と v1 の境界

- **Tier 0、物理配置。** `repeater facing=north delay=2` のように部品を置き、ブロックステートは
  導出されます。挙動はモデル化しません ([ブロックステート](blockstate))。
- **Tier 1、論理。** 本章です。信号グラフを宣言し、コンパイラが合成 → 配置 → 配線でボクセルにします。

新しいキーワードは `logic` / `circuit` / `assert` の 3 つだけです。論理プリミティブは組み込みの
`def` ライブラリとして提供され、語彙は小さくクローズドなまま保たれます (P3)。

Verilog で言えば、v1 が許すのは `assign` 相当だけで、クロック付き代入は許しません。

| | 範囲 |
|---|---|
| **組み合わせ** | `and` / `or` / `not` / `xor` / `nand` / `nor` / `mux` |
| **厳選した順序マクロ** | `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter` |
| **スコープ外** (→ Tier 0 か `raw`) | `always` / `process` / `state` / `case` / FSM / クロック付き代入 / CPU |

## 14.2 信号バインディング

センサが信号を発し、アクチュエータが消費します。どちらも先行フェーズで配置される物理メンバです
([コンポーネント・編集・複数建築](components-editing-sites))。

```
# センサ → 信号
lever      id=sw   side=front offset=2 y=1 -> sig.power
button     id=bt   side=front               -> sig.ring
daylight   id=dl   at=..                     -> sig.day
observer   id=ob   at=.. facing=down         -> sig.tick
pressure_plate id=pp at=front.outside offset=0 y=0 -> sig.step

# アクチュエータ ← 信号
lamp       id=l1   at=..  lit_by=sig.lamps
piston     id=p1   at=..  powered_by=sig.mem facing=up sticky=true
door       id=d1   ..     opened_by=sig.power
dispenser  id=ds   at=..  fired_by=sig.pulse facing=south
```

この対応は規範です。`-> sig.X` の末尾はセンサのもの、各アクチュエータキーはそれを読む 1 つのコンポー
ネントのものです。それ以外の場所に書かれたバインディングは `E_LOGIC_MISPLACED_BINDING` です。
`walls ... powered_by=sig.x` は何の回路も記述しておらず、受け入れればコンポーネントの無いポートがネ
ットリストに載ります。

**上記のうち現在受け付けるのは `door` と `pressure_plate` だけです。** `lit_by=` / `powered_by=` /
`fired_by=` にはまだホストが無く、どこに書かれても拒否されます。

**信号名。** センサは `sig.` 名前空間へ発し、アクチュエータはそこから読むので、名前空間の外の名前は
誰にも読まれません。`logic` 行の左辺でも、センサの `->` の末尾でも、アクチュエータキーの値でも同じで、
`E_LOGIC_INVALID_SIGNAL` になります。名前は `sig.` とその後ろのセグメント 1 つです。`opened_by=a` は
`a` という信号への配線ではなく、`opened_by=sig.a.b` も何も指しません。

値より先にホストが検査されます。`walls -> a` の誤りは 1 つで、それはホストの誤りです。値をどう書いて
も `walls` はセンサになりません。

**バインディングは `[selector]` の後に書き、中には書きません。**
`door[id=front] opened_by=sig.power` は束縛し、`door[id=front,opened_by=sig.power]` は束縛しません。
角括弧は行が作用するメンバを選ぶもので、その中に書かれたものはバインディングとして読まれません。括弧
内の組は、外に出したあとも残る所見を得ます。それだけが問題なら角括弧を名指す
`E_LOGIC_MISPLACED_BINDING`、そうでなければホストかキーに対する所見です。

4 つのアクチュエータキー以外のキーに `sig.` の値が付いている場合は `E_LOGIC_UNKNOWN_BINDING_KEY` で
す。値は「信号を配線するつもりだった」と言い、キーは「誰も読まない」と言っている状態で、これはタイポ
の形です (`oepend_by=sig.power` など)。

## 14.3 論理層は依存 DAG

信号どうしの依存を書きます。ブール結合とマクロ適用です。時間を含まない純粋なデータフローで、
コンパイラ内部では Logic IR になります。

```
logic sig.lamps = sig.power and not sig.day
logic sig.mem   = latch(set=sig.a, reset=sig.b)   # RS ラッチ (マクロ)
logic sig.pulse = pulse(sig.ring, 4)              # 単安定: 4 段
logic sig.fire  = edge_rising(sig.tick)
logic sig.sel   = mux(sel=sig.s, a=sig.x, b=sig.y)
```

式に時間演算はありません。`pulse(sig.ring, 4)` の `4` は tick 値ではなく **段数** です。

## 14.4 時間モデル

v1 で時間を持つのはマクロ (`delay` / `pulse` / `edge` / `latch` / `counter`) だけです。`delay(3)` は
内部でリピータ 3 個に落ちるセルマクロで、書くべき tick 演算子はありません。

**ディレイは Logic IR にも Netlist IR にも載りません。Placement IR で初めて確定します** ([§14.8](#148-ir-とフェーズへの接続))。
`and` は論理的にはゼロディレイですが、tick 数が分かるのはセル選択 (Java なら `and → ComparatorAND`)
と配置後の実配線長が決まってからです。

数値が tick として現れるのは検証アサーション ([§14.7](#147-検証)) だけです。論理式の中で tick 演算を
することはありません。

## 14.5 Place-and-route

DSL が見せるのは 2D のメンタルモデルです。純粋な 2D のフロアプランは行き詰まるので、内部実装は
擬似 2.5D で、`plane` / `via` / `bridge` の概念を持ちます (DSL には露出しません)。純粋な 2D では
扱えない回路クラス: fanout、バス、交差、コンパレータのフィードバック、オブザーバチェーン。

```
circuit region=basement void=3       # 高さ 3 のサービス層を確保し、ここに回路を配線する
```

内部アルゴリズムは 5 段階です。

1. **配置**: トポロジカル順、左から右へ。セル同士の間に 1 列、入力パッドの列と行の間にも 1 列、
   そして最後のセルの先にも 1 列の空きを置きます (行末がアクチュエータのパッド列と領域の端に
   挟まれないように)。セル本体はブロックなので、ネットはその隣接座標を通って届きます。2 入力
   ゲートには 3 つの異なるネット (2 本のドライバと自分の出力) が接するので、空いた隣接座標が
   3 つ必要です。詰めて並べると行の途中のセルには 2 つしか残らず、領域をいくら広げても増えません。
   行を空けるのはそのためで、ショートの無い配線を *可能にする* ものであって保証するものでは
   ありません。

   行はさらに、領域の手前の端から 1 行内側に立ちます。I/O パッドは `z = 0` から並びます。ダストは
   隣の座標のダストを読むので、空き座標のレーンは何マス長くても 1 本のネットしか運べません。端に
   接したセルにはレーンが 1 本しかなく、2 入力ゲートに接する 3 つのネットはそれを共有できません。
   1 行内側にすれば、どのセルにも両側にレーンが 1 本ずつできます。これはセル 1 つにつき 1 行では
   なくネットリスト全体で 1 行なので、列の間隔と違ってセル数に比例しません。

   間隔は配線の保証ではありません。通りすがりのネットが最後の 1 面を取ることはあり、そのスコープは
   ショートさせずに拒否されます。行が収まらない領域 — `n` 個のセルに `2n + 1` 列、そして 3 行 —
   は、2 段階先で到達不能なシンクとして落ちる前に、ここで拒否されます。
2. **Steiner 配線**: Manhattan で、既に立っているもの、そして既に敷かれたネットのダストを避けて。
   セル本体と I/O パッドは予約されており、その上にダストは引けず、信号がその中を *通過* することも
   できません。コンポーネントは信号を発するか消費するかのどちらかだからです。したがってすべての
   シンクはネットの木の葉であり、fanout はシンクを数珠つなぎにするのではなく、行の脇に幹を通して
   各シンクへ枝を出します。障害物が無ければ配線は直線の矩形経路です。あれば回り込むか、回る場所が
   無ければ `void=<N>` の予算内で `bridge` 層へ登ります。

   1 つの座標に 2 つのネットが乗れば、2 つの信号を運ぶ 1 本のダストになります。同じ平面で 1 歩
   隣り合う 2 本も同じです — ダストは隣のダストとつながるからです。そこでネットは 1 本ずつ敷かれ、
   後のものは先に敷かれたダスト *とその隣の座標* を避けます。これが交差の脱出であり、段階 4 では
   なくここで行うのは登った分を測るためです。`wire_length` もディレイパスのティック数も、この
   経路木から読み取られます。順序は fanout の降順、次にネット自身のキー — 全順序なので、1 つの
   レイアウトに対する答えは何度尋ねても 1 つです。

   「隣」は同一平面に限ります。`y + 1` のダストが真下のダストを読むかどうかは、その間に何が
   立っているかで決まり、このモデルはそれを持ちません。内部モデルは疑似 2.5D で、ボクセルへの
   実現は物理タイル層の担当なので、同じ列を共有する 2 本を隔てるのはルータではなくその層の義務
   です。
3. **ディレイ挿入**: 減衰限界 15 を超えるセグメントにのみバッファとしてリピータを入れます。
   セグメントはドライバからそのシンクまでの **実配線** 経路で測り、バッファはその経路上に立ちます。
   2 点間の直線が常に配線とは限りません。
4. **交差の合法化**: 段階 3 が数えたバッファのリピータに座標を割り当てます。この時点で配線を
   合法化する必要はありません。リピータは自分のネットの実配線経路の上に立ち、その経路はその
   ネットだけのもので、他のどのネットも 1 歩以内には走っていないので、持ち上げるべきショートも、
   奪い合う座標もありません。
5. **エディションの合法化**: [§14.6](#146-エディション差) を参照。

配線は `circuit` 領域に閉じ込められます。収まらなければ fail-loud です。ドライバからの経路がすべて
塞がれているシンク — コンポーネント、先に敷かれたネットのダストとその隣の座標、あるいは予約の端で
— も、理由は違えど同じ拒否で、結べなかった 2 つの座標と、面を取ったネットを告げます。

```text
E_ROUTE_CONGESTION line 21 circuit=basement:
  synthesized netlist needs ~3.2x the reserved area (void=3, region 9x7).
  Fix: increase `void`, enlarge region, or split into multiple `circuit` blocks.
```

## 14.6 エディション差

3 層のセルライブラリが、エディション差をライブラリの中だけに閉じ込めます。

```
Logical Cell → Edition Cell → Physical Tile
  AND        → Java:    ComparatorAND → block array
             → Bedrock: TorchAND      → block array
```

- **吸収する**: リピータ、オブザーバ、コンパレータ、向き。いずれもセル実装の差です。
- **吸収しない**: QC (準接続)、BUD、更新順序。ブロック更新順序の暗黙の意味論に依存し、可搬な実装が
  存在しません。

更新順序の意味論を要する論理は、サイレントな地雷ではなくコンパイルエラーになります。「recompile で
あり transcode ではない」と整合します。

```text
E_NO_PORTABLE_IMPL line 15:
  this circuit requires update-order (quasi-connectivity / BUD) semantics.
  No portable redstone implementation exists for the target edition.
  Fix: redesign the logic to be order-independent, or drop to Tier 0 with an @edition guard.
```

手で置いたレッドストーンはエディションをまたぐと壊れます。論理記述であればコンパイラがエディションに
正しい回路を出せます。部品ではなく論理を書く最大の理由です。

## 14.7 検証

意図を宣言し、合成された回路を tick 単位でヘッドレスにシミュレートして照合します。アサーションは
3 種類です。

```
# 組み合わせ: 真理値表
assert truth(sig.a, sig.b -> sig.out) { 00->0; 01->1; 10->1; 11->0 }

# レイテンシ — place-and-route がディレイを変えるので重要
assert latency(sig.in -> sig.out) <= 4

# 時相 — 完全な LTL ではなく有界の eventually のみ
assert always(sig.button -> eventually sig.door_open within 8)
```

自己修正ループ (P5) は **synth → sim → diff → patch** で、検証はターゲットエディションごとに走ります。
パッチが触るのは place-and-route のヒント、リピータ、バッファだけです。**Logic IR は決して書き換えま
せん。** 論理を自動改変する自己修正は危険だからです。

```text
E_SIM_ASSERTION_FAILED edition=bedrock:
  assert latency(sig.in -> sig.out) <= 4, but measured 6 (extra repeaters from crossing legalization).
  Patch target: placement hint / route. (logic is never auto-modified)
  Suggested: relax to <=6, enlarge circuit void to shorten routes, or pin cell placement.
```

## 14.8 IR とフェーズへの接続

Intent IR と block-array IR の間に、HDL と同じ分け方で 3 つの IR 層が入ります。

```
Intent IR        logic 宣言 / circuit 領域 / 信号バインディング
   ↓ logic_synth
Logic IR         論理式と依存 DAG。エディション中立、ゼロディレイ
   ↓
Netlist IR       セルとネット。Logical Cell の選択。まだディレイ無し
   ↓ logic_place
Placement IR     セル座標 + 実配線長。ここでディレイが確定する
   ↓ logic_route
block-array IR   ダスト・リピータ・トーチ・コンパレータのボクセル実体
```

フェーズモデル ([コンパイルモデル](compilation)) は `fixtures` の直後を
`logic_synth → logic_place → logic_route` に分割します。センサとアクチュエータが 3D に配置される
まで I/O ポートの座標が確定しないからです。

## 14.9 逆方向変換

schematic から取り込んだ手作りのレッドストーン ([エコシステム連携](ecosystem-interop)) は、v1 では
Tier 0 の raw として保持します。ダストの塊から論理を逆合成することはスコープ外です。generation-first
かつ lossy な方針と整合します。
