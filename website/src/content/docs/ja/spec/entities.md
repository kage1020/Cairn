---
title: "8. エンティティ"
---

## 8.1 二段モデル
`nbt={}` を完全に開放するとテーマ/編集/lint/バージョン耐性が壊れます。**重要なエンティティは構造化**
し、名前付きメンバ編集モデルに乗せます。

昇格基準: **「編集したい属性」または「バージョン差を吸収する属性」を持つエンティティは構造化する。
ワンオフの特殊 NBT のみ `nbt={}` で逃がす**。

- **ファーストクラス、構造化**: `sign`、`painting`、`item_frame`、`armor_stand`、`villager` (+`trade`)、
  `display` (text/block/item)、`bed` (ブロック扱い)。
- **汎用フォールバック**: `spawn id=.. type=<entity> at=<selector> [nbt={...}]` (その他の mob)。

```
villager id=trader at=stall[0] profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=front.above block=@lantern scale=0.5
item_display  id=trophy at=counter item=diamond_sword rotation=y90
spawn id=cat type=cat at=inside.floor nbt={variant:"black"}
```

村人取引所は定番建築で、display エンティティは現代的な装飾の中核です。これらを `nbt={}` に流すと
生成品質と編集の安定性が落ちるため構造化します。

ブロックエンティティ (看板など) と真のエンティティ (絵画など) は NBT 上は別物ですが、DSL 上は同一の
セレクタ文法を共有します。区別はコンパイラの責務です。

## 8.2 可変サイズ要素のアンカー規約 (最重要の未解決事項)
絵画、額縁、アーチ窓、階段室、張り出し屋根は、宣言サイズと実際の占有 AABB が異なります。曖昧にすると
編集の安定性、テーマの切り替え、相互運用がすべて崩れます。

- **すべてのプリミティブが `anchor` (基準点)、宣言 bbox、実 bbox、ホスト面を IR に持つ**。
- 重なる AABB の解決ルールを仕様で固定する: **優先マージか lint エラー** のいずれか
  ([Lint](lint))。
- 隣接依存のブロックステート (階段、フェンス) は、干渉検出なしで上書きすると壊れます (「内角階段が
  外角のまま空中に取り残される」)。境界ブロックステートの再解決は IR 層の責務です。

```
painting id=hall_art side=inside.front anchor=center y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 エンティティを描く範囲
看板、絵画、額縁、ベッドは建築の「雰囲気」に寄与するので採用します。チェストの中身、村人のインベン
トリなど建築精度に寄与しない情報は構造化せず、汎用 `spawn` `nbt={}` かエスケープハッチに送ります。
