---
title: "9. コンポーネント・編集・複数建築"
---

## 9.1 `def` — コンポーネント

`def` はスロットを持つコンポーネントを定義します。`theme` や `site` と同じ機構で統一されているので、
編集・テーマ適用・複数建築で参照系が分裂しません。

パラメータ化 (可変サイズなど) は許可し、再帰は禁止します。`def` は `requires version>=X` を宣言でき、
合成物の最小バージョンは構成要素の最大値です ([バージョンとエディション](versioning-editions))。

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=door  class=entry side=front at=center
  roof   id=roof  kind=gable mat_slot=roof
```

## 9.2 編集モデル

重要なメンバは `id=` を持ちます。持たないメンバには、生成順ではなく親 / ロール / side / level /
offset から導かれる **意味ベースの安定アドレス** が付きます。struct に追記してもアドレスは変わりません。

編集はセレクタやアドレスに対するパッチ DSL です。

```
edit window[class=vent][level=floor2] set shape=arch
edit window@front[0]                  set mat_slot=accent_glass
edit door[id=entry]                   set side=front at=center
```

「2 階の窓だけをアーチにする」のような概念レベルの編集が、全体を壊さずにできなければなりません。
編集の差分は `intent_state` だけを見る ([ブロックステート](blockstate)) ので、導出結果が変わっても
編集の安定性は損なわれません。

## 9.3 複数建築 — `site`

AI に絶対座標の計算をさせてはいけません。配置はトポロジカルな制約であり、座標へ解決するのはコンパイラの
仕事です。

```
site village:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  connect home1.door to home2.door path=@gravel
```

各 struct はポート (位置・法線・幅) を公開し、`connect` がそれらを結びます。ストラクチャブロックの
48³ 制限を超える村や城は、複数 struct の合成として表現します。

### 9.3.1 座標規約

`east` は `+x` へ進み、`north` は `-z` へ退きます。これは §5.4 の「front は `+z`」と整合します。
`front` が南を向く建物はファサードが `+z` にあり、`north_of=X` は次の配置をその背後に置きます。

Y 軸はトポロジカルセレクタの影響を受けず、現状すべての配置は `y = 0` に着きます。

### 9.3.2 原点セレクタ

各 `place` は `at` / `east_of` / `north_of` の **ちょうど 1 つ** を持ちます。

| セレクタ | 効果 | 備考 |
|---|---|---|
| `at=origin` | ワールド `(0, 0, 0)` に固定。 | `at=` に許される唯一の値。site の最初の `place` は必ずこれを使います。暗黙の既定はありません。 |
| `east_of=ID gap=N` | 新しい原点 = 直前の `(x + dims.x + N, y, z)`。 | `ID` は同じ `site` 内で先に宣言された place を指す必要があります。`gap` はブロック単位の外面間距離 (`0` なら壁が接する)、既定は `0`。 |
| `north_of=ID gap=N` | 新しい原点 = 直前の `(x, y, z − dims.z − N)`。 | `ID` と `gap` は `east_of` と同じ規則。 |

セレクタの併用、および `origin` 以外の値を持つ `at=` は `E_INVALID_PLACE_ORIGIN` です。

### 9.3.3 スコープ跨ぎ参照

すべての `place` 行は `id=` / `use=` / `theme=` を宣言します。どれかを欠いた行は placement になれま
せん。`.nbt` の名前が無い、実体化する `def` が無い、`mat_slot=` を解決するテーマが無いからです。
これが `E_INCOMPLETE_PLACE` で、メッセージは欠けているキーをすべて挙げ、その行は落とされます。キーは
あるがラベルでない場合 (`use=3`) は `E_TYPE_MISMATCH_LABEL` です。

§9.2 のジオメトリメンバと違って `id=` が必須なのは、それが `east_of=` と `connect` が参照する名前で
あり、`.nbt` が書き出される名前でもあるからです (§9.3.4)。

| コード | 原因 |
|---|---|
| `E_UNRESOLVED_PLACE_REF` | `use=NAME` がトップレベルの `def` を指していない。最近傍候補の提案付き。 |
| `E_UNRESOLVED_THEME_REF` | `theme=NAME` が同じファイルの `theme` を指していない。最近傍候補の提案付き。 |
| `E_DUPLICATE_PLACE_ID` | 同じ site の 2 つの `place` 行が `id=` を共有している。最初の宣言へのスパンが付きます。 |
| `W_UNUSED_DEF` | どの `place use=NAME` からも参照されない `def`。`use=` 側のタイポで空のビルドが黙って出ないようにする助言です。 |

### 9.3.4 出力ファイル名

コンパイラは `place` ごとに `.nbt` を 1 つ、`id=` の名前で書きます (`home1.nbt`、`home2.nbt`)。各
placement のワールド原点と `(site, def, theme)` の provenance は `build.cairn.lock` の `placements`
に記録されるので、下流の消費者は座標ソルバを再実行せずにレイアウトを再構築できます。

### 9.3.5 ポートと `connect`

`connect FROM.PORT to TO.PORT path=@MATERIAL` は、同じ `site` 内の 2 つの placement の名前付きポートの
間に幅 1 ブロックの walkway を敷きます。

**ポートとは。** `PLACE.PORT` が解決する `(place, member_id)` の組です。ポートは参照先 `def` の `door`
と `window` メンバで公開されます。stair と roof のポートは将来の拡張用に予約されています。

**ポートの位置。** メンバの `side=` の壁の 1 ブロック外、placement の地面の段 (`place_origin.1`) です。
`front` / `back` / `left` / `right` は `+z` / `-z` / `-x` / `+x` に対応します (§9.3.1)。壁ローカルの
オフセットは次から取ります。

- `door` は `at=` の値 — `center` / `left` / `right` (§5.4)。数値オフセットは予約されています。
- `window` は矩形の幾何中心 `offset + size.w / 2`。

placement の overhang はポートを外面の外側、overhang のリングまで押し出します。`window` に書かれた
`y=` はポートを地面の段から持ち上げ **ません**。walkway は厚さ 1 ボクセルの平らな帯で、Y は相手側の端点
と一致していなければならないからです。`sym=true` の窓は元の `offset` 側にポートを 1 つだけ提供します。
鏡映側の開口は壁に現れますが、`id=` が解決するのは 1 つの座標です。

**窓はポートを持つために壁に収まっていなければなりません。** 水平にも垂直にもです。

```
offset + size.w ≤ wall_length            # 水平
y ≥ 1  かつ  y + size.h ≤ H + 1          # 垂直 (walls height=H のとき)
```

`walls height=H` はワールドの段 `1 … H` を埋め、段 `0` は床スラブのものです。壁の外に段がはみ出す窓は
walkway を支えられず、その行は `W_DEFERRED_MEMBER` とともに落ちます。note には door / window /
予約ロールの契約が順に並びます。

現時点でポートと開口が食い違う場面が 1 つあります。ポートは `walls` メンバが *宣言する* 段を読み、
開口パスは実際に *塗る* 段を読みます。`mat_slot=` が解決しない `walls` では、切り抜きは保留されますが
ポートは張り付いたままです。

**経路の走り方。** 2 つのポートが共有する Y での Manhattan の L 字 (x 軸の脚、次に z 軸の脚) です。
3D 経路探索 (階段、多層 walkway) は意図的にスコープ外です。

その L 字が既存の構造の床を横切る場合、コンパイラは地面平面で迂回路を探します。障害物を回る最短経路、
同じ長さなら曲がりの少ない方を選び、同着はタイブレークを決定的にして、同じソースが常に同じ帯を敷くよう
にします。

遮られない経路がそもそも存在しない場合 — ポートが別の placement の床の下に埋まっている、対象が完全に
囲われている、site がルータの探索面積上限を超えている — に限り、直線の L 字に戻して衝突するセルを飛ばし
ます。このとき `W_WALKWAY_BLOCKED` が 1 件出て、具体的な原因とその修復 (埋まったドアや窓を動かす、
gap を広げる、構造どうしを近づける) を告げます。`--format json` では
`data: { kind: "walkway_blocked", skipped: N }` が付きます。

**マテリアル。** `path=@TOKEN` はメンバのマテリアルと同じ `mat_slot=` パイプラインを通ります。
`@gravel` のような具体トークンはレジストリパック無しで動きます。`@path.gravel` のような抽象トークンは
パックの materials カタログを必要とし、外れると `W_ABSTRACT_TOKEN_DEFERRED` か
`E_UNKNOWN_ABSTRACT_TOKEN` を出します。

**出力。** `connect` 行ごとに `.nbt` を 1 つ、site とポートの名前で書きます
(`hamlet_walkway_home1_entry__home2_entry.nbt`)。ロックファイルには、ワールド原点・寸法・解決済みの
経路マテリアルを持つ `walkways:` エントリを記録します。

**診断。**

| コード | 原因 |
|---|---|
| `E_CONNECT_ARITY` | 行の形が `FROM.PORT to TO.PORT` でない。読めない端点はその行の walkway を失わせるので、解決の前に検査します。 |
| `E_UNRESOLVED_PORT` | ドットの右のポート id が、参照先 def のメンバを指していない。最近傍候補の note 付き。 |
| `E_AMBIGUOUS_PORT` | def が同じ `id=` を複数のメンバで公開している。衝突をリネームしてください。 |
| `E_MISSING_PATH_MATERIAL` | 行が `path=` を欠いており、walkway の lowering に敷くものが無い。 |
| `E_UNRESOLVED_PLACE_REF` | 先頭の place id が、この site の先行する place を指していない (§9.3.3 と共通)。 |
| `W_WALKWAY_BLOCKED` | 遮られない経路が無い。直線の L 字に戻り、残りの帯は敷かれます。 |
| `W_DUPLICATE_WALKWAY` | 同じ `(from, to)` のポート対がこの site で既に敷かれている。重複行は落とされます。 |
