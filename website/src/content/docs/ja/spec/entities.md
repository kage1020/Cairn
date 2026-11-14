---
title: "8. エンティティ"
---

## 8.1 2 つの段階

すべてのエンティティを `nbt={}` で自由に書けるようにすると、テーマ、編集、lint、バージョンの違いへの強さが失われます。そこで、重要なエンティティは **構造化** し、名前付きのメンバーと同じ編集モデルで扱えるようにします。

構造化するかどうかの基準は、「編集したい属性、またはバージョンの違いを吸収する属性を持つエンティティは構造化する」です。`nbt={}` で書くのは、一度きりの特殊な NBT だけです。

- **構造化する (第一級)**: ブロックエンティティを持つブロックである `sign` と `bed`、面に掛ける `painting` と `item_frame`、床に立つ `armor_stand` と `villager` (と `trade`)、そして display の `text_display`、`block_display`、`item_display`。`hologram` は、エディションのテーマが束縛するとおり、テキストの display か看板として組み上げます ([バージョンとエディション §10.7](/ja/spec/versioning-editions/#代替を書くとき))。
- **汎用の受け皿**: そのほかのエンティティには `spawn type=<entity> at=... [nbt={...}]` を使います。

どれもキーワードです。`sign` と `bed` は [ブロックステート §6.1](/ja/spec/blockstate/#61-既定では導出し上書きで昇格する) のとおりに置き、エンティティは下の節のとおりに置きます。どのエンティティも、器具と同じく行の `at=` で置きます。壁際なら `at=<side>.outside` か `at=inside.<side>` と `offset=`、`y=` で、座標の三つ組なら `at=x,y,z` です。何も置かない行、つまり `at=` がないか別の形の行は `E_INVALID_MEMBER` で、壁や構造物が収められない `at=` は、チェストの `at=` と同じように拒否または保留します ([ブロックステート §6.1](/ja/spec/blockstate/#61-既定では導出し上書きで昇格する))。メンバーはそれぞれエンティティを 1 つ置き、あとのメンバーがその上に組むことはありません。エンティティはブロックからセルを取らず、ブロックもエンティティからセルを取りません。

```
villager      id=trader at=inside.back offset=3 y=1 profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=2,2,2 mat_slot=lamp scale=0.5
item_display  id=trophy at=5,1,2 facing=east item=diamond_sword
@edition java
  spawn id=cat type=cat at=1,1,1 nbt={variant:"minecraft:black"}
```

`scale=` のように小数を読むキーは、埋める NBT のフィールドが持つ型に合わせて、float か double として書き出されます。

`nbt={...}` のコンパウンドは、もともとエディション固有のものです。Java と Bedrock では、エンティティやブロックエンティティのフィールドの名前が異なり、そのフィールドを持つレジストリパックもないので、生の ID を検査するように ([マテリアルとテーマ §7.4](/ja/spec/materials-themes/#74-生のブロック)) コンパウンドをターゲットに照らして検査するものはありません。そのため、`nbt=` を書く行は、そのフィールド名で綴ったエディションの `@edition` ガードの下に置きます。ガードは、生のメンバーと同じようにその行を囲みます ([バージョンとエディション §10.7](/ja/spec/versioning-editions/#edition-ガードはエスケープハッチを囲む))。ガードの外にある場合は、コンパウンドの位置で `E_NBT_OUTSIDE_EDITION_GUARD` になります。コンパウンドを読むキーワードは、エンティティの `painting`、`item_frame`、`armor_stand`、`villager`、3 つの display、`spawn` と、`raw_block` です。ほかの行では `nbt=` は `E_UNKNOWN_ARGUMENT` です。その行を残すビルドは、エンティティについて書き出すものの上に、コンパウンドを最上位のキーごとに重ねます。コンパウンドが示すキーは書き出しのキーを置き換え、それ以外のキーは加わります。`raw_block` のコンパウンドはそのブロックのブロックエンティティで、書かれたとおりに書き出します。コンパイラが検査するのはコンパウンドの構文だけで、中身は検査しません。書き出しは、文字に ASCII の外の文字を含むコンパウンドを、書き出す NBT が運べないので拒否し、エンティティのセルを示します。

村人の交易所は定番の建築で、display エンティティは現代的な装飾に欠かせません。これらを `nbt={}` で書かせると、生成の品質と編集の安定性が落ちるので、構造化します。

ブロックエンティティ (看板) と本物のエンティティ (絵画) は、NBT の上では別物ですが、DSL では同じセレクタの文法を使います。両者を区別するのはコンパイラの仕事です。ブロックエンティティはそのブロックと並べて持ち、エンティティは構造のエンティティのリストに書き込みます。Java はそれぞれを `.nbt` の `entities` のエントリとして書き出します。エンティティの点を 3 つの double で表す `pos`、セルを 3 つの int で表す `blockPos`、そして `id` と下の節が示すフィールドを持つ `nbt` です。Bedrock はそれぞれを `.mcstructure` の `entities` に書き出し、`identifier`、3 つの float で表す点 `Pos`、`Rotation`、止まった `Motion`、リストの中での位置を 1 から数えた `UniqueID`、エンティティを示す `definitions` を持たせます。ただし額縁は、Bedrock がブロックとして組むので例外です。点もセルも構造の座標で、構造の低い隅から数えるので、ゲームは構造を置くときに一緒に動かします。リストはエンティティのセルの順、つまり x が最も速く、次に z、次に y の順に並び、1 つのセルに 2 つあれば行を書いた順に並びます。

アイテム、エンチャント、エンティティの種類を持つレジストリパックはありません。アイテムの ID、エンチャントの ID、`spawn` の `type=` は書かれたとおりに書き出します。どれも、ビルドが `minecraft:` の名前空間を付ける語か、名前空間付きの ID です。ゲームは、持たないものを構造を置くときに捨てます。

### 掛けるエンティティ

`painting` と `item_frame` は、後ろのブロックの面に掛けます。壁際では、同じ `at=`、`offset=`、`y=` のチェストが取るセルを取り、壁の面に、壁から離れる向きで掛けます。そのため `facing=` が選ぶものは残らず、行に書けば `E_INVALID_MEMBER` です。座標の三つ組では、Tier 0 部品が取るセルを取り ([レッドストーン §14.1](/ja/spec/redstone/#tier-0-の部品))、`facing=` で向く方向を示さなければなりません (MUST)。エンティティは、セルの反対側、つまり後ろのブロックに掛かります。絵画が向くのは `north`、`south`、`east`、`west` で、額縁はさらに、下のブロックの上に寝かせる `up` と、上のブロックの下に付ける `down` も向きます。三つ組で `facing=` がないこと、ほかの識別子を書くことは `E_INVALID_MEMBER` で、識別子でない値は `W_IGNORED_ARGUMENT` で、キーを書かなかったものとして読みます。エンティティの点は、セルの中央を、掛ける面の方へ 15/32 ブロック動かした点です。ゲームは掛けるエンティティを、厚さ 1/16 ブロックとしてそこに保ちます。セルは、ゲームがエンティティを見つけるセルです。後ろに何が要るかは制約検査が決めます ([Lint §11.4](/ja/spec/lint/#エンティティの-aabb))。

```
painting   id=hall_art at=inside.front offset=2 y=1 variant=kebab
painting   id=mural    at=3,2,1 facing=south variant=pointer
item_frame id=relic    at=inside.back offset=4 y=2 item=diamond glowing=true
item_frame id=floor    at=2,1,2 facing=up
```

`painting` は、ターゲットの `paintings` テーブルが与える名前で、`variant=` に絵画を示さなければなりません (MUST) ([バージョンとエディション §10.3](/ja/spec/versioning-editions/#103-バックエンドはデータテーブル))。`kebab` などです。絵画はその種類の `width` × `height` のセルを覆います。行のセルは、前から見た絵画の左下のセルで、そこから右と上へ広がります。ゲームは絵画を、左下から右へ `(width - 1) / 2`、上へ `(height - 1) / 2` (切り捨て) のセルで保つので、そのセルが絵画のセルで、点もそのセルの点です。セルが本体のブロック配列からはみ出す絵画は掛けず、`W_DEFERRED_MEMBER` になります。固定したターゲットがその絵画を持たない `variant=` は `E_INVALID_MEMBER` で、持っている絵画を、またはほかのバージョンが持つならそのバージョンを示します。エディションだけを固定し、バージョンを固定しないときは、どれかのバージョンが持つ種類を、それを持つすべてのバージョンが一致する大きさで掛けます。パックがなければ、種類を検査せず、絵画は 1 つのセルを覆います。`variant=` がないこと、識別子でないことは `E_INVALID_MEMBER` です。

`item_frame` は `item=` が示すアイテムを 1 つ持ち、なければ空のまま掛けます。`glowing=true` で輝く額縁になります。ID でない `item=` と、`true` でも `false` でもない `glowing=` は `W_IGNORED_ARGUMENT` です。

Java は絵画の `variant` を名前空間付きの ID で、`facing` を byte で書き出します。南が 0、西が 1、北が 2、東が 3 です。額縁は `item_frame` か `glow_item_frame` として、`Facing` を byte で、下を 0、上を 1、北を 2、南を 3、西を 4、東を 5 として書き出し、`Item` も書き出します。Bedrock は絵画を `minecraft:painting` として、ターゲットの `paintings` テーブルが与える綴りの `Motive` と、Java の `facing` と同じ番号の `Direction` で書き出します。Bedrock は額縁をブロック `frame` か `glow_frame` として組み、`facing_direction` を Java の `Facing` と同じ番号にし、`item_frame_map_bit` と `item_frame_photo_bit` を切ります。そのブロックエンティティ `ItemFrame` か `GlowItemFrame` は、`Item`、0 の `ItemRotation`、1 の `ItemDropChance` を持ち、行の `nbt=` を受け取ります。ブロックは額縁のセルを取るので、書き出しは、メンバーがブロックを組んだセルにある額縁を拒否し、セルとブロックを示します。

### 立つエンティティ

`armor_stand`、`villager`、`spawn` は、セルの床の中央に立ちます。壁際では、チェストが取るセルを取って壁から離れる方向を向くので、行の `facing=` は `E_INVALID_MEMBER` です。座標の三つ組では、Tier 0 部品が取るセルを取り、`facing=` が示す `north`、`south`、`east`、`west` を向きます。書かなければ `south` です。`facing=` のほかの識別子は `E_INVALID_MEMBER` で、識別子でない値は `W_IGNORED_ARGUMENT` です。どちらの書き出しも、エンティティを `Rotation` で向けます。ヨーは南が 0、西が 90、北が 180、東が 270 です。

`armor_stand` は自分のキーを持たず、腕なしで書き出します。

`villager` は `profession=` と `level=` を取ります。`profession=` は `none`、`armorer`、`butcher`、`cartographer`、`cleric`、`farmer`、`fisherman`、`fletcher`、`leatherworker`、`librarian`、`mason`、`nitwit`、`shepherd`、`toolsmith`、`weaponsmith` のどれかで、`level=` は `novice`、`apprentice`、`journeyman`、`expert`、`master` のどれかです。書かなければ `none` の村人で、`novice` です。どちらもほかの値は `E_INVALID_MEMBER` で、村人は置きません。村人は平原の村人です。経験値はそのレベルの最小値で、`novice` から順に 0、10、70、150、250 です。ただし `novice` は 1 にします。経験値が 0 の村人は、最初に見つけた職業ブロックに職業を奪われるからです。

`trade` の行は、同じ本体の中で、`villager=` が `id=` を示す村人の取引の 1 つです。村人は取引を、行を書いた順に並べます。`buy=` が示すアイテムを `count=` 個買い、`sell=` が示すアイテムを `sell_count=` 個売ります。どちらも 1 から 64 の整数で、書かなければ 1 です。`enchant=` は売るアイテムにエンチャントを付け、そのレベルは `enchant_level=` で、1 から 255 の整数、書かなければ 1 です。エンチャントの本はそれを蓄え、ほかのアイテムはそれを帯びます。本体のどの村人も示さない `villager=`、ないか形の崩れた `buy=` や `sell=` は `E_INVALID_MEMBER` で、取引は並びません。範囲の外か別の種類の個数やレベル、ID でない `enchant=`、`enchant=` のない `enchant_level=` は `W_IGNORED_ARGUMENT` です。取引は 12 回売ると村人が補充し、価格は需要に応じて 0.05 ずつ動きます。`trade` 自身は何も置きません。

```
villager id=smith at=inside.left offset=2 y=1 profession=weaponsmith level=expert
trade villager=smith buy=emerald count=12 sell=iron_sword enchant=sharpness enchant_level=3
armor_stand id=stand at=4,1,2 facing=west
```

`spawn` は、`type=` が示すエンティティを、`nbt=` が与えるものとともに置きます。`type=` は書かなければなりません (MUST)。`spawn` は、どのキーワードも構造化しないエンティティのためのエスケープハッチなので、行が何を書いても `@edition` ガードはそれを囲めます ([バージョンとエディション §10.7](/ja/spec/versioning-editions/#edition-ガードはエスケープハッチを囲む))。

Java は村人の `VillagerData` を、`type`、名前空間付きの `profession`、1 から数える `level` として書き出し、`Xp` と、`Offers` の下に `Recipes` として取引も書き出します。取引はそれぞれ `buy`、`sell`、12 の `maxUses`、`rewardExp`、1 の `xp`、0.05 の `priceMultiplier` を持ちます。Bedrock は村人を `minecraft:villager_v2` として、職業をゲームが番号を付ける `Variant` と `PreferredProfession` として、レベルを 0 から数える `TradeTier` と、Java の `Xp` が与える `TradeExperience` として書き出します。取引は `Offers` の下に `Recipes` として、それぞれ `buyA`、`buyCountA`、`sell`、`tier`、`maxUses` を持ち、職業はエンティティ自身のものと並べて `definitions` に書きます。

どちらの書き出しも、アイテムをターゲットが読む形のアイテムスタックとして保存します。Java は 1.20.5 より前では、byte の `Count` と、`tag` の中の `Enchantments` (本なら `StoredEnchantments`) に `id` と `lvl` でエンチャントを保存します。1.20.5 からは `count` とデータコンポーネント `minecraft:enchantments` か `minecraft:stored_enchantments` で、エンチャントの ID をレベルに対応させ、1.21.5 より前はその対応を `levels` の下に置きます。Bedrock はアイテムの `Name`、`Count`、`Damage`、`WasPickedUp` と、`tag` の下の `ench` に Bedrock が与える番号でエンチャントを保存します。そのため書き出しは、番号を持たないエンチャントを拒否し、持っているものを示します。

### display

`text_display`、`block_display`、`item_display` は、立つ場所に文字、ブロック、アイテムを描き、防具立てと同じように立ちます。つまりセルの床に、壁か `facing=` が示す方向を向いて立ちます。どれも `scale=` を取ります。0 より大きい数で、書かなければ 1 です。描くものをそれぞれの軸に沿って拡大し、ほかの値は `W_IGNORED_ARGUMENT` です。

- `text_display` は `text=` を表示します。1 行を表す文字列か、上からの行を表す文字列のリストです。`billboard=` のとおりに見る人の方へ回ります。書かないときの `fixed` は回らず、`vertical` は縦の軸の周りに、`horizontal` は横の軸の周りに、`center` は両方の周りに回ります。`text=` の別の種類の値と、ほかの `billboard=` は `W_IGNORED_ARGUMENT` です。
- `block_display` は、`floor` がスロットからブロックを取るように、`mat_slot=` が束縛するブロックを表示します。
- `item_display` は、`item=` が示すアイテムを表示します。`item=` は書かなければなりません (MUST)。`item=` がないこと、ID でないことは `E_INVALID_MEMBER` で、display は置きません。

Java はそれぞれの display の `Rotation`、`billboard`、そして回さず、動かさず、`scale=` で拡大する `transformation` を書き出します。テキストの display の `text` は行を改行でつないだもので、それぞれのリリースが読むとおり、1.21.5 より前は JSON の文字列、1.21.5 からはただの文字列です。ブロックの display の `block_state` は、構造のパレットが書くとおりのブロックで、アイテムの display の `item` はアイテムスタックです。

Bedrock には display エンティティがありません。Bedrock のターゲットでは、それぞれの display は `E_PARITY_UNSUPPORTED` になり、[バージョンとエディション](/ja/spec/versioning-editions/#代替を書くとき) が代わりのものについて与える注記を伴います。Bedrock の書き出しは、届いた display を拒否します。両方のエディションが組み上げる浮かぶ文字は `hologram` です。

### ブロックエンティティ

ビルドは、ビルドが決めたものをブロックエンティティに持つブロックそれぞれについて、ブロックエンティティを書き出します。看板の文字、ベッドの色、回路のコンパレーターが出力する強度がそれで ([レッドストーン §14.5](/ja/spec/redstone/#145-配置配線-place-and-route))、コンパレーターを敷くのは Java の回路だけです。ブロック配列は、それぞれをブロックのセルと並べて、どのエディションの形でもなく持ち、書き出しはそれぞれのエディションの形で保存します。Java では `.nbt` の `blocks` にあるそのブロックのエントリの `nbt` として、Bedrock では `.mcstructure` の `block_position_data` の下に、`block_indices` の中でのそのブロックの位置をキーとして保存します。ブロックエンティティは、それを決めたメンバーがブロックのセルを取っている間だけ保たれます。そのため、あとのメンバーが看板の上に組むと、看板の文字もいっしょに消えます。`chest`、`dispenser`、`daylight` が置くブロックのように ([ブロックステート §6.1](/ja/spec/blockstate/#61-既定では導出し上書きで昇格する)、[レッドストーン §14.2](/ja/spec/redstone/#142-信号バインディング))、ブロックエンティティを持つほかのブロックは、データを持たずに置かれ、ゲームが空のブロックエンティティを与えます。ただし、そのブロックを置く `raw_block` が `nbt=` を書けば、それを持ちます。

```
sign id=menu at=front.outside offset=1 y=2 text=["The Inn", "rooms"] glowing=true
sign id=shut at=2,1,2 facing=north back="Closed"
bed  id=cot  at=inside.back offset=1 y=1 mat_slot=quilt
```

看板の `text=` は表、つまり看板が向く側の面を、`back=` は裏を書きます。それぞれ 1 行を表す文字列か、上からの行を表す 4 つまでの文字列のリストです。行が書かない面は空白で、`glowing=true` は両方の面の文字を光らせます。4 行を超える面は `E_INVALID_MEMBER` で、看板は組みません。ほかの種類の値は、リストの要素も含めて `W_IGNORED_ARGUMENT` で、その面は空白のまま、またはその文字は光らないままになります。文字は黒です。Java はそれぞれの面を 4 つのメッセージと `has_glowing_text` として保存し、メッセージはそれぞれのリリースが読むとおり、1.21.5 より前は JSON の文字列、1.21.5 からはただの文字列です。Bedrock はそれぞれの面を、行を改行でつないだ 1 つの `Text` として保存し、光る文字は `IgnoreLighting` で表します。書き出しは、文字に ASCII の外の文字を含む看板を、書き出す NBT が運べないので拒否し、看板のセルを示します。

Java はベッドの色を `blue_bed` のようにブロックで表し、そのためにブロックエンティティには何も持ちません。Bedrock のベッドのブロックは `bed` の 1 つだけで、色はそれぞれの半分のブロックエンティティに、ゲームが染料に与える番号として持ちます。Bedrock の `@bed.default` と `@bed.<colour>` のトークンは `bed` に解決し、それぞれが表す Java のベッドを記録します ([バージョンとエディション §10.3](/ja/spec/versioning-editions/#103-バックエンドはデータテーブル))。書き出しはその色を保存します。そのようなトークンが示さない Bedrock のベッドは、色を示さないベッドをゲームが保存するとおり、白で保存します。

Bedrock の書き出しは、Java ではブロック自身のステートであるものを Bedrock がブロックエンティティに持つブロックについても、そのブロックエンティティを埋めます。組になったチェストのそれぞれの半分は、自分の位置と同じく構造の座標で、もう一方の半分のセルを示し、左の半分が組を率います。もう一方の半分となる、同じ向きの同じブロックが隣にない半分は、単体のチェストです。音符ブロックは `note` を、0 でないときにそこに持ちます。

## 8.2 アンカーの規約

**変更の余地あり。** 下に挙げる window の `shape=` は、まだ組み上げられません ([互換性ティア](/ja/spec/compatibility/#変更の余地あり))。

絵画、額縁、アーチ窓、階段室、張り出した屋根は、宣言したサイズと実際に占める AABB が一致しません。ここを曖昧にしたままだと、編集の安定性、テーマの差し替え、実装間の互換性がすべて損なわれます。

この曖昧さをなくすため、すべてのプリミティブは IR の中で 4 つの情報を持ちます。`anchor` (基準点)、宣言上の bbox、実際の bbox、取り付ける面です。lowering は、セルを 1 つでも組み上げるか、エンティティを置くすべてのメンバーについてこの 4 つを導き、`resolved_state` の隣に記録します ([ブロックステートモデル §6.2](/ja/spec/blockstate/#62-intent_state-と-resolved_state))。

- **アンカー**は、メンバーの行がそのメンバーを置く基準のセルです。
- **宣言上の bbox** は、ほかのメンバーが塗る前に、行が指すすべてのセルを囲む箱です。
- **実際の bbox** は、本体を塗り終えた時点でメンバーが持っているすべてのセルを囲む箱です。ほかのメンバーが端のセルを取れば宣言上の bbox より小さくなり、屋根の張り出しのように行が指す範囲の外まで組み上げれば大きくなります。エンティティはブロックからセルを取らないので、実際の bbox は宣言上の bbox と同じです。
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
| `beam` | 壁の左端にある並びのセル | 並び | 壁の外側の面 |
| 壁際の `chest`、`trapdoor`、`sign`、センサー、アクチュエーター、Tier 0 部品、立つエンティティ、display | 1 つのセル | その 1 つのセル | 立っている壁の面 |
| `bed` | 頭 | 両半分 | 頭を付けた壁の面。座標の三つ組で置くときはなし |
| 座標の三つ組で置く `trapdoor`、`sign`、`note_block`、Tier 0 部品、立つエンティティ、display、`raw_block` | 1 つのセル | その 1 つのセル | なし |
| `item_frame` | 1 つのセル | その 1 つのセル | 掛ける面 |
| `painting` | 左下のセル | 覆うすべてのセル | 掛ける面 |
| `fill`、`raw_fill` | 箱の低い隅 | `from=` と `to=` が張る箱 | なし |
| `circuit` | 箱の低い隅 | 確保するすべての層 | なし |

window の長方形は、`repeat=` の連なりが打つ 1 つずつと、`sym=true` のときの鏡像です。そのため、window と重なるので省かれた鏡像は、宣言上の bbox には残り、実際の bbox には入りません ([構文 §5.4](/ja/spec/syntax/#54-セレクタ))。壁際の器具は、壁の外側から (`at=front.outside`) なら外側の面に、内側から (`at=inside.front`) なら内側の面に据えます。ブロックを 1 つも敷かない `circuit` は、確保した層を空気で書くので、その層を持ちます。絵画のセルは、前から見てアンカーから右へ広がるので、前の壁の内側の絵画は `-x` へ、外側の絵画は `+x` へ広がります ([§8.1](#掛けるエンティティ))。`trade` は何も置かないので、4 つのどれも持ちません。

**window の `anchor=`** は、`offset=` と `y=` が置く window のセルを、壁の外から見て名指しします。`bottom_left`、`bottom_center`、`bottom_right`、`middle_left`、`center`、`middle_right`、`top_left`、`top_center`、`top_right` のどれかです。書かなければ `bottom_left`、つまり window の最初の列と最も低い段を置きます ([構文 §5.4](/ja/spec/syntax/#54-セレクタ))。

**roof の `footprint=` と `bounds=`** は、屋根が覆うセルと、張り出しが本体に何をするかを名指しします。どちらも取る値は 1 つです。`footprint=struct` は構造物の `size=` の敷地で、`bounds=expand` は本体のブロック配列を敷地の四方へ `overhang=` だけ広げます ([コンパイルモデル §4.7](/ja/spec/compilation/#47-level-によるまとまりと体積の導出))。どちらを書いても、書かないときと同じものを組み上げます。ほかの値は `E_INVALID_MEMBER` で、屋根は何も描きません。それでも、書かれたとおりの構造物は形のとおりに立ち上がります ([Lint](/ja/spec/lint/#lowering))。

**AABB の重なり**は、1 つの本体の中では優先度による統合で解決し、本体どうしではエラーです。本体のメンバーは 1 つのブロック配列を塗るので、2 つのメンバーが指すセルは後のフェーズが取り、同じフェーズの中では後に書いた行が取ります。後者は `W_PHASE_CONFLICT` で報告します ([コンパイルモデル §4.8](/ja/spec/compilation/#48-同一フェーズ内の衝突とパレット))。セルを失ったメンバーも宣言上の bbox はそのままで、実際の bbox はまだ持っているセルを囲みます。エンティティは統合しません。その箱がブロック、扉、ほかのエンティティに入り込むことは、制約検査の `W_ENTITY_CLIPPING` が扱います ([Lint §11.4](/ja/spec/lint/#エンティティの-aabb))。2 つの `place` 行は配列を共有しません。本体はそれぞれ独立した構造物で、埋めないすべてのセルに空気を書くので、片方を重ねて置くと元にあったものが消え、統合するものがありません。この重なりはエラーです ([Lint](/ja/spec/lint/#意味カテゴリ))。ただし、まだどのコマンドも検査しません。

隣接するブロックに依存するブロックステート (階段、フェンス) は、干渉を検出せずに上書きすると壊れます。たとえば、内角になるはずの階段が外角のまま宙に浮いてしまいます。境界のブロックステートを解決し直すのは、IR の層の責任です。

下の例では、window の `shape=arch` は受け付けますがどのパスも読まないので `W_IGNORED_ARGUMENT` になります ([Lint](/ja/spec/lint/))。絵画の左下のセルは、前の壁の内側で `offset=2` と `y=2` が置くセルです。window の `anchor=bottom_center` は最初の列を 3 に置き、roof の `footprint=` と `bounds=` は、書かないときに組み上げるものを名指しします。

```
painting id=hall_art at=inside.front offset=2 y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 どこで線を引くか

看板、絵画、額縁、ベッドは建築の雰囲気を左右するので、構造化して扱います。チェストの中身、村人の持ち物など、建築の精度に関わらない情報は構造化しません。汎用の `spawn` の `nbt={}` か、エスケープハッチで書きます。
