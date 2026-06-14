---
title: "7. マテリアルとテーマ"
---

## 7.1 依存性注入としてのスロット
構造体側は具体的なブロック名を一切書かず、`mat_slot` (注入ポイント) のみを持ちます。`theme` が
スロットとセレクタに値を束ねます (CSS / 依存性注入のアナロジー)。これにより「壁がどこにあるか」 (構造)
と「どのブロックでどの装飾か」 (スタイル) が分離されます。

```
def cottage class=house size=9x7:
  floor  id=floor  mat_slot=floor
  walls  id=walls  class=outer mat_slot=wall height=4
  roof   id=roof   kind=gable  mat_slot=roof
  window id=front_windows class=small side=front y=2 repeat=2 mat_slot=glass

theme medieval:
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  walls[class=outer]  -> trim=@spruce_log     # セレクタによる部位ディテール注入
  window[class=small] -> frame=@spruce_wood
```

`def`、`theme`、`site` は、同じスロット保持型 Component 機構で統一されています
([components-editing-sites.ja.md](components-editing-sites))。

## 7.2 正規語彙 (canonical token)
テーマ/スロットが束ねる値は **正規トークン** であり、生のブロック ID ではありません。バックエンドが
`(edition, version)` ごとに ID、状態名、状態値、シリアライゼーションを解決します
([versioning-editions.ja.md](versioning-editions))。LLM は `pillar_axis` も、リトルエンディアン
NBT も、Bedrock の weirdo_direction も知る必要がありません。

正規トークンは **二段** です:
- **canonical block token** (Minecraft における意味): `@oak_planks` `@water_cauldron` `@oak_log[axis=x]`。
  サイレントな意味破壊的ダウングレードは **禁止** (例: `@water_cauldron` → `cauldron` は不可)。
- **abstract material token** (美的選択): `@floor.wood.broadleaf` `@roof.dark_wood`。テーマポリシーが
  ダウングレードしてよい (例: オーク↔シラカバ)。

```
theme cottage:
  slot floor -> @floor.wood.broadleaf   # 抽象: ターゲット/ポリシーで具体マテリアルに解決
theme exact_oak:
  slot floor -> @oak_planks             # canonical: 1:1 に固定
```

## 7.3 バージョン・エディション横断のマッピング
正規トークンは以下の 5 パターンを吸収します (解決テーブルの構造は
[versioning-editions.ja.md](versioning-editions)):

| パターン | 例 | ポリシー |
|---|---|---|
| 名前変更 1:1 | `@dirt_path` (grass_path→dirt_path) | 自動解決 |
| 分割 1:N | `@cauldron[fluid=water]` (cauldron→water_cauldron) | 意味トークンで分離 |
| 統合 N:1 | `@oak_slab` (wooden_slab{variant}→oak_slab) | ターゲットごとに解決 |
| 新規 | `@cherry_planks` | `requires >=` が必要 |
| 削除 | (ターゲットバージョンに存在しない) | ハードエラー + 代替案 |

**ID/状態/シリアライゼーションの差分のみ吸収可能** です。「概念の不在」や「ゲーム挙動の差分」は吸収
しません ([versioning-editions.ja.md](versioning-editions))。
