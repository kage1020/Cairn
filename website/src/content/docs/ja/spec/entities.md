---
title: "8. エンティティ"
---

## 8.1 2 つの段階

すべてのエンティティを `nbt={}` で自由に書けるようにすると、テーマ、編集、lint、バージョンの違いへの強さが失われます。そこで、重要なエンティティは **構造化** し、名前付きのメンバーと同じ編集モデルで扱えるようにします。

構造化するかどうかの基準は、「編集したい属性、またはバージョンの違いを吸収する属性を持つエンティティは構造化する」です。`nbt={}` で書くのは、一度きりの特殊な NBT だけです。

- **構造化する (第一級)**: `sign`、`painting`、`item_frame`、`armor_stand`、`villager` (と `trade`)、`display` (text / block / item)、`bed` (ブロックとして扱う)。
- **汎用の受け皿**: そのほかの mob には `spawn id=.. type=<entity> at=<selector> [nbt={...}]` を使います。

ただし、これらはまだどれもキーワードではなく、`spawn` を含めてすべて `E_UNKNOWN_KEYWORD` になるので、エンティティやブロックエンティティを置くビルドはまだありません。下の例には、パーサがまだ受け付けない構文も含まれています。座標の組 (`at=4,3,2`)、小数 (`scale=1.5`)、`nbt={...}` の値は `E_PARSE` になります ([構文 §5.1.1](/ja/spec/syntax/#511-リテラルと区切り))。

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
