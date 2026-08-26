---
title: "サンプル"
description: 実際の .crn ファイル。言語面だけが画面に残るよう、どれも小さく保っています。
---

すべて [`examples/`](https://github.com/kage1020/Cairn/tree/main/examples) にあり、意図的に最小限で
す。最初のグループの 4 つは [チュートリアル](/ja/tutorial/) が順に辿ります。

> リファレンスコンパイラは未完成なので、これらは今日ビルドできるファイルではなく規範的な例示です。

## まずここから

| ファイル | 示すもの |
|---|---|
| [`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn) | 最小限の実用ビルド。`struct` / `theme` / スロット / 壁セレクタ。 |
| [`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn) | 抽象マテリアルトークン、階ごとの `level`、上書きによる intent 昇格。 |
| [`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn) | 論理レッドストーン。信号バインディング、`circuit` 領域、アサーション。 |
| [`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn) | `site` とトポロジカルな `connect` による複数建築。 |

## 屋根の種類

| ファイル | 示すもの |
|---|---|
| [`roof-shed.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-shed.crn) | `kind=shed slope_to=front`。front の壁へ向かって上がる 1 枚のスロープ。 |
| [`roof-hip.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-hip.crn) | 正方形 footprint の `kind=hip`。4 面が 1 つの頂部に集まる。 |
| [`roof-flat.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-flat.crn) | `kind=flat`。1 層のデッキと、それを壁の外へ張り出させる overhang。 |

## walkway とポート

| ファイル | 示すもの |
|---|---|
| [`l-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/l-walkway.crn) | 両軸で位置の異なる 2 つのポートを結ぶ Manhattan の L 字。 |
| [`at-side-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/at-side-walkway.crn) | `at=left` / `at=right` のドアアンカーが、向かい合う角へポートを寄せる。 |
| [`window-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/window-walkway.crn) | 窓のポート — 矩形の中心に張り付き、地面の段に固定される。 |

## 境界のケース

| ファイル | 示すもの |
|---|---|
| [`edition-fallback.crn`](https://github.com/kage1020/Cairn/blob/main/examples/edition-fallback.crn) | エディション共通のプリミティブが無いスロットに対する、エディション別テーマバリアント。 |
| [`crossbar.crn`](https://github.com/kage1020/Cairn/blob/main/examples/crossbar.crn) | 配線座標を共有する 2 つのネット — 交差パスが報告し、修復しないもの。 |
