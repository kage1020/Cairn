---
title: "サンプル"
description: 実際の .crn ファイル集。言語の書き方だけに集中できるよう、どれも小さくまとめています。
---

どのファイルも [`examples/`](https://github.com/kage1020/Cairn/tree/main/examples) にあり、あえて最小限の内容にしています。最初のグループの 4 つは、[チュートリアル](/ja/tutorial/)で順番に解説しています。

> どのファイルも、現在のコンパイラでコンパイルできます。ただし、レッドストーンはまだコンパイル結果に含まれないので、`redstone-door.crn` は回路なしで作られます。

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

## 境界のケース

| ファイル | 内容 |
|---|---|
| [`edition-fallback.crn`](https://github.com/kage1020/Cairn/blob/main/examples/edition-fallback.crn) | 両エディション共通のプリミティブがないスロットに、エディション別のテーマバリアントで対応する。 |
| [`crossbar.crn`](https://github.com/kage1020/Cairn/blob/main/examples/crossbar.crn) | 2 つのネットが同じ配線座標を共有する例。交差パスが報告はするが修復はしないケース。 |
