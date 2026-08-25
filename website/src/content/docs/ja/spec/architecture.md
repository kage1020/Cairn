---
title: "3. アーキテクチャ"
---

```
Intent DSL
   ↓ parse
Semantic / Component-Theme IR    名前付きメンバ: id / class / role / mat_slot / intent_state
   ↓ resolve                     フェーズ評価、ジオメトリ展開、テーマ注入、ブロックステート導出
block-array IR                   ボクセル格子 + パレット + block entities + entities  ← 普遍ピボット
   ↓ serialize                   エディション・バージョン・フォーマットごとのバックエンド
.nbt (Java) / .litematic / .schem / .mcstructure (Bedrock)
```

## 3.1 block-array IR が普遍ピボット

すべてのフォーマットのフロントエンドとバックエンドはこの層で出会い、diff・IoU・シリアライズもここで
起こります。ボクセル格子、パレット、block entities、entities を保持し、フォーマット・エディション・
バージョンに対して中立です。

順方向の出力先であり、逆方向の入力先でもあります ([エコシステム連携](ecosystem-interop))。

## 3.2 Intent IR は豊かで、不変条件を運ぶ

名前付きメンバは `id` / `class` / `role` / `mat_slot` / `intent_state` / `resolved_state` を持ちます
([ブロックステート](blockstate))。

raw なインポートは有効な Intent IR を生みません。意味的なリフトを経てはじめて到達します。その途上に
おける成果物の進捗は `semantic_level: raw | grouped | lifted` で表します。

## 3.3 レッドストーンのサブ層

レッドストーンを論理的に記述すると ([レッドストーン](redstone))、Intent IR と block-array IR の間に
役割の異なる 3 つの IR 層が入ります。HDL と同じ分け方です。

```
Logic IR      論理式と依存 DAG。エディション中立、ゼロディレイ
Netlist IR    セルとネット。論理セルの選択。まだディレイ無し
Placement IR  セル座標 + 実配線長。ここでディレイと tick が決まる
```

論理はエディション中立で、place-and-route の結果 (タイル、タイミング) はエディション固有です。
**ディレイは Logic IR にも Netlist IR にも載りません。** Placement IR で確定します。

## 3.4 この分割が買うもの

最下層の block-array IR は、順方向と逆方向、そしてすべてのフォーマットで共有されます。その上のメンバ /
Intent IR は不変条件を持つ独立した型です (すべてのメンバが `intent_state` を持つ、すべてのスロットが
解決済みである、など)。

この分離によって、シリアライズ・diff・lint・IoU 評価を下層で共有しつつ、意味層を型安全に保てます。
