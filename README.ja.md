# Cairn

> 言語: **日本語** ([English](README.md))

Cairn は Minecraft の建築物を記述するための言語です。「丸石の壁と切妻屋根を持ち、正面に窓が並ぶ 9×7 のコテージ」のように作りたいものを書くと、必要なブロックはコンパイラが割り出します。屋根の階段の向き、開口部の位置、座標の計算、エディションやバージョンごとに異なるブロック ID まで、手で書く必要はありません。

ケルン (cairn) は、場所の目印として石を積み上げたものです。Minecraft の建築物も、積み上げたブロックで場所に印を付けるという点で同じです。

## インストール

```sh
cargo install cairn-lang-cli
```

これで `cairn` コマンドが使えるようになります。

Linux・macOS・Windows (`x86_64` と `aarch64`) 向けのビルド済みアーカイブも、各[リリース](https://github.com/kage1020/Cairn/releases)に添付しています。アーカイブには `cairn` と言語サーバー `cairn-lsp` の両方が入っています。Windows `aarch64` 版を除くすべてのアーカイブに sigstore の署名が付きます (Windows `aarch64` 向けには cosign のバイナリがないためです)。

リポジトリから直接ビルドする場合は `cargo build --release` を実行してください。両方のバイナリが `target/release/` に出力されます。

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

次の 2 つのファイルができます。

- `cottage.nbt`: ストラクチャーブロックで読み込めるバニラの構造ファイル
- `cottage.crn.lock`: ソースのハッシュ、解決したターゲット、ビルドに使ったレジストリパックの記録

同じファイルを Bedrock 向けにもビルドできます。

```sh
cairn compile cottage.crn --edition bedrock --target 1.21.60
```

今度は `cottage.mcstructure` ができます。ソースは 1 文字も変えていません。エディションはコンパイル時に指定するもので、エディションごとに書き方が変わるわけではないからです。階段の角の形のように Bedrock では表現できないものがあれば、黙って捨てずにそのことを報告します。

## ビルドする前に

`cairn check` は、何も書き出さずに同じ解析だけを行います。エディションとターゲットを指定すると、ブロック ID まで検査し、書き間違いには正しいブロック名を示します。

```
$ cairn check cottage.crn --edition java --target 1.21.4
cottage.crn:6:17: error[E_UNKNOWN_ID]: `minecraft:cobblestoen` is not a block in `java 1.21.4`
  note: `java 1.21.4` spells the nearest block `minecraft:cobblestone`
…
```

診断は、何が誤りかと、どんな値なら有効かを示します。ビルドが失敗しても、次に何を書けばよいかがその場で分かります。`--format json` を付けると同じ内容を機械可読な形式で出力するので、書いて、チェックして、直すというループを人の手でもツールからでも回せます。

`cairn info` を使うと、ターゲットを決める前にどのバージョンでビルドできるかを確かめられます。

```
$ cairn info cottage.crn
registry compatibility:  1.20 .. latest
edition portability:     Java: portable: 6  degraded: 0  unsupported: 0   Bedrock: portable: 6  degraded: 0  unsupported: 0
buildable targets:       Java: 1.20.4, 1.21, 1.21.4   Bedrock: 1.21.0, 1.21.40, 1.21.60
intended targets:        (none declared)
semantic-sensitive:      (none)
```

## いま使える機能

- `cairn parse` / `check` / `info` / `lower` / `compile`
- 1 つのソースから Java の `.nbt` と Bedrock の `.mcstructure` を出力
- メンバー: 床、壁、ドアの開口、窓、屋根 (`gable` / `shed` / `hip` / `flat`)、階段、感圧板、`level` による階ごとのまとまり
- スロットを持つテーマと、そのエディション別バリアント
- ソースごとのロックファイルと、バージョン別のブロック ID と改名前の旧名を収めたレジストリパック
- 診断と補完を提供する `cairn-lsp` と [VS Code 拡張](editors/vscode/)
- tree-sitter 文法 (言語サーバーなしでハイライトだけ使いたいエディタ向け)

**実験的な機能。** `cairn synth --experimental-logic-synth` は、`logic` グラフに合成、ネットリスト構築、エディション選択、配置、配線、遅延挿入、交差の合法化を順に施し、各ステージの結果を JSON で出力します。ただし、レッドストーンはまだコンパイル成果物に出力されず、出力の形式も今後変わる可能性があります。

**未実装の機能。**

- テーマのセレクタ束縛。`window[class=small] -> frame=@spruce_wood` は窓にマッチしますが何も塗らず、`cairn check` がそのことを報告します。
- 屋根と階段以外のブロックステートの導出。`door` は開口を切り抜くだけでドアのブロックは置かず、板ガラスやフェンスの接続や水没状態も導出しません。
- `edit` による部分的な変更と、`id=` を書かなかったメンバーにコンパイラが付ける安定したアドレス
- `.litematic` と `.schem` の書き出し、既存の schematic の読み込み
- コンパイル成果物へのレッドストーン出力、`and` / `or` / `not` 以外のゲートと順序マクロ、`assert latency`、回路を検証するティックシミュレータ
- ブラウザで動くプレイグラウンド (`cairn-lang-wasm` クレートは、まだ何も export しないプレースホルダです)

## 基本の考え方

- **ブロックステートではなく意図を書く。** 切妻屋根は、自分の階段がどちらを向き、それぞれがブロックの上半分と下半分のどちらに置かれるかを知っているので、`facing=` を書く必要はありません。ブロックステートはコンパイラが導出します。自分で指定するのは、その値自体が意図であるときだけです。
- **書く順番は問わない。** メンバーはどの順に書いても構いません。コンパイラが固定のフェーズ (massing → envelope → openings → fixtures → redstone → raw) に並べ替えます。
- **テーマで「どこに」と「何で」を分ける。** 構造は `mat_slot` を持ち、テーマがそのスロットに素材を割り当てます。スタイルシートがクラスにスタイルを割り当てるのと同じ関係です。
- **変換せず、再コンパイルする。** 持ち運べる成果物は `.crn` ソースです。構造ファイルは特定のエディションとバージョン向けのビルド出力で、プログラムでいえば実行バイナリにあたります。
- **黙って置き換えない。** 未知のブロック、解決できないスロット、意図を表現できないターゲットは、エラーか名前の付いた劣化 (degradation) として報告します。

## ドキュメント

ドキュメントは英語版の <https://cairn.kage1020.com/> を基準とします。日本語訳は `/ja/<path>/` の下にあります (<https://cairn.kage1020.com/ja/>)。

- [チュートリアル](https://cairn.kage1020.com/ja/tutorial/): インストールから、ワールドに構造ファイルを置くまでの最短ルート。
- [サンプル](examples/): `cottage` / `themed-tower` / `village` / `redstone-door` と、メンバーを 1 つずつ扱う小さなファイル。
- [仕様](https://cairn.kage1020.com/ja/spec/): 規範となるリファレンス。全章に共通の[用語集](https://cairn.kage1020.com/ja/spec/glossary/)もあります。
- [開発者ガイド](https://cairn.kage1020.com/development/): ワークスペースの構成、依存関係のルール、ビルド・テスト・lint のコマンド (英語のみ)。
- [ロードマップ](https://cairn.kage1020.com/ja/roadmap/): 各月の目標。

サイトのソースは [`website/src/content/docs/`](website/README.md) にあり、コードと同じようにレビューを受けます。

同じページはコンパイラにも同梱されていて、言語モデル向けの形式でも配信しています。

- `cairn docs` は、トップページとプレイグラウンドを除くサイトのページを一覧します。表示されるのは、インストールした `cairn` がビルドされた時点の内容です。`cairn docs ja/spec/syntax` で 1 ページを Markdown で表示でき、`cairn docs spec/lint "Error vs warning"` で引用が指す節だけを表示できます。ネットワークは不要で、表示される内容は常にそのリリースが実装している仕様と一致します。
- [`/llms.txt`](https://cairn.kage1020.com/llms.txt) は言語モデル向けのサイト索引です。同じページは、URL の末尾に `index.md` を付けると Markdown で取得できます ([`/ja/spec/syntax/index.md`](https://cairn.kage1020.com/ja/spec/syntax/index.md))。[`/llms-full.txt`](https://cairn.kage1020.com/llms-full.txt) は英語版の全ページを 1 つにまとめたものです。

## バージョニング

リリースには日付ベースのバージョン `YYYY.M[.PATCH]` を使い、言語・コンパイラ・レジストリパックを 1 つの単位としてリリースします。このバージョンは `--target` に渡す Minecraft のバージョンとは別物です。両者は書式ではなく、フラグやキーワードで区別します。

バージョンを上げるときに何を壊してよいかは、[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/)で決めています。すべての公開面は **Stable**・**Evolving**・**Internal** のいずれかに属します。`Evolving` を壊せるのは月次のマイナーリリースだけです。`Stable` を壊すときは、その前に 1 リリースの間 `W_DEPRECATED` 警告を出します。

## コントリビュート

バグ報告、言語の穴を突くサンプル、仕様への批判、コードのどれも歓迎します。[CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) と[行動規範](CODE_OF_CONDUCT.md)をご覧ください。

## ライセンス

[Apache License 2.0](LICENSE) © kage1020 and the Cairn authors.
