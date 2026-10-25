---
title: "3. アーキテクチャ"
---

```
Intent DSL
   ↓ parse
Semantic / Component-Theme IR    名前付きメンバー: id / class / role / mat_slot / intent_state
   ↓ resolve                     フェーズ評価、形状の展開、テーマの注入、ブロックステートの導出
block-array IR                   ボクセル格子 + パレット + block entities + entities  ← 共通のピボット
   ↓ serialize                   エディション・バージョン・フォーマットごとのバックエンド
.nbt (Java) / .litematic / .schem / .mcstructure (Bedrock)
```

## 3.1 block-array IR はすべてのフォーマットに共通のピボット

すべてのフォーマットのフロントエンドとバックエンドは、この層でつながります。差分 (diff)、IoU、シリアライズもすべてこの層で行います。block-array IR はボクセル格子、パレット、block entities、entities を持ち、フォーマット・エディション・バージョンのどれにも依存しません。

順方向の変換ではこの層が出力先になり、逆方向の変換では入力先になります ([エコシステム連携](/ja/spec/ecosystem-interop/))。

## 3.2 Intent IR は情報が豊富で、不変条件を持つ

名前付きのメンバーは、`id`、`class`、`role`、`mat_slot`、`intent_state`、`resolved_state` を持ちます ([ブロックステート](/ja/spec/blockstate/))。

raw のまま取り込んだだけでは、有効な Intent IR にはなりません。意味を付けて引き上げる (リフトする) ことで、はじめて Intent IR になります。その途中の成果物がどこまで進んだかは、`semantic_level: raw | grouped | lifted` で表します。

## 3.3 レッドストーンのサブ層

レッドストーンを論理で記述すると ([レッドストーン](/ja/spec/redstone/))、Intent IR と block-array IR の間に、役割の異なる 3 つの IR 層が入ります。HDL と同じ分け方です。

```
Logic IR      論理式と依存関係の DAG。エディションに依存せず、遅延はゼロ
Netlist IR    セルとネット。論理セルを選ぶ。まだ遅延はない
Placement IR  セルの座標と実際の配線長。ここで遅延と tick が決まる
```

論理はエディションに依存しませんが、配置配線の結果 (タイルとタイミング) はエディションごとに異なります。**遅延は Logic IR にも Netlist IR にも持たせません。** 遅延が決まるのは Placement IR です。

## 3.4 この分け方で得られるもの

最下層の block-array IR は、順方向と逆方向の両方で、すべてのフォーマットに共有されます。その上にあるメンバーの層 (Intent IR) は、不変条件を持つ独立した型です。たとえば、すべてのメンバーが `intent_state` を持つこと、すべてのスロットが解決済みであることが保証されます。

このように分けることで、シリアライズ、diff、lint、IoU による評価を下層で共有しながら、意味の層を型安全に保てます。
