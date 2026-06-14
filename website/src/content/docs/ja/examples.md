---
title: "サンプル"
---

実用 Cairn (`.crn`) サンプル。それぞれ [チュートリアル](/ja/tutorial/) から参照され
ており、言語面だけが画面に残るよう意図的に最小化されています。

> リファレンスコンパイラはまだ実装されていないので、このサンプル群は現時点では *規範的な例示* で
> あり、ビルドできるファイルではありません。仕様書とチュートリアルから参照されることで
> `cargo check` 経由で間接的に検査されます。

| ファイル | 例示する内容 |
|---|---|
| [`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn) | 最小限の実用ビルド: `struct` + `theme` + スロット + セレクタ。 |
| [`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn) | 抽象マテリアルトークン、フロア別レベル、上書きによる昇格。 |
| [`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn) | 論理的レッドストーン: 信号バインディング、`circuit` 領域、アサーション。 |
| [`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn) | `site` とトポロジカルな `connect` による複数建築。 |

順に追うときは、まずチュートリアルを読んでください。各ファイルを上から順に辿り、すべての行の背後に
ある仕様章を指します。
