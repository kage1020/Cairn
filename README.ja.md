# Cairn

> 言語: **日本語** ([English](README.md))

Cairn は Minecraft の建築物を記述するための言語です。作りたいものを書けば — 丸石の壁、切妻屋根、
正面に並んだ窓を持つ 9×7 のコテージ — コンパイラがブロックを組み立てます。屋根の階段が向く方向、
開口部の位置、座標計算、そしてエディションとバージョンごとに異なるブロック ID まで。

ケルン (cairn) とは、場所を示すために積み上げられた石のことです。Minecraft の建築物も同じです。

## インストール

```sh
cargo install cairn-lang-cli
```

これで `cairn` コマンドが入ります。Linux・macOS・Windows (`x86_64` と `aarch64`) 向けの署名済み
ビルド済みアーカイブは各[リリース](https://github.com/kage1020/Cairn/releases)に添付されており、
`cairn` と言語サーバー `cairn-lsp` の両方が含まれています。チェックアウトから直接ビルドする場合は
`cargo build --release` で両方が `target/release/` に出力されます。

## はじめかた

`cottage.crn` を書きます。

```
@cairn 2026.09
@requires version>=1.20

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  slot glass -> @glass_pane
  window[class=small] -> frame=@spruce_wood

struct cottage size=9x7
  floor  mat_slot=floor
  walls  class=outer mat_slot=wall height=4
  door   side=front at=center
  window class=small side=front offset=2 y=2 size=2x2 sym=true mat_slot=glass
  roof   kind=gable mat_slot=roof overhang=1
```

コンパイルします。

```sh
cairn compile cottage.crn --edition java --target 1.21.4
```

ストラクチャーブロックで読み込めるバニラの構造ファイル `cottage.nbt` と、ソースのハッシュ・
解決されたターゲット・使用したレジストリパックを記録した `cottage.crn.lock` が出力されます。

同じファイルが Bedrock でもビルドできます。

```sh
cairn compile cottage.crn --edition bedrock --target 1.21.60
```

今度は `cottage.mcstructure` が出力されます。ソースは 1 文字も変えていません。エディションは
フラグであって方言ではないからです。Bedrock が本当に表現できないもの (たとえば階段の角の形状)
については、黙って落とすのではなくその旨を報告します。

## ビルドする前に

`cairn check` は何も書き出さずに同じ解析だけを実行します。ブロック名を打ち間違えれば、意図していた
であろうブロックを名指しして指摘します。

```
cottage.crn:6:17: error[E_UNKNOWN_ID]: `minecraft:cobblestoen` is not a block in `java 1.21.4`
  note: `java 1.21.4` spells the nearest block `minecraft:cobblestone`
```

診断は「何が誤りか」と「妥当な値は何か」の両方を示すので、ビルドが失敗したときに次に何を打てば
よいかがそのまま分かります。`--format json` を付ければ同じ内容が機械可読な形で出てくるので、
書く → チェックする → 直す のループが実用的に回せます。手作業でも、ツール経由でも同じです。

`cairn info` は、ターゲットを決める前にバージョンの問いに答えます。

```
$ cairn info cottage.crn
registry compatibility:  1.20 .. latest
edition portability:     Java: portable: 6  degraded: 0  unsupported: 0   Bedrock: portable: 6  degraded: 0  unsupported: 0
buildable targets:       Java: 1.20.4, 1.21, 1.21.4   Bedrock: 1.21.0, 1.21.40, 1.21.60
intended targets:        (none declared)
semantic-sensitive:      (none)
```

## 現在動くもの

- `cairn parse` / `check` / `info` / `lower` / `compile`
- 単一のソースから Java `.nbt` と Bedrock `.mcstructure` を出力
- メンバー: 床、壁、ドア、窓、屋根 (`gable` / `shed` / `hip` / `flat`)、階段、感圧板、`level` による
  階層のグルーピング
- スロットとセレクタを持つテーマ、およびそのエディション別バリアント
- ソースごとのロックファイルと、バージョン別ブロック ID・リネームエイリアスを収めたレジストリパック
- 診断と補完を提供する `cairn-lsp`、および [VS Code 拡張](editors/vscode/)
- tree-sitter 文法 (言語サーバーなしでハイライトだけ欲しいエディタ向け)

**実験的機能。** `cairn synth --experimental-logic-synth` は `logic` グラフを合成・ネットリスト
構築・エディション選択・配置・配線・遅延挿入・交差の合法化まで通し、各ステージを JSON で出力
します。レッドストーンはまだコンパイル成果物には届いておらず、出力の形も変わりうるものです。

**まだないもの。** `.litematic` と `.schem` の書き出し、既存 schematic の読み込み、コンパイル
成果物へのレッドストーン出力とそれを検証するティックシミュレータ、そしてブラウザ上の
プレイグラウンド (`cairn-lang-wasm` クレートは export を持たないプレースホルダのままです)。

## 核となる考え方

- **ブロックステートではなく意図を書く。** 切妻屋根は、自分の階段がどちらを向き、それぞれが上下
  どちらの半分に座るかを知っています。`facing=` を書く必要はありません。ブロックステートは導出
  されるもので、上書きするのは、その値自体が意図であるときだけです。
- **順序は問わない。** メンバーはどの順に書いても構いません。コンパイラが固定のフェーズ
  (マッシング → 外皮 → 開口部 → 什器 → レッドストーン → raw) に並べ替えます。
- **テーマが「どこ」と「なに」を分ける。** 構造は `mat_slot` を持ち、テーマがそのスロットを素材に
  束ねます。スタイルシートがクラスに束ねるのと同じ関係です。
- **変換ではなく再コンパイル。** 可搬な成果物は `.crn` ソースです。構造ファイルは特定の
  エディションとバージョンに固定されたビルド出力で、バイナリと同じ位置づけです。
- **黙って代替しない。** 未知のブロック、解決できないスロット、意図を表現できないターゲットは
  エラーか、名前の付いた劣化 (degradation) として報告されます。

## ドキュメント

<https://cairn.kage1020.com/ja/> がプロジェクトの文章の正規の置き場所です。英語版は
`/<path>/`、日本語版は `/ja/<path>/` に対応しています。

- [チュートリアル](https://cairn.kage1020.com/ja/tutorial/) — インストールからワールドに構造
  ファイルを置くまでの最短経路。
- [サンプル](examples/) — `cottage` / `themed-tower` / `village` / `redstone-door` と、メンバーを
  ひとつずつ扱う小さなファイル群。
- [仕様](https://cairn.kage1020.com/ja/spec/) — 規範的なリファレンス。全体を横断する
  [用語集](https://cairn.kage1020.com/ja/spec/glossary/)付き。
- [開発者ガイド](https://cairn.kage1020.com/development/) — ワークスペース構成、依存関係のルール、
  ビルド・テスト・lint コマンド (英語のみ)。
- [ロードマップ](https://cairn.kage1020.com/ja/roadmap/) — 各月が目指しているもの。

サイトのソースは [`website/src/content/docs/`](website/README.md) にあり、コードと同じレビューを
通ります。

## バージョニング

リリースは日付ベースのバージョニング `YYYY.M[.PATCH]` を使い、言語・コンパイラ・レジストリパックを
ひとつの束として扱います。これは `--target` に渡す Minecraft のバージョンとは
別の軸で、両者はフラグやキーワードで区別され、書式で区別されることはありません。

バージョンを上げるときに何を壊してよいかは
[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/)が定めます。すべての表面が
**Stable** / **Evolving** / **Internal** のいずれかに属し、`Evolving` を壊せるのは月次マイナー
だけ、`Stable` を壊す前には 1 リリース分の `W_DEPRECATED` 警告を挟みます。

## コントリビュート

バグ報告、隙を突くサンプル、仕様への批判、そしてコード。どれも歓迎します。
[CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) と[行動規範](CODE_OF_CONDUCT.md)をご覧ください。

## ライセンス

[Apache License 2.0](LICENSE) © kage1020 and the Cairn authors.
