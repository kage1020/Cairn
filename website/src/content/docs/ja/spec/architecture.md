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

順方向の変換ではこの層が出力先になり、逆方向の変換はこの層を読みます。取り込みもこの層で検証します。取り込みが書き出すソースは、ファイルが持つ block array へ lowering されなければなりません ([エコシステム連携](/ja/spec/ecosystem-interop/))。

## 3.2 Intent IR は情報が豊富で、不変条件を持つ

名前付きのメンバーは、`id`、`class`、`role`、アドレス ([構文 §5.5](/ja/spec/syntax/#55-idクラスアドレス))、`mat_slot`、`intent_state`、`resolved_state` を持ちます ([ブロックステート](/ja/spec/blockstate/))。`resolved_state` は lowering が導くので、lowering が読む Intent IR ではなく、メンバーを lowering した先の block-array IR に、構造ごとのキーで記録します。ソースの `edit` の行は、どのパスが読むよりも先に Intent IR に適用するので、その後のパスはどれも編集したメンバーを見ます ([コンポーネント・編集・複数建築 §9.2](/ja/spec/components-editing-sites/#92-編集モデル))。

取り込みは独自の IR を加えません。`cairn import` が書き出すのはソース、つまり Intent IR のエスケープハッチである生のメンバーからなる `.crn` で、そのソースはほかのソースと同じようにパースされ、ファイルが持っていた block array へ lowering されます ([エコシステム連携 §12.2](/ja/spec/ecosystem-interop/#122-逆方向の変換-コンパイラは写し取りllm-がリフトする))。生のメンバーはブロックを名指し、`mat_slot` を読まないので、書き出したものは `semantic_level: grouped`、つまり L1 にあたります。リフトはそのソースを `lifted`、つまり上の不変条件が述べるメンバーへとリファクタリングします。

## 3.3 レッドストーンのサブ層

レッドストーンを論理で記述すると ([レッドストーン](/ja/spec/redstone/))、Intent IR と block-array IR の間に、役割の異なる 4 つの IR 層が入ります。HDL と同じ分け方です。

```
Logic IR            論理式と依存関係の DAG。エディションに依存せず、tick を持たない
Netlist IR          セルとネット。論理セルを選ぶ。まだ遅延はない
Edition Netlist IR  同じセルとネット。エディションのセルを選ぶ。まだ配線はない
Placement IR        セルの座標、続いて実際の配線長。ここで遅延と tick が決まる
```

論理はエディションに依存しませんが、エディションのセルと配置配線の結果 (タイルとタイミング) はエディションごとに異なります。**遅延は Logic IR にも Netlist IR にも持たせません。** 遅延が決まるのは Placement IR です。エディションのセルで各ポートからの tick 数は決まりますが、配線するまでは配線そのものがないからです。Placement IR を block-array IR に落とすパスは、まだありません ([レッドストーン §14.8](/ja/spec/redstone/#148-ir-とフェーズの関係))。

## 3.4 この分け方で得られるもの

最下層の block-array IR は、順方向と逆方向の両方で、すべてのフォーマットに共有されます。その上にあるメンバーの層 (Intent IR) は、不変条件を持つ独立した型です。たとえば、すべてのメンバーが `intent_state` を持つこと、すべてのスロットが解決済みであることが保証されます。

このように分けることで、シリアライズ、diff、lint、IoU による評価を下層で共有しながら、意味の層を型安全に保てます。
