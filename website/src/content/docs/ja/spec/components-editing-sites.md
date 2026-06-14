---
title: "9. コンポーネント、編集、複数建築"
---

## 9.1 def (コンポーネント)
`def` はスロット保持型 Component を定義し、`theme` および `site` と同じ機構で統一されています。これに
より参照系が、編集・テーマ化・複数建築接続のあいだで分裂しません。

- パラメータ化 (可変サイズなど) は許可します。再帰は禁止です。
- `def` は `requires version>=X` を宣言してかまいません。コンポジットの最小バージョンはその構成要素
  の最大値です ([versioning-editions.ja.md](versioning-editions))。

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=door  class=entry side=front at=center
  roof   id=roof  kind=gable mat_slot=roof
```

## 9.2 編集モデル
**明示 ID + 自動安定アドレスの組み合わせ**。重要なメンバは `id=` を持ち、未指定のメンバはコンパイラが
**意味ベースの安定アドレス** を自動付与します。アドレスは生成順ではなく parent / role / side / level /
offset から導かれるため、追記しても安定します。

編集はセレクタ/アドレスに対するパッチ DSL です:

```
edit window[class=vent][level=floor2] set shape=arch
edit window@front[0]                  set mat_slot=accent_glass
edit door[id=entry]                   set side=front at=center
```

「2階の窓だけアーチにする」のような概念レベルの編集が、全体を壊さずに行える必要があります。編集 diff
は `intent_state` のみを見るため ([blockstate.ja.md](blockstate))、導出結果の変化は編集の安定
性を損ないません。

## 9.3 複数建築 (site)
AI に絶対座標の計算をさせない。トポロジカルな関係制約で配置し、絶対座標への解決はコンパイラの責務
とします。

```
site village:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  connect home1.door to home2.door path=@gravel
```

各 struct はポート (位置 / 法線 / 幅) を露出し、`connect` がそれらを繋ぎます。structure block の 48³
上限を超える村や城は、複数 struct の合成で表現します。
