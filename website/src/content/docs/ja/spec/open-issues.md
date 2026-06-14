---
title: "15. 未決事項"
---

## 15.1 実装時に決める設計選択
- **provenance の保管場所**: `.crn` ヘッダか、ロックか。暫定方針は「`.crn` は `@intended_targets`
  (ヒント) のみを持ち、`verified` などはコンパイラがロックに書き込む」
  ([versioning-editions.ja.md](versioning-editions))。
- **逆方向 IR の型**: 退化可能な単一 IR にするか、Intent IR と Raw Geometry IR を別型にするか。暫定
  方針は「block-array 層は共有、その上のメンバ層で型を分ける」
  ([architecture.ja.md](architecture))。
- **レガシー `.schematic` (1.13 以前)**: v1 では未対応。後日オプションとして数値 ID マッピングを検討
  する余地はあります。

## 15.2 まだ手を付けていないトピック
- **座標系**: 角原点 + front=+z を固定するか、中心原点 / 入口相対の向き / フロアごとのローカル y=0
  (`level id=floor2 y=4`) を導入するか。
- **プリミティブの昇格**: 寄棟/陸屋根/ピラミッド屋根、column、arch、repeat などを意味プリミティブに
  昇格するか。判断は評価フレームワークの実験データに基づきます ([evaluation.ja.md](evaluation))。
- **室内**: `inside.front` 接頭辞で十分か、上位概念 `room` を導入するか。家具を `def` ライブラリで
  賄えるか。

## 15.3 言語進化のポリシー (日付ベースバージョニング)
Cairn 自身の進化で破壊的変更をどう扱うかは未確定です。Rust 風の「edition」機構 (年単位の opt-in) を
入れるか、CHANGELOG でリリースごとに告知するか。「edition」という語は Java/Bedrock で既に使われて
いるので別の用語が必要になります。当面は後者 (CalVer + `@cairn` provenance) で十分です。
