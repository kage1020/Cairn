---
title: "9. コンポーネント・編集・複数建築"
---

## 9.1 コンポーネント構文 `def`

`def` はスロットを持つコンポーネントを定義します。`theme` や `site` と同じ機構で統一されているので、
編集・テーマ適用・複数建築で参照系が分裂しません。

パラメータ化 (可変サイズなど) は許可し、再帰は禁止します。`def` は `requires version>=X` を宣言でき、
合成物の最小バージョンは構成要素の最大値です ([バージョンとエディション](/ja/spec/versioning-editions/))。

パラメータの仕組みが定まるまで、ヘッダの語彙は閉じています。`def` のヘッダが取るのは、下げが読む
`size=` と、まだどのパスも読まない `class=` で、`struct` のヘッダも同じ 2 つを取ります。どちらでも
それ以外のキーは `E_UNKNOWN_ARGUMENT` として拒否され、ヘッダの `class=` は `W_IGNORED_ARGUMENT`
として報告されます ([Lint §11.3](/ja/spec/lint/#113-エラーと警告の区分))。そのため下のサンプルは
その警告を伴います。

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=door  class=entry side=front at=center
  roof   id=roof  kind=gable mat_slot=roof
```

## 9.2 編集モデル

重要なメンバは `id=` を持ちます。持たないメンバには、生成順ではなく親 / ロール / side / level /
offset から導かれる **意味ベースの安定アドレス** が付きます。struct に追記してもアドレスは変わりませ
ん。

編集はセレクタやアドレスに対するパッチ DSL です。

```
edit window[class=vent][level=floor2] set shape=arch
edit window@front[0]                  set mat_slot=accent_glass
edit door[id=entry]                   set side=front at=center
```

「2 階の窓だけをアーチにする」のような概念レベルの編集が、全体を壊さずにできなければなりません。
編集の差分は `intent_state` だけを見る ([ブロックステート](/ja/spec/blockstate/)) ので、導出結果が変わっても
編集の安定性は損なわれません。

## 9.3 `site` による複数建築

AI に絶対座標の計算をさせてはいけません。配置はトポロジカルな制約であり、座標へ解決するのはコンパイ
ラの仕事です。

```
site village:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  connect home1.door to home2.door path=@gravel
```

各 struct はポート (位置・法線・幅) を公開し、`connect` がそれらを結びます。ストラクチャブロックの
48³ 制限を超える村や城は、複数 struct の合成として表現します。

### 9.3.1 座標規約

`east` は `+x` へ進み、`north` は `-z` へ退きます。これは [§5.4](/ja/spec/syntax#54-セレクタ) の「front
は `+z`」と整合します。 `front` が南を向く建物はファサードが `+z` にあり、`north_of=X` は次の配置を
その背後に置きます。

Y 軸はトポロジカルセレクタの影響を受けず、現状すべての配置は `y = 0` に着きます。

### 9.3.2 原点セレクタ

各 `place` は `at` / `east_of` / `north_of` の **ちょうど 1 つ** を持ちます。下の式の `prior` は `ID` が
指す配置、`new` はこれから置く配置です。原点は配置の低 `x`・低 `z` 側の角で、`dims` はその全体の広がり、
つまり `size=WxH` の footprint に屋根の `overhang=` の列を両側に足したもので、ロックファイルに記録される
広がりと同じです。

| セレクタ | 効果 | 備考 |
|---|---|---|
| `at=origin` | ワールド `(0, 0, 0)` に固定。 | `at=` に許される唯一の値。site の最初の `place` は必ずこれを使います。暗黙の既定はありません。 |
| `east_of=ID gap=N` | 新しい原点 = `(prior.x + prior.dims.x + N, prior.y, prior.z)`。 | `ID` は同じ `site` 内で先に宣言された place を指す必要があります。`gap` は向かい合う 2 つのバウンディングボックスの面 (それぞれ壁とその `overhang=` の列) の間のブロック数 (`0` ならボックスが接する)、既定は `0`。 |
| `north_of=ID gap=N` | 新しい原点 = `(prior.x, prior.y, prior.z − new.dims.z − N)`。 | `ID` と `gap` は `east_of` と同じ規則。原点は低 `z` 側の角なので、後退量は新しい配置自身の奥行きです。これにより 2 つのどちらが深くても、新しい配置の `+z` 面と直前の配置の `−z` 面の間がちょうど `N` ブロックになります。 |

セレクタの併用、および `origin` 以外の値を持つ `at=` は `E_INVALID_PLACE_ORIGIN` です。

`gap=` は相対セレクタ 2 つのものです。`at=origin` の行は絶対位置に固定され距離を読まないので、隣に
書かれた `gap=` は何にも読まれず、`W_IGNORED_ARGUMENT` で報告されます
([Lint §11.3](/ja/spec/lint/#113-エラーと警告の区分))。引数自体は実在し、どちらを意図したのかは書いた人が
決めることだからです。

相対セレクタの行では `gap=` は整数を取ります。それ以外の値 (`gap=wide`、`gap="4"`) は読めない値で、
その行は `gap=0` と同じ位置に置かれ、その値は `W_IGNORED_ARGUMENT` で報告されます。原点は 32 ビット
符号付きの座標として記録されるので、原点がその範囲を越える行は配置されず `W_DEFERRED_MEMBER` で
報告され、その行を基準に置かれる行もすべて同様です。端に丸めずに拒否するのは、丸めた原点はソースが
求める原点ではなく、同じ端に丸められた 2 行は同じ座標に重なるからです。拒否された行も、その `def` の
本体が出す指摘は報告します。`E_INCOMPATIBLE_MATERIAL` や `W_NO_THEME_BOUND` は、行がどこに置かれても
`def` やテーマの欠陥だからです。配置されない行の読めない `gap=` も、配置されないという注記付きで
報告されます。

### 9.3.3 スコープ跨ぎ参照

すべての `place` 行は `id=` / `use=` / `theme=` を宣言します。どれかを欠いた行は placement になれま
せん。`.nbt` の名前が無い、実体化する `def` が無い、`mat_slot=` を解決するテーマが無いからです。
これが `E_INCOMPLETE_PLACE` で、メッセージは欠けているキーをすべて挙げ、その行は落とされます。キーは
あるがラベルでない場合 (`use=3`) は `E_TYPE_MISMATCH_LABEL` です。

[§9.2](#92-編集モデル) のジオメトリメンバと違って `id=` が必須なのは、それが `east_of=` と `connect`
が参照する名前であり、`.nbt` が書き出される名前でもあるからです ([§9.3.4](#934-出力ファイル名))。

| コード | 原因 |
|---|---|
| `E_UNRESOLVED_PLACE_REF` | `use=NAME` がトップレベルの `def` を指していない。最近傍候補の提案付き。 |
| `E_UNRESOLVED_THEME_REF` | `theme=NAME` が同じファイルの `theme` を指していない。最近傍候補の提案付き。 |
| `E_DUPLICATE_PLACE_ID` | 同じ site の 2 つの `place` 行が `id=` を共有している。最初の宣言へのスパンが付きます。 |
| `W_UNUSED_DEF` | どの `place use=NAME` からも参照されない `def`。`use=` 側のタイポで空のビルドが黙って出ないようにする助言です。 |

### 9.3.4 出力ファイル名

コンパイラは `place` ごとに `.nbt` を 1 つ、`id=` の名前で出力ディレクトリの直下に書きます
(`home1.nbt`、`home2.nbt`)。`/` や `\` を含む id はファイルではなくパスを指し、Windows のファイル名に
使えない文字を含む id はそこでは書き出せないので、どちらも `E_INVALID_PLACE_ID` です
([Lint](/ja/spec/lint/))。名前に site は含まれず、同じディレクトリに
すべての `struct` (自身の名前で書かれる) とすべての walkway ([§9.3.5](#935-ポートと-connect)) も書かれます。
大文字小文字を無視して比べて同じファイル名になる 2 つの成果物は `E_OUTPUT_NAME_COLLISION` です。
`struct` と同じ名前の `place` や、2 つの site に置かれた同じ `id=` は、片方をもう片方で上書きする代わりに
拒否されます。各
placement のワールド原点と `(site, def, theme)` の provenance は `build.cairn.lock` の `placements`
に記録されるので、下流の消費者は座標ソルバを再実行せずにレイアウトを再構築できます。

### 9.3.5 ポートと `connect`

`connect FROM.PORT to TO.PORT path=@MATERIAL` は、同じ `site` 内の 2 つの placement の名前付きポート
の間に幅 1 ブロックの walkway を敷きます。

**ポートとは。** `PLACE.PORT` が解決する `(place, member_id)` の組です。ポートは参照先 `def` の
`door` と `window` メンバで公開されます。stair と roof のポートは将来の拡張用に予約されています。

**ポートの位置。** メンバの `side=` の壁の 1 ブロック外、placement の地面の段 (`place_origin.1`) で
す。 `front` / `back` / `left` / `right` は `+z` / `-z` / `-x` / `+x` に対応します ([§9.3.1](#931-座標規約))。
壁ローカルのオフセットは次から取ります。

- `door` は `at=` の値 (`center` / `left` / `right`、[§5.4](/ja/spec/syntax#54-セレクタ))。数値オフ
  セットは予約されています。
- `window` は矩形の幾何中心 `offset + size.w / 2`。`offset=` が無い場合は、切り抜きと同じく `0` と
  読みます。

placement の overhang はポートを外面の外側、overhang のリングまで押し出します。`window` に書かれた
`y=` はポートを地面の段から持ち上げ **ません**。walkway は厚さ 1 ボクセルの平らな帯で、Y は相手側の
端点と一致していなければならないからです。`sym=true` の窓は元の `offset` 側にポートを 1 つだけ提供し
ます。鏡映側の開口は壁に現れますが、`id=` が解決するのは 1 つの座標です。

**ポートは壁が開けられた場所です。** ポートを持つ 2 つのロールはどちらも石積みを貫く開口なので、開口
が必要とする段に壁が届いていない placement はどちらのポートも支えません。`door` は開く段が 1 つの層
の中に、`window` は矩形全体が 1 つの層の中に収まっていなければなりません。

```
段 1 が 1 つの層の中                                 # door (ポートのドアは def 本体のメンバなので、
                                                 #       開く段は常に 1)
offset + size.w ≤ wall_length                    # window、水平
y … y + size.h - 1 の全段が 1 つの層の中             # window、垂直
```

以上はポートが自分で読む規則です。これに加えて、ポートは開口パスが実際に切り抜いた開口にしか立ちま
せん: 切り抜きだけが読む引数 (`repeat=`、`step=`、`sym=`) で保留された `window` や、自身の
`mat_slot=` がブロックに解決しない `window` は壁をそのまま残すので、そのポートも一緒に拒否されます。

`level y=N` の下の `walls height=H` メンバはワールドの段 `N + 1 … N + H` を塗ります。その level の
基底段は、その level の床スラブが置かれるはずの段です — ただし level スコープの `floor` は現時点で
保留されるので、実際に塗られるのは struct 自身のスラブが持つ段 `0` だけです。接する層は 1 つに融合
するので、`walls height=5` と `level y=5 walls
height=4` は段 1 から 9 までの 1 枚の壁になり、継ぎ目をまたぐ窓もその中にあります。間に空気がある層
は融合せず、その隙間に吊られた窓はどの層の中にもありません。`def` 本体の `door` メンバが開くのは段
`1` なので、石積みが `level y=6 walls` しかない placement は、戸口が決して届かない壁です — 同じ本体
の `window` も同じ理由で拒否されます。

ポートが照合される段は placement が実際に **塗った** 段であり、これは開口パスが切り抜きに使ったのと
同じ値です — `def` を読み直した二番目の答えではありません。`mat_slot=` が解決しない `walls` は何も塗
らないので、切り抜きは保留され、ポートもそれに従って拒否されます。`level` の中で宣言された `walls` は
その段を塗るので、切り抜きは行われ、ポートもその上に立ちます。ポートを置けなかった行は
`W_DEFERRED_MEMBER` とともに落ち、拒否された端点ごとに note が 1 つ付いて、その端点が破った規則を
1 つだけ名指しします: ポートを持てないロール、書かれた `side=` や `at=`、欠けているか形の誤った
window の引数、矩形が壁からはみ出した量、石積みが覆っている段と覆っていない段、あるいは開口パスがそれ
を切り抜かなかったことです。note はそのメンバの行を
指し、問題が `side=`、引数、石積みのいずれかにある場合は、その行にメンバ自身の指摘も並びます — 引数
の同じ読み取りから文言を作るので、書かれた内容について両者が食い違うことはありません。`place` によって座標の範囲外へ押し出されたポートは指すべきメンバの行を持たず、
行の上だけでそう述べます。

**経路の走り方。** 2 つのポートが共有する Y での Manhattan の L 字 (x 軸の脚、次に z 軸の脚) です。
3D 経路探索 (階段、多層 walkway) は意図的にスコープ外です。

その L 字が既存の構造の床を横切る場合、コンパイラは地面平面で迂回路を探します。障害物を回る最短経路、
同じ長さなら曲がりの少ない方を選び、同着はタイブレークを決定的にして、同じソースが常に同じ帯を敷くよ
うにします。

直線の L 字に戻して衝突するセルを飛ばすのは、遮られない経路がそもそも存在しない場合だけです。ポート
が別の placement の床の下に埋まっている、対象が完全に囲われている、site がルータの探索面積上限を超え
ている、のいずれかです。このとき `W_WALKWAY_BLOCKED` が 1 件出て、具体的な原因とその修復 (埋まったド
アや窓を動かす、 gap を広げる、構造どうしを近づける) を告げます。`--format json` では
`data: { kind: "walkway_blocked", skipped: N }` が付きます。

**マテリアル。** `path=@TOKEN` はメンバのマテリアルと同じ `mat_slot=` パイプラインを通ります。
`@gravel` のような具体トークンはレジストリパック無しで動きます。`@path.gravel` のような抽象トークン
はパックの materials カタログを必要とし、外れると `W_ABSTRACT_TOKEN_DEFERRED` か
`E_UNKNOWN_ABSTRACT_TOKEN` を出します。

**出力。** walkway を敷いた `connect` 行ごとに `.nbt` を 1 つ、site とポートの名前で書きます
(`hamlet_walkway_home1_entry__home2_entry.nbt`)。ロックファイルには、ワールド原点・寸法・解決済みの
経路マテリアルを持つ `walkways:` エントリを記録します。

ブロックを 1 つも敷かなかった行は、ソースが求めた walkway を失っています。`place` が上流で拒否された
端点 (`W_DEFERRED_MEMBER`)、置けないポート、walkway の名前に載せられない id、解決しない経路マテリア
ル、それ自体がルータの探索面積上限を超える直線の L 字、セルがすべて配置と重なり帯が空気だけになる直
線の L 字のいずれかです。このとき `cairn compile` はスコープを失ったときと同じく `E_PARTIAL_BUILD` で
ビルドを拒否し、`cairn check --edition E --target V` も同じく拒否します ([Lint](/ja/spec/lint/))。
`--target` なしの `cairn check` は lowering しないので、この損失を見ることはありません。失われた
walkway は `site::SITE::FROM ↔ TO` と、行が書いたとおりのポートで名指されます。
`W_DUPLICATE_WALKWAY` の行は何も失いません。その対は先の行が敷いているからです。どちらも敷かなかっ
た同じ対を名指す 2 行は、1 件の損失です。1 つの誤りが複数の損失になることもあります。def が `size=`
を欠く `place` は失われたスコープで、そこに端点を持つ walkway もすべて失われるので、それぞれが
`E_PARTIAL_BUILD` に note を 1 つ加え、lowering されなかったスコープの数を 1 つ増やします。

**診断。**

| コード | 原因 |
|---|---|
| `E_CONNECT_ARITY` | 行の形が `FROM.PORT to TO.PORT` でない。読めない端点はその行の walkway を失わせるので、解決の前に検査します。 |
| `E_UNRESOLVED_PORT` | ドットの右のポート id が、参照先 def の本体のメンバを指していない。最近傍候補の note 付き。その id が `level` の下に入れ子になったメンバのものなら、そう告げます。level スコープのメンバは、まだポートになれません。 |
| `E_AMBIGUOUS_PORT` | def が同じ `id=` を複数のメンバで公開している。衝突をリネームしてください。 |
| `E_MISSING_PATH_MATERIAL` | 行が `path=` を欠いており、walkway の lowering に敷くものが無い。 |
| `E_UNRESOLVED_PLACE_REF` | 先頭の place id が、この site の先行する place を指していない ([§9.3.3](#933-スコープ跨ぎ参照) と共通)。 |
| `W_WALKWAY_BLOCKED` | 遮られない経路が無い。直線の L 字に戻り、残りの帯は敷かれます。直線の L 字のセルがすべて配置と重なるとき、または直線の L 字だけでルータの探索面積上限を超えるときは、何も敷かれず、walkway は失われます。 |
| `W_DUPLICATE_WALKWAY` | 同じ `(from, to)` のポート対がこの site で既に敷かれている。重複行は落とされます。 |
| `W_INVALID_WALKWAY_IDENT` | site / place / port の id を walkway の名前に載せられない ([Lint](/ja/spec/lint/))。行は落とされます。 |
