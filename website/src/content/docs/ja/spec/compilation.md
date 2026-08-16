---
title: "4. コンパイルモデル"
---

## 4.1 フェーズ評価
ソースは行指向、フラット、順不同で書いてかまいません。コンパイラは各コマンドを暗黙のフェーズに
振り分け、**固定順** で評価します:

```
massing (shell: floor/walls/volume)
  → envelope (roof/exterior)
  → openings (door/window)
  → fixtures (装飾物: sign/painting/frame/bed/sensors & actuators)
  → logic_synth (レッドストーン合成: Logic IR → Netlist IR)
  → logic_place (セル配置)
  → logic_route (配線 → Placement IR、ディレイ決定)
  → raw (エスケープハッチ)
```

- ソース内で `roof` の後に書かれた `window` も、壁の開口として適用されます (順序事故が消えます)。
- **last-wins は同一フェーズ内のローカル上書きにのみ適用されます**。`raw` (fill など) は危険ゾーン
  であり、常に最後に適用されます。
- レッドストーン論理 ([レッドストーン](redstone)) は `fixtures` の直後を 3 フェーズに分割します。
  センサ/アクチュエータが 3D に配置されて初めて I/O ポートの座標が確定し、配置と配線が可能になり
  ます。

```
struct keep size=11x9
floor  id=base   mat_slot=floor
walls  id=shell  mat_slot=wall height=5
roof   id=roof   kind=gable mat_slot=roof overhang=1
window id=front_windows side=front y=2 offset=2 size=2x2 mat_slot=glass   # roof の後でも開口を切る
door   id=entry  side=front at=center
```

## 4.2 ターゲット軸
ターゲットは **二軸 `(edition, version)`** です。バージョンとエディションは **DSL ソースには書きません**。
バージョン/エディションを知るのはバックエンドのみです ([バージョンとエディション](versioning-editions))。

```sh
cairn compile build.crn --edition java    --target 1.21.4
cairn compile build.crn --edition bedrock --target 1.21.40
```

- `--target` 単独は **禁止** です。`--edition` は **必須** です。
- 同じ「1.21」も Java と Bedrock では異なる意味を持ち、Java の DataVersion は Bedrock の block_version
  とは無関係です。

## 4.7 level グループ化と体積の導出

`level y=N` はメンバをグループ化し、その各メンバを struct の基準面から `N`
ボクセル上に置きます。これ自体はメンバではなくグループ化構文で、`level` の行
はブロックを一切生みません。配下の各メンバは、それぞれが元から持つ垂直座標に
`N` を足した上で、本体に直接書かれたのと同じように立体化されます。

struct が立体化される体積は書くものではなく、導出されるものです。

```
Dims.x = size.W + 2 × overhang
Dims.z = size.H + 2 × overhang
Dims.y = 1 + wall_top + roof_extra
```

`overhang` は全 roof のうち最大の `overhang=`、`wall_top` は全 walls のうち最大
の `N + height` (`N` は囲っている level の値、本体直下なら `0`)、`roof_extra` は
§4.3–§4.6 の kind 別の寄与のうち最大のものです。`level` の中のメンバもこの 3 つ
すべてに数えられます — walls が `level y=5` の下にしかない struct は、それを本体
に直接書いた struct と同じ高さになります。

オフセットが 0 でないときの立体化規則を、すべてのロールが持つわけではありません。
`walls` / `door` / `window` / `stair` / `pressure_plate` は `N` を自身のジオメトリ
の基準として読みます。`floor` と `roof` は struct が 1 枚だけ持つ面で、落とすべき
2 枚目の床も、置くべき 2 枚目の屋根もありません。したがって `N > 0` の `level y=N`
の下では `W_DEFERRED_MEMBER` を出し、ブロックを生みません。

何も生まないメンバは導出される体積にも何も寄与しません。落とされた roof の
`overhang=` は footprint を広げませんし、その高さも `Dims.y` を上げません。逆も
成り立ちます — 立体化されるメンバは必ず、その体積が収めるつもりで大きさを決めた
メンバです。この 2 つは 1 つのリストを両側から読んだものであり、渡された配列の外
にメンバが描き込むことを防いでいるのはこの一致です。
