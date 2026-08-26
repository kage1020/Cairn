---
title: "4. コンパイルモデル"
---

## 4.1 フェーズ評価

コマンドはフラットに、順不同で書けます。コンパイラが各コマンドをフェーズに振り分け、次の固定順で
評価します。

```
massing     floor / walls / volume
envelope    roof / stair
openings    door / window
fixtures    sign / painting / frame / bed / センサ / アクチュエータ
logic_synth レッドストーン合成: Logic IR → Netlist IR
logic_place セル配置
logic_route 配線 → Placement IR、ディレイ確定
raw         エスケープハッチ
```

`roof` の後に書いた `window` も、壁の開口として切られます。メンバの意味をソース順が決めることは
ありません。

`circuit` は配線領域を確保するだけでボクセルを書かないので、どのフェーズにも属しません。3 つの
`logic_*` フェーズが `fixtures` の後に来るのは、センサとアクチュエータが 3D に配置されるまでポート
座標が確定しないからです ([レッドストーン](redstone))。

last-wins は **同一フェーズ内のローカル上書き** にのみ効き、`raw` は常に最後です。同一フェーズ内で別々
のメンバが 1 つのボクセルを奪い合う場合も同じ規則で解決され、報告されます ([§4.8](#48-同一フェーズ内の衝突とパレット))。

```
struct keep size=11x9
floor  id=base   mat_slot=floor
walls  id=shell  mat_slot=wall height=5
roof   id=roof   kind=gable mat_slot=roof overhang=1
window id=front_windows side=front y=2 offset=2 size=2x2 mat_slot=glass   # それでも開口として切られる
door   id=entry  side=front at=center
```

## 4.2 ターゲット軸

ターゲットは `(edition, version)` の 2 軸です。どちらもソースには書きません。知っているのはバック
エンドだけです ([バージョンとエディション](versioning-editions))。

```sh
cairn compile build.crn --edition java    --target 1.21.4
cairn compile build.crn --edition bedrock --target 1.21.40
```

`--edition` は必須で、`--target` 単独は拒否されます。同じ「1.21」も Java と Bedrock では別物で、
Java の DataVersion は Bedrock の block_version とは無関係です。

## 4.3 切妻屋根のボクセル規則

`roof kind=gable [overhang=N] mat_slot=...` は、棟で出会う 2 枚の対向スロープに落ちます。

屋根の 4 種類 (`gable` / `shed` / `hip` / `flat`) は以下の overhang と壁天端の規約を共有します。
レイアウトはこの節と、続く 3 つの節にあります。

**マテリアル。** 傾斜屋根は `mat_slot=` から材質を取り、それは階段ファミリ (パスが `_stairs` で
終わる id) でなければなりません。ジオメトリが `facing` / `half` / `shape` を塗る対象に付けるので、
フルブロックはそれらを保持できません。ファミリ外の束縛は `E_INCOMPATIBLE_MATERIAL` でビルドが
止まります。`mat_slot=` が無ければ `minecraft:spruce_stairs` にフォールバックします。レジストリ
パックの 4 種 (`roof.dark_wood` / `roof.light_wood` / `roof.warm_wood` / `roof.cool_wood`) はすべて
ファミリ内で解決します。

ファミリ内でありながら独自のブロックステートを持つ束縛は、id を保ったままステートをジオメトリに
譲り、`W_DEFERRED_MEMBER` を出します。軒の `stair kind=stairs` も同じ材質規則に従いますが、
ステートは自分の引数から取ります。

**棟の軸。** 棟は footprint の長い方の水平軸に沿います。正方形 (`size=WxW`) は `x` に倒れ、東西棟に
なります。

**棟の高さ。** 切妻は壁天端から `ceil(short_span / 2)` ボクセル立ち上がります。`short_span` は
overhang 膨張後の `min(dims.x, dims.z)` です。

**レイヤ。** レイヤ `0` は壁天端に座り、スロープ行のペアです (`short_span` が 1 のときは両者が
収束して 1 行)。上のレイヤは左右それぞれ 1 ずつ内側に寄ります。最上段が頂部です。

- 奇数スパン: 中央行に `half=top` の階段 1 つ。
- 偶数スパン: 出会う 2 行に `half=top` の階段 2 つ。棟に V 字の開きを残さないためです。

`short_span` が 1 か 2 なら立ち上がりはちょうど 1 レイヤ、つまりレイヤ `0` だけで、頂部の段は
ありません。

**overhang。** `overhang=N` はボクセルグリッドを水平 2 軸それぞれ `N` 膨張させます
(`Dims.x = size.w + 2N`、`Dims.z = size.h + 2N`)。floor / walls / door / window は書かれた座標を
保ったまま `+N` 内側へ寄ります。屋根は膨張後の箱いっぱいに張るので、軒と妻壁が壁のリングから
はみ出します。

**階段の向き。** スロープ行は `half=bottom, shape=straight` で、`facing` は棟を向きます。x 軸棟なら
`-z` 側スロープが `south`、`+z` 側が `north`。z 軸棟では `east` / `west` に鏡映します。

偶数スパンの頂部 2 つは、それぞれ棟から *外* を向きます (x 軸棟なら `-z` 行が `north`、`+z` 行が
`south`)。内向きにすると両方の外面に沿って屋根の全長にわたり 0.5 × 0.5 のえぐれが残ります。外向き
ならその空隙は棟の下へ移ります。

奇数スパンの頂部 1 つは `half=top` で、低いスロープと同じ向きです。1 セルには外面が 2 つあり階段は
片方しか埋められないので空隙は避けられず、この規則は選択を固定するだけです。

## 4.4 片流れ屋根のボクセル規則

`roof kind=shed slope_to=front|back|left|right [overhang=N] mat_slot=...` は、`slope_to=` が指す壁に
向かって上がる 1 枚のスロープに落ちます。各行の形は切妻の低い側スロープと同じ
(`half=bottom, shape=straight`) ですが、スロープは片側だけなので反対側の壁は書かれた高さのままです。

- **スロープ軸。** `slope_to=front|back` は `z` 方向、`slope_to=left|right` は `x` 方向に上がります。
  高い辺が指定された壁、低い辺が反対側の壁に載ります。
- **高さ。** 片流れは壁天端から `slope_span` ボクセル立ち上がります (`front|back` なら `dims.z`、
  `left|right` なら `dims.x`、いずれも overhang 膨張後)。`y` が上がるごとに低い辺から 1 ずつ内側へ
  寄ります。
- **階段の向き。** すべてのスロープ階段が高い辺を向きます。`front` → `facing=south`、`back` →
  `north`、`left` → `west`、`right` → `east`。最上段は `half=top` の 1 行で同じ向きに蓋をします。
- **`slope_to=` は必須。** 既定値はありません。欠落や未知の値は方向を推測せず
  `W_DEFERRED_MEMBER` になります。

## 4.5 寄棟屋根のボクセル規則

`roof kind=hip [overhang=N] mat_slot=...` は四方向の階段ピラミッドに落ちます。4 面すべてが中央の棟に
向かって内側に傾きます。

- **棟の軸と高さ。** `gable` と同じです (長い軸、正方形は `x`、壁天端から `ceil(short_span / 2)`)。
- **レイヤ構成。** レイヤ `L ∈ 0..extra_height` は内側にオフセットした矩形枠
  `[L, dims.x − 1 − L] × [L, dims.z − 1 − L]` です。レイヤ `0` は壁天端に座り、それが最終レイヤで
  あってもこの枠になります。

  | 辺 | ステート |
  |---|---|
  | 北行 (`z = L`) | `facing=south, shape=straight` |
  | 南行 (`z = dims.z − 1 − L`) | `facing=north, shape=straight` |
  | 西列 (`x = L`) | `facing=east, shape=straight` |
  | 東列 (`x = dims.x − 1 − L`) | `facing=west, shape=straight` |
  | 北西 / 北東の角 | `facing=south` で `outer_left` / `outer_right` |
  | 南西 / 南東の角 | `facing=north` で `outer_right` / `outer_left` |

- **頂部。** 頂部は下の枠が立ち上げたものに蓋をするので、`extra_height > 1` のときだけ現れます。
  正方形 footprint では `half=top` の階段 1 つ (奇数短スパン) か `2x2` の塊 (偶数短スパン)。長方形
  footprint では長い軸に沿って内側の領域を渡る `half=top` の 1 行になります。頂部の向きは切妻の規則
  どおり、x 棟なら `south`、z 棟なら `east` です。
- **overhang。** `gable` と同じです。

## 4.6 陸屋根のボクセル規則

`roof kind=flat [overhang=N] mat_slot=...` は `y = wall_top + 1` にフルブロック 1 層を敷きます。
デッキは膨張後の bounding box 全体を覆います。

- **マテリアル。** デッキの各セルは `mat_slot=` の id をブロックステート無しで置き、束縛が無ければ
  `minecraft:spruce_planks` にフォールバックします。デッキはフルブロックなので、傾斜屋根と違って
  どの id でも有効です。階段を指定すれば既定ステートの階段が並ぶだけです。
- **高さ。** 陸屋根は footprint によらず `Dims.y` に `1` を足すので、`size=WxH` に
  `walls height=K` なら `Dims.y = 1 + K + 1` です。
- **スロープ引数は無し。** `slope_to=`、kind 固有の向き、棟の軸はいずれも適用されません。

## 4.7 level グループ化と体積の導出

`level y=N` はメンバをグループ化し、struct の基準面から `N` ボクセル上に置きます。`level` の行自体は
ブロックを生みません。配下の各メンバは、自身の垂直座標に `N` を足したうえで、本体に直接書かれたのと
同じように立体化されます。

struct が立体化される体積は書くものではなく、導出されるものです。

```
Dims.x = size.W + 2 × overhang
Dims.z = size.H + 2 × overhang
Dims.y = 1 + wall_top + roof_extra
```

各項が数えるのは、実際に塗るメンバだけです。

- `overhang` — 実際に描かれる roof (コンパイラが知っている `kind=` を持ち、`shed` なら `slope_to=`
  もあるもの) のうち最大の `overhang=`。
- `wall_top` — `mat_slot=` が解決する walls のうち最大の `N + height`。`N` は囲っている level の値で、
  本体直下なら `0`。
- `roof_extra` — kind ごとの寄与 ([§4.3](#43-切妻屋根のボクセル規則)–[§4.6](#46-陸屋根のボクセル規則))
  のうち最大のもの。
- `1` — どの struct にもある基準面。

`level` の中のメンバもこの 3 つすべてに数えられます。walls が `level y=5` の下にしかない struct は、
それを本体に直接書いた struct と同じ高さです。

**オフセットが 0 でないときの立体化規則を、すべてのロールが持つわけではありません。** `walls` /
`door` / `window` / `stair` / `pressure_plate` は `N` を自身のジオメトリの基準として読みます。
`floor` と `roof` は struct が 1 枚だけ持つ面なので、`N > 0` の `level y=N` の下では
`W_DEFERRED_MEMBER` を出し、ブロックを生みません。

**塗らないメンバは体積も決めません。** level 配下の roof の `overhang=` は footprint を広げず、その
高さも `Dims.y` を上げません。これはメンバが脱落するあらゆる場面で成り立ちます。`kind=` の無い
`roof` や `slope_to=` の無い `shed` は footprint を広げず、材質が解決できない `walls` は `Dims.y` を
上げません。

材質の側は `walls` に効いて `roof` には効きません。`mat_slot=` が解決しない roof は自前の材質に
フォールバックして描かれるからです。テーマの無い struct がこの非対称の分かりやすい例で、壁は空気に
落ちて何も確保しませんが、その上の `roof kind=gable` は描かれ、棟はやはり壁より上に座ります。

「材質が解決するか」は固定された target に対して問われるので、一部のバージョンしか宣言していない
ブロックは `--target` の違いで `Dims.y` を変えます。固定した target が宣言しない id は
`E_UNKNOWN_ID` なので、その形から成果物は出ません。

## 4.8 同一フェーズ内の衝突とパレット

フェーズをまたぐならフェーズ順が決めます。`walls` を貫く `door` は massing に続く openings であり、
穴が開くことこそが目的です。同一フェーズ内で 2 つのメンバを分けるのはソース順だけで、
[§4.1](#41-フェーズ評価) はそれを「同一フェーズ内のローカル上書き」に対して認めています。

この許可は、作者が同じメンバを書き直す場合のためのものです。たまたま footprint が交差しただけのもの
は別物なので、コンパイラは最後の書き込みを残し、両方のメンバ名と入れ替わったボクセル数を示す
`W_PHASE_CONFLICT` を出します。ビルド結果は変わりません。動かせる 1 行が結果を決めていることが作者に
伝わるだけです。

衝突ではない場合が 2 つあります。

- 値が変わらないセル — 同じ材質の `walls` 2 枚が共有する段で重なる場合。
- メンバが自分自身の上に書く場合 — `repeat=` / `step=` のスタンプが重なる `window` など。

**パレット** は、フェーズが評価する本体 (`struct`、`def`、それらを実体化する各 `place`) が含むブロッ
クを、各フェーズが最初に塗った順で並べたものです。スロット `0` は空気です。途中で intern された履歴
ではありません。最後の 1 ボクセルを後続フェーズに覆われた材質は削除され、残りのスロットは詰め直され
ます。そうしなければ敗者が `.nbt` に紛れ込み、`cairn info` に数えられ、`resolved_ir_hash` に含まれる
ので、「どちらのメンバが負けたか」だけが異なる 2 つのソースが同じビルドに対して別々の成果物を生むこ
とになります。

walkway の配列はフェーズではなく `connect` パスが敷くもので、この節の対象外です。
