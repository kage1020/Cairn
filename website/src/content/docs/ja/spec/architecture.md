---
title: "3. アーキテクチャ (三層 IR + 普遍ピボット)"
---

```
Intent DSL
   ↓ parse
Semantic / Component-Theme IR     … 名前付きメンバ (id/class/role/mat_slot/intent_state)
   ↓ resolve (フェーズ評価、ジオメトリ展開、テーマ注入、ブロックステート導出)
block-array IR                    … ボクセル格子 + パレット + block entities + entities [普遍ピボット]
   ↓ serialize (エディション、バージョン、フォーマットごとのバックエンド)
{ .nbt (Java) / .litematic / .schem / .mcstructure (Bedrock) }
```

## 3.1 block-array IR = 普遍ピボット
- すべてのフォーマットのフロントエンド/バックエンドはこの層で出会います。**diff / IoU / シリアライ
  ゼーションはここで起こります**。
- ボクセル格子 + パレット + block entities + entities を保持し、フォーマット・エディション・バージョン
  に対して中立です。
- 順方向の出力先であり、逆方向の入力先です ([ecosystem-interop.ja.md](ecosystem-interop))。

## 3.2 メンバ / Intent IR は豊かで不変条件を運ぶ
- 名前付きメンバは `id` / `class` / `role` / `mat_slot` / `intent_state` / `resolved_state` を保持します
  ([blockstate.ja.md](blockstate))。
- raw な import (schematic 取り込み) は有効な Intent IR を生成しません。意味的リフトを経てはじめて
  Intent IR に到達します。
- 成果物の進捗は `semantic_level: raw | grouped | lifted` で表現します。

## 3.3 レッドストーン論理サブ層 (Logic / Netlist / Placement IR)
レッドストーンを論理的に記述すると ([redstone.ja.md](redstone))、Intent IR と block-array IR の
間に役割の異なる 3 つの IR 層が入ります (HDL では標準的な分離です):
```
Logic IR     論理式 / 依存 DAG (エディション中立、ゼロディレイ)
Netlist IR   セル/ネット (論理セル選択。まだディレイは持たない)
Placement IR セル座標 + 実際のワイヤ長 (ここで初めてディレイ/tick が決まる)
```
論理はエディション中立、place-and-route の結果 (タイル、タイミング) はエディション固有です。要点:
**ディレイは Logic/Netlist では保持しません。Placement IR で初めて決まります**。

## 3.4 二層モデルの帰結
- **下の block-array IR は共有されます** (順方向/逆方向、全フォーマット間で)。
- **その上のメンバ/Intent IR は独立した型** で不変条件を持ちます (すべてのメンバは intent_state を持つ、
  スロットは解決済みである、など)。
- この分離により、シリアライゼーション、diff、lint、評価 (IoU) を下層で共有しつつ、意味層を型安全に
  保つことができます。
