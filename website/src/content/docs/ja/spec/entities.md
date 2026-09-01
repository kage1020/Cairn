---
title: "8. エンティティ"
---

## 8.1 2 つの段

すべてを `nbt={}` に開放すると、テーマ・編集・lint・バージョン耐性が壊れます。そこで重要なエンティテ
ィは **構造化** し、名前付きメンバの編集モデルに乗せます。

昇格の基準は「編集したい属性を持つか、バージョン差を吸収する属性を持つエンティティは構造化する」です。
一度きりの特殊な NBT だけが `nbt={}` から逃げます。

- **構造化 (ファーストクラス)**: `sign` / `painting` / `item_frame` / `armor_stand` / `villager`
  (+ `trade`) / `display` (text / block / item) / `bed` (ブロックとして扱う)。
- **汎用フォールバック**: その他の mob 用の `spawn id=.. type=<entity> at=<selector> [nbt={...}]`。

```
villager id=trader at=stall[0] profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=front.above block=@lantern scale=0.5
item_display  id=trophy at=counter item=diamond_sword rotation=y90
spawn id=cat type=cat at=inside.floor nbt={variant:"black"}
```

村人の交易所は定番の建築で、display エンティティは現代的な装飾の中核です。これらを `nbt={}` に送ると
生成品質と編集の安定性が落ちるので、構造化します。

ブロックエンティティ (看板) と本物のエンティティ (絵画) は NBT では別物ですが、DSL では同じセレクタ
文法を共有します。区別するのはコンパイラの仕事です。

## 8.2 アンカー規約

絵画、額縁、アーチ窓、階段室、張り出した屋根は、宣言されたサイズと実際に占める AABB が食い違います。
曖昧なままだと編集の安定性、テーマの差し替え、実装間の互換性がすべて壊れます。この章で最も大きな未決
事項です。

そのため、すべてのプリミティブは IR で 4 つを持ちます。`anchor` (基準点)、宣言 bbox、実 bbox、
ホスト面です。

AABB が重なった場合の解決規則は仕様で固定します。優先マージか lint エラーのどちらかです ([Lint](lint))。
隣接依存のブロックステート (階段、フェンス) は、干渉検出なしに上書きされると壊れます。内角の階段が外
角のまま空中に浮く、といった具合です。境界のブロックステートを再解決するのは IR 層の責務です。

```
painting id=hall_art side=inside.front anchor=center y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 線を引く場所

看板・絵画・額縁・ベッドは建築の「雰囲気」に寄与するので採用します。チェストの中身、村人の持ち物、そ
の他の建築精度に寄与しない情報は構造化せず、汎用の `spawn` の `nbt={}` かエスケープハッチに送ります。
