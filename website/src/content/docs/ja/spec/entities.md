---
title: "8. エンティティ"
---

## 8.1 2 つの段階

**変更の余地あり。** 下に挙げるメンバーのうち、組み上げられるのは `sign` と `bed` だけです ([互換性ティア](/ja/spec/compatibility/#変更の余地あり))。

すべてのエンティティを `nbt={}` で自由に書けるようにすると、テーマ、編集、lint、バージョンの違いへの強さが失われます。そこで、重要なエンティティは **構造化** し、名前付きのメンバーと同じ編集モデルで扱えるようにします。

構造化するかどうかの基準は、「編集したい属性、またはバージョンの違いを吸収する属性を持つエンティティは構造化する」です。`nbt={}` で書くのは、一度きりの特殊な NBT だけです。

- **構造化する (第一級)**: `sign`、`painting`、`item_frame`、`armor_stand`、`villager` (と `trade`)、`display` (text / block / item)、`bed` (ブロックとして扱う)。
- **汎用の受け皿**: そのほかの mob には `spawn id=.. type=<entity> at=<selector> [nbt={...}]` を使います。

このうち `sign` と `bed` はキーワードで、[ブロックステート §6.1](/ja/spec/blockstate/#61-既定では導出し上書きで昇格する) のとおりに置きます。ほかのものは `spawn` を含めてまだキーワードではなく、すべて `E_UNKNOWN_KEYWORD` になるので、エンティティを置くビルドはまだありません。下の例には、パーサがまだ受け付けない構文も含まれています。小数 (`scale=1.5`) と `nbt={...}` の値は `E_PARSE` になります ([構文 §5.1.1](/ja/spec/syntax/#511-リテラルと区切り))。

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

### ブロックエンティティ

ビルドは、ビルドが決めたものをブロックエンティティに持つブロックそれぞれについて、ブロックエンティティを書き出します。看板の文字、ベッドの色、回路のコンパレーターが出力する強度がそれで ([レッドストーン §14.5](/ja/spec/redstone/#145-配置配線-place-and-route))、コンパレーターを敷くのは Java の回路だけです。ブロック配列は、それぞれをブロックのセルと並べて、どのエディションの形でもなく持ち、書き出しはそれぞれのエディションの形で保存します。Java では `.nbt` の `blocks` にあるそのブロックのエントリの `nbt` として、Bedrock では `.mcstructure` の `block_position_data` の下に、`block_indices` の中でのそのブロックの位置をキーとして保存します。ブロックエンティティは、それを決めたメンバーがブロックのセルを取っている間だけ保たれます。そのため、あとのメンバーが看板の上に組むと、看板の文字もいっしょに消えます。`chest`、`dispenser`、`daylight` が置くブロックのように ([ブロックステート §6.1](/ja/spec/blockstate/#61-既定では導出し上書きで昇格する)、[レッドストーン §14.2](/ja/spec/redstone/#142-信号バインディング))、ブロックエンティティを持つほかのブロックは、データを持たずに置かれ、ゲームが空のブロックエンティティを与えます。

```
sign id=menu at=front.outside offset=1 y=2 text=["The Inn", "rooms"] glowing=true
sign id=shut at=2,1,2 facing=north back="Closed"
bed  id=cot  at=inside.back offset=1 y=1 mat_slot=quilt
```

看板の `text=` は表、つまり看板が向く側の面を、`back=` は裏を書きます。それぞれ 1 行を表す文字列か、上からの行を表す 4 つまでの文字列のリストです。行が書かない面は空白で、`glowing=true` は両方の面の文字を光らせます。4 行を超える面は `E_INVALID_MEMBER` で、看板は組みません。ほかの種類の値は、リストの要素も含めて `W_IGNORED_ARGUMENT` で、その面は空白のまま、またはその文字は光らないままになります。文字は黒です。Java はそれぞれの面を 4 つのメッセージと `has_glowing_text` として保存し、メッセージはそれぞれのリリースが読むとおり、1.21.5 より前は JSON の文字列、1.21.5 からはただの文字列です。Bedrock はそれぞれの面を、行を改行でつないだ 1 つの `Text` として保存し、光る文字は `IgnoreLighting` で表します。書き出しは、文字に ASCII の外の文字を含む看板を、書き出す NBT が運べないので拒否し、看板のセルを示します。

Java はベッドの色を `blue_bed` のようにブロックで表し、そのためにブロックエンティティには何も持ちません。Bedrock のベッドのブロックは `bed` の 1 つだけで、色はそれぞれの半分のブロックエンティティに、ゲームが染料に与える番号として持ちます。Bedrock の `@bed.default` と `@bed.<colour>` のトークンは `bed` に解決し、それぞれが表す Java のベッドを記録します ([バージョンとエディション §10.3](/ja/spec/versioning-editions/#103-バックエンドはデータテーブル))。書き出しはその色を保存します。そのようなトークンが示さない Bedrock のベッドは、色を示さないベッドをゲームが保存するとおり、白で保存します。

Bedrock の書き出しは、Java ではブロック自身のステートであるものを Bedrock がブロックエンティティに持つブロックについても、そのブロックエンティティを埋めます。組になったチェストのそれぞれの半分は、自分の位置と同じく構造の座標で、もう一方の半分のセルを示し、左の半分が組を率います。もう一方の半分となる、同じ向きの同じブロックが隣にない半分は、単体のチェストです。音符ブロックは `note` を、0 でないときにそこに持ちます。

## 8.2 アンカーの規約

**変更の余地あり。** `painting` と、下に挙げるアンカーのキー (`anchor=`、`shape=`、`footprint=`、`bounds=`) は、まだ組み上げられません ([互換性ティア](/ja/spec/compatibility/#変更の余地あり))。

絵画、額縁、アーチ窓、階段室、張り出した屋根は、宣言したサイズと実際に占める AABB が一致しません。ここを曖昧にしたままだと、編集の安定性、テーマの差し替え、実装間の互換性がすべて損なわれます。この章で最も大きな未決事項です。

この曖昧さをなくすため、すべてのプリミティブは IR の中で 4 つの情報を持ちます。`anchor` (基準点)、宣言上の bbox、実際の bbox、取り付ける面です。

現時点では、IR はこの 4 つのどれも持っていません。下の例では、`painting` は `E_UNKNOWN_KEYWORD` になります。window の `anchor=` と `shape=`、roof の `footprint=` と `bounds=` は受け付けますが、どのパスも読まないので、それぞれ `W_IGNORED_ARGUMENT` になります ([Lint](/ja/spec/lint/))。

AABB が重なったときの解決方法は、仕様で 1 つに固定します。優先度による統合か、lint のエラーのどちらかです ([Lint](/ja/spec/lint/))。隣接するブロックに依存するブロックステート (階段、フェンス) は、干渉を検出せずに上書きすると壊れます。たとえば、内角になるはずの階段が外角のまま宙に浮いてしまいます。境界のブロックステートを解決し直すのは、IR の層の責任です。

```
painting id=hall_art side=inside.front anchor=center y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 どこで線を引くか

看板、絵画、額縁、ベッドは建築の雰囲気を左右するので、構造化して扱います。チェストの中身、村人の持ち物など、建築の精度に関わらない情報は構造化しません。汎用の `spawn` の `nbt={}` か、エスケープハッチで書きます。
