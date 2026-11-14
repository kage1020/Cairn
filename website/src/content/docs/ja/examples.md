---
title: "サンプル"
description: 実際の .crn ファイル集。言語の書き方だけに集中できるよう、どれも小さくまとめています。
---

どのファイルも [`examples/`](https://github.com/kage1020/Cairn/tree/main/examples) にあり、あえて最小限の内容にしています。最初のグループの 4 つは、[チュートリアル](/ja/tutorial/)で順番に解説しています。

> どのファイルも、現在のコンパイラでコンパイルできます。`logic` を持つファイルは、床下に回路が組み上がります。

## まずはここから

| ファイル | 内容 |
|---|---|
| [`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn) | 実用的な最小のビルド。`struct`、`theme`、スロット。 |
| [`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn) | 抽象マテリアルトークン、階ごとの `level`、上書きによる意図への昇格。 |
| [`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn) | 論理で書くレッドストーン。信号の割り当て、`circuit` 領域、アサーション。 |
| [`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn) | `site` と位置関係で結ぶ `connect` による、複数の建物の配置。 |

## 屋根の種類

| ファイル | 内容 |
|---|---|
| [`roof-shed.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-shed.crn) | `kind=shed slope_to=front`。正面の壁に向かって高くなる片流れの屋根。 |
| [`roof-hip.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-hip.crn) | 正方形の敷地に `kind=hip`。4 つの斜面が 1 つの頂部に集まる寄棟屋根。 |
| [`roof-flat.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-flat.crn) | `kind=flat`。1 層の平らな屋根と、それを壁の外まで広げる軒の出 (overhang)。 |

## 通路とポート

| ファイル | 内容 |
|---|---|
| [`l-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/l-walkway.crn) | 縦横どちらの位置も異なる 2 つのポートを、L 字の直交経路で結ぶ。 |
| [`at-side-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/at-side-walkway.crn) | ドアを `at=left` / `at=right` に置くと、ポートが向き合う角の側に寄る。 |
| [`window-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/window-walkway.crn) | 窓のポート。窓の矩形の中心を基準にし、地面の高さに固定される。 |

## ブロックステート

| ファイル | 内容 |
|---|---|
| [`storeroom.crn`](https://github.com/kage1020/Cairn/blob/main/examples/storeroom.crn) | 壁を背にした `chest`。どのチェストも背にした壁から離れる向きを向き、並んだ 2 つは連結し、`size=single` で 2 つを別々に保つ。 |
| [`workshop.crn`](https://github.com/kage1020/Cairn/blob/main/examples/workshop.crn) | `beam`、`trapdoor`、`note_block`、`bed`、`sign`。ドアと窓の上と左右の壁の上端に渡した原木、壁に沿って開いたトラップドアと座標の 3 つ組の位置で上半分に置いたトラップドア、行に書いた音と楽器を持つ音符ブロック、頭を奥の壁に付けた茶色のベッド、ドアの横に掛けた、行に書いた文字を持つ看板。 |
| [`control-room.crn`](https://github.com/kage1020/Cairn/blob/main/examples/control-room.crn) | 壁のそばに置いたすべてのセンサーとアクチュエーター。壁から離れる向きに掛けたレバーとボタン、日照センサー、上を向けたオブザーバーと粘着ピストン、ランプ、外を向いたディスペンサー、パッチで結び付けたピストン。 |
| [`aquarium.crn`](https://github.com/kage1020/Cairn/blob/main/examples/aquarium.crn) | `fill`。マテリアルで砂の床を塗り、水槽に置いたものの周りに水を注ぐ。仕切りの板ガラスと 1 つのチェストは水を含み、`waterlogged=false` のもう 1 つは乾いたまま残る。 |

## エンティティ

| ファイル | 内容 |
|---|---|
| [`trading-post.crn`](https://github.com/kage1020/Cairn/blob/main/examples/trading-post.crn) | 奥の壁のそばに立ち、2 つの取引を持つ司書の村人、左右の壁に掛けた絵画と額縁、座標の 3 つ組の位置に立てた防具立て、ドアの上の `hologram`。hologram は、エディションごとのテーマがそれぞれのやり方で組み上げ、Java ではテキストの display、Bedrock では光る看板になる。 |

## 境界のケース

| ファイル | 内容 |
|---|---|
| [`edition-fallback.crn`](https://github.com/kage1020/Cairn/blob/main/examples/edition-fallback.crn) | 両エディション共通のプリミティブがないスロットに、エディション別のテーマバリアントで対応する。エディションごとに綴りの違うブロックは `@edition` ガードで分ける。 |
| [`crossbar.crn`](https://github.com/kage1020/Cairn/blob/main/examples/crossbar.crn) | 2 枚の感圧板が両方のゲートに入り、ゲートの出力が両端のドアを動かすので、2 本の配線が交差する例。`void=4` では一方が 1 層上でもう一方を越え、`void=3` ではルーターが `E_ROUTE_CONGESTION` で拒否します。 |
