---
title: "8. エンティティ"
---

## 8.1 2 つの段階

**変更の余地あり。** この節の内容は、まだどれも組み上げられません ([互換性ティア](/ja/spec/compatibility/#変更の余地あり))。

すべてのエンティティを `nbt={}` で自由に書けるようにすると、テーマ、編集、lint、バージョンの違いへの強さが失われます。そこで、重要なエンティティは **構造化** し、名前付きのメンバーと同じ編集モデルで扱えるようにします。

構造化するかどうかの基準は、「編集したい属性、またはバージョンの違いを吸収する属性を持つエンティティは構造化する」です。`nbt={}` で書くのは、一度きりの特殊な NBT だけです。

- **構造化する (第一級)**: `sign`、`painting`、`item_frame`、`armor_stand`、`villager` (と `trade`)、`display` (text / block / item)、`bed` (ブロックとして扱う)。
- **汎用の受け皿**: そのほかの mob には `spawn id=.. type=<entity> at=<selector> [nbt={...}]` を使います。

ただし、これらはまだどれもキーワードではなく、`spawn` を含めてすべて `E_UNKNOWN_KEYWORD` になるので、エンティティを置くビルドはまだありません。ビルドが書き出すブロックエンティティは、回路のコンパレーターが出力の強度を保持するものだけです ([レッドストーン §14.5](/ja/spec/redstone/#145-配置配線-place-and-route))。コンパレーターを敷くのは Java の回路だけです。`chest`、`dispenser`、`daylight` が置くブロックのように ([ブロックステート §6.1](/ja/spec/blockstate/#61-既定では導出し上書きで昇格する)、[レッドストーン §14.2](/ja/spec/redstone/#142-信号バインディング))、ブロックエンティティを持つほかのブロックは、データを持たずに置かれます。下の例には、パーサがまだ受け付けない構文も含まれています。小数 (`scale=1.5`) と `nbt={...}` の値は `E_PARSE` になります ([構文 §5.1.1](/ja/spec/syntax/#511-リテラルと区切り))。

```
villager id=trader at=stall[0] profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=front.above block=@lantern scale=0.5
item_display  id=trophy at=counter item=diamond_sword rotation=y90
spawn id=cat type=cat at=inside.floor nbt={variant:"black"}
```

村人の交易所は定番の建築で、display エンティティは現代的な装飾に欠かせません。これらを `nbt={}` で書かせると、生成の品質と編集の安定性が落ちるので、構造化します。

ブロックエンティティ (看板) と本物のエンティティ (絵画) は、NBT の上では別物ですが、DSL では同じセレクタの文法を使います。両者を区別するのはコンパイラの仕事です。

## 8.2 アンカーの規約

**変更の余地あり。** `painting` と、下に挙げる window の `shape=` は、まだ組み上げられません ([互換性ティア](/ja/spec/compatibility/#変更の余地あり))。

絵画、額縁、アーチ窓、階段室、張り出した屋根は、宣言したサイズと実際に占める AABB が一致しません。ここを曖昧にしたままだと、編集の安定性、テーマの差し替え、実装間の互換性がすべて損なわれます。

この曖昧さをなくすため、すべてのプリミティブは IR の中で 4 つの情報を持ちます。`anchor` (基準点)、宣言上の bbox、実際の bbox、取り付ける面です。lowering は、セルを 1 つでも組み上げるすべてのメンバーについてこの 4 つを導き、`resolved_state` の隣に記録します ([ブロックステートモデル §6.2](/ja/spec/blockstate/#62-intent_state-と-resolved_state))。

- **アンカー**は、メンバーの行がそのメンバーを置く基準のセルです。
- **宣言上の bbox** は、ほかのメンバーが塗る前に、行が指すすべてのセルを囲む箱です。
- **実際の bbox** は、本体を塗り終えた時点でメンバーが持っているすべてのセルを囲む箱です。ほかのメンバーが端のセルを取れば宣言上の bbox より小さくなり、屋根の張り出しのように行が指す範囲の外まで組み上げれば大きくなります。
- **取り付ける面**は、メンバーを据える壁や壁の上端の面で、その面が向く方向で呼びます。`north` は `-z`、`south` は `+z`、壁の上端の面は `up` です。どの面にも据えないメンバーは持ちません。

どれも本体のブロック配列のセル、または両端のセルを含むセルの箱で、配列の低い隅から数えます。張り出しはメンバーと一緒にこれらも動かし、基準面の下に敷く `circuit` は構造物と一緒にこれらも持ち上げます ([レッドストーン §14.5](/ja/spec/redstone/#145-配置配線-place-and-route))。

| メンバー | アンカー | 宣言上の bbox | 取り付ける面 |
|---|---|---|---|
| `floor` | 箱の低い隅 | 0 段目の `size=` の敷地 | なし |
| `walls` | 箱の低い隅 | 壁が埋めるすべての段にわたる敷地 | なし |
| `roof` | 箱の低い隅 | 壁の上の段から頂点までの `footprint=` | `up` |
| `door` | 下半分 | 上下の両半分 | 壁の外側の面 |
| `window` | `offset=` と `y=` が置くセル | 行が置くすべての長方形 | 壁の外側の面 |
| 軒の `stair` | 壁の左端にある帯のセル | 帯 | 壁の外側の面 |
| 壁際の `chest`、センサー、アクチュエーター、Tier 0 部品 | 1 つのセル | その 1 つのセル | 立っている壁の面 |
| 座標の三つ組で置く Tier 0 部品、`raw_block` | 1 つのセル | その 1 つのセル | なし |
| `fill`、`raw_fill` | 箱の低い隅 | `from=` と `to=` が張る箱 | なし |
| `circuit` | 箱の低い隅 | 確保するすべての層 | なし |

window の長方形は、`repeat=` の連なりが打つ 1 つずつと、`sym=true` のときの鏡像です。そのため、window と重なるので省かれた鏡像は、宣言上の bbox には残り、実際の bbox には入りません ([構文 §5.4](/ja/spec/syntax/#54-セレクタ))。壁際の器具は、壁の外側から (`at=front.outside`) なら外側の面に、内側から (`at=inside.front`) なら内側の面に据えます。ブロックを 1 つも敷かない `circuit` は、確保した層を空気で書くので、その層を持ちます。

**window の `anchor=`** は、`offset=` と `y=` が置く window のセルを、壁の外から見て名指しします。`bottom_left`、`bottom_center`、`bottom_right`、`middle_left`、`center`、`middle_right`、`top_left`、`top_center`、`top_right` のどれかです。書かなければ `bottom_left`、つまり window の最初の列と最も低い段を置きます ([構文 §5.4](/ja/spec/syntax/#54-セレクタ))。

**roof の `footprint=` と `bounds=`** は、屋根が覆うセルと、張り出しが本体に何をするかを名指しします。どちらも取る値は 1 つです。`footprint=struct` は構造物の `size=` の敷地で、`bounds=expand` は本体のブロック配列を敷地の四方へ `overhang=` だけ広げます ([コンパイルモデル §4.7](/ja/spec/compilation/#47-level-によるまとまりと体積の導出))。どちらを書いても、書かないときと同じものを組み上げます。ほかの値は `E_INVALID_MEMBER` で、屋根は何も描きません。それでも、書かれたとおりの構造物は形のとおりに立ち上がります ([Lint](/ja/spec/lint/#lowering))。

**AABB の重なり**は、1 つの本体の中では優先度による統合で解決し、本体どうしではエラーです。本体のメンバーは 1 つのブロック配列を塗るので、2 つのメンバーが指すセルは後のフェーズが取り、同じフェーズの中では後に書いた行が取ります。後者は `W_PHASE_CONFLICT` で報告します ([コンパイルモデル §4.8](/ja/spec/compilation/#48-同一フェーズ内の衝突とパレット))。セルを失ったメンバーも宣言上の bbox はそのままで、実際の bbox はまだ持っているセルを囲みます。2 つの `place` 行は配列を共有しません。本体はそれぞれ独立した構造物で、埋めないすべてのセルに空気を書くので、片方を重ねて置くと元にあったものが消え、統合するものがありません。座標ソルバーが site のすべての行を置いたあと、箱がセルを共有する 2 つの行は、後の行の `E_PLACEMENT_OVERLAP` です ([Lint](/ja/spec/lint/#site-と配置))。通路もそれ自体が 1 つの構造物で、結ぶ 2 つ以外の配置の箱の中に帯がセルを敷けば、同じエラーです。両端の 2 つの配置は除きます。帯は各ポートの壁の 1 ブロック外から始まり、`overhang=` があればそこはその配置の箱の中だからです。

隣接するブロックに依存するブロックステート (階段、フェンス) は、干渉を検出せずに上書きすると壊れます。たとえば、内角になるはずの階段が外角のまま宙に浮いてしまいます。境界のブロックステートを解決し直すのは、IR の層の責任です。

下の例では、`painting` は `E_UNKNOWN_KEYWORD` になり、window の `shape=arch` は受け付けますがどのパスも読まないので `W_IGNORED_ARGUMENT` になります ([Lint](/ja/spec/lint/))。window の `anchor=bottom_center` は最初の列を 3 に置き、roof の `footprint=` と `bounds=` は、書かないときに組み上げるものを名指しします。

```
painting id=hall_art side=inside.front anchor=center y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 どこで線を引くか

看板、絵画、額縁、ベッドは建築の雰囲気を左右するので、構造化して扱います。チェストの中身、村人の持ち物など、建築の精度に関わらない情報は構造化しません。汎用の `spawn` の `nbt={}` か、エスケープハッチで書きます。
