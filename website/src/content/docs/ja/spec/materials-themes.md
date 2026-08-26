---
title: "7. マテリアルとテーマ"
---

## 7.1 依存性注入としてのスロット

構造体側は具体的なブロック名を書きません。`mat_slot` という注入ポイントを持つだけで、それに値を束ね
るのが `theme` です。CSS、あるいは依存性注入をブロックに適用したものです。これで「壁がどこにあるか」
(構造) と「どのブロックでどう飾るか」 (スタイル) が分かれます。

```
def cottage class=house size=9x7:
  floor  id=floor  mat_slot=floor
  walls  id=walls  class=outer mat_slot=wall height=4
  roof   id=roof   kind=gable  mat_slot=roof
  window id=front_windows class=small side=front y=2 repeat=2 mat_slot=glass

theme medieval:
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  walls[class=outer]  -> trim=@spruce_log     # セレクタによる部位ディテール
  window[class=small] -> frame=@spruce_wood
```

**カスケード。** メンバはマッチするすべてのセレクタ行のバインディングをソース順に集めるので、2 つの
行が同じキーを束ねたら後の値が残ります。同じ詳細度の 2 つのルールに対する CSS の規則と同じです。

属性が部分的に重なるだけの行はこの性質に依存しています。`window[class=small,side=front]` は自分が選
ぶメンバについて `window[class=small]` を上書きし、広い方の行だけが選ぶメンバは広い方のバインディン
グを保ちます。

**同じ** メンバを選ぶ 2 行は別の話です。キーワードも属性も同じなら 2 行はメンバ単位で一致するため、
両方が束ねるキーは先の行のものを読むものが 1 つも残りません。これが `E_DUPLICATE_SELECTOR` です ([Lint §11.1](lint#111-診断コード))。
同一性は意味で判定します。属性の順序は含まれず、`class=` / `id=` / `mat_slot=` の値はラベル文字列と
して比較されるので、`window[class=small]` と `window[class="small"]` は 1 つのセレクタです。一致して
いても異なるキーを束ねる行は報告しません。それらは合成され、長いバインディング列を 2 行に分けて書く
ことは許されています。

`def` / `theme` / `site` は、同じスロット保持型コンポーネント機構で統一されています
([コンポーネント・編集・複数建築](components-editing-sites))。

## 7.2 正準語彙

テーマが束ねるのは **正準トークン** であり、生のブロック ID ではありません。`(edition, version)` ご
とに ID、ステート名、ステート値、シリアライズを解決するのはバックエンドです ([バージョンとエディション](versioning-editions))。
LLM は `pillar_axis` も、リトルエンディアン NBT も、 Bedrock の `weirdo_direction` も知る必要があり
ません。

トークンは 2 段あります。

| 段 | 例 | 意味 |
|---|---|---|
| **正準ブロックトークン** | `@oak_planks` / `@water_cauldron` / `@oak_log[axis=x]` | Minecraft における特定の意味。意味を壊すサイレントなダウングレードは **禁止** で、`@water_cauldron` が `cauldron` になることはありません。 |
| **抽象マテリアルトークン** | `@floor.wood.broadleaf` / `@roof.dark_wood` | 美的な選択。テーマポリシーはこれをダウングレードしてかまいません (オーク ↔ シラカバ)。 |

```
theme cottage:
  slot floor -> @floor.wood.broadleaf   # 抽象: ターゲットとポリシーで解決
theme exact_oak:
  slot floor -> @oak_planks             # 正準: 1:1 に固定
```

## 7.3 バージョン・エディション横断のマッピング

正準トークンは 5 つのパターンを吸収します。解決テーブルの構造は
[バージョンとエディション](versioning-editions) にあります。

| パターン | 例 | ポリシー |
|---|---|---|
| 名前変更 1:1 | `@dirt_path` (旧 `grass_path`) | 自動解決。 |
| 分割 1:N | `@cauldron[fluid=water]` → `water_cauldron` | 意味トークンで分離。 |
| 統合 N:1 | `@oak_slab` (旧 `wooden_slab{variant}`) | ターゲットごとに解決。 |
| 新規 | `@cherry_planks` | `requires >=` の下限が必要。 |
| 削除 | ターゲットバージョンに存在しない | ハードエラーと代替案。 |

**吸収できるのは ID / ステート / シリアライズの差だけです。** 概念の不在とゲーム挙動の差は吸収しませ
ん。
