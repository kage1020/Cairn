# Cairn

> 言語: **日本語** ([English](README.md))

Cairn は Minecraft の建築物を記述するための言語です。「丸石の壁と切妻屋根を持ち、正面に窓が並ぶ 9×7 のコテージ」のように作りたいものを書くと、必要なブロックはコンパイラが割り出します。屋根の階段の向き、開口部の位置、座標の計算、エディションやバージョンごとに異なるブロック ID まで、手で書く必要はありません。

ケルン (cairn) は、場所の目印として石を積み上げたものです。Minecraft の建築物も、積み上げたブロックで場所に印を付けるという点で同じです。

## インストール

```sh
cargo install cairn-lang-cli
```

これで `cairn` コマンドが使えるようになります。

Linux・macOS・Windows (`x86_64` と `aarch64`) 向けのビルド済みアーカイブも、各[リリース](https://github.com/kage1020/Cairn/releases)に添付しています。アーカイブには `cairn` と言語サーバー `cairn-lsp` の両方が入っています。Windows `aarch64` 版を除くすべてのアーカイブに sigstore の署名が付きます (Windows `aarch64` 向けには cosign のバイナリがないためです)。2026.12.0 からは [VS Code 拡張](editors/vscode/)も `cairn-lang-<version>.vsix` としてリリースに添付し、同じファイルを Visual Studio Marketplace と Open VSX に公開します。

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

`cairn info` を使うと、ターゲットを決める前にどのバージョンでビルドできるかを確かめられます。最後のコンパイルが検証したターゲットも、ロックファイルから読んで表示します。

```
$ cairn info cottage.crn
registry compatibility:  1.20 .. latest
inferred range:          Java: 0.0 .. latest   Bedrock: 0.0 .. latest
edition portability:     Java: portable: 6  degraded: 0  unsupported: 0   Bedrock: portable: 6  degraded: 0  unsupported: 0
respelled:               Java: none   Bedrock: none
buildable targets:       Java: 1.20.4, 1.21, 1.21.4, 26.3   Bedrock: 1.21.0, 1.21.40, 1.21.60, 1.26.60
intended targets:        (none declared)
semantic-sensitive:      (none)
verified target:         bedrock 1.21.60/block version 18168865, source unchanged, written by Cairn 2026.11.0
```

## いま使える機能

- `cairn parse` / `check` / `info` / `lower` / `compile`
- 1 つのソースから Java の `.nbt`、Litematica の `.litematic`、Sponge Schematic の `.schem` のいずれかと、Bedrock の `.mcstructure` を出力
- `cairn import`。この 4 つのどのファイルも、同じブロックへコンパイルし直せる生のメンバーの `.crn` として読み戻します。復元するのはブロックとその規則性で、意図ではないことを `W_SEMANTIC_LOSS` で示します。
- `cairn diff-blocks`。構造ファイルを、`.crn` がそのファイルのターゲットで組むものと比べ、block IoU、食い違う領域とその考えられる原因、それを埋める `edit` の行を示します。リフトが収束するまで繰り返せます。
- メンバー: 床、壁、ドア、窓、屋根 (`gable` / `shed` / `hip` / `flat`)、階段、チェスト、レッドストーンのセンサーとアクチュエーター (感圧板、レバー、ボタン、日照センサー、オブザーバー、ランプ、ピストン、ディスペンサー)、手で置くレッドストーンの部品 (`repeater` / `comparator` / `redstone_torch` / `redstone_dust`)、マテリアルや水を満たす `fill`、ブロック ID を直接名指す生のブロック (`raw_block` / `raw_fill` / `raw_repeat`)、`level` による階ごとのまとまり
- スロットを持つテーマと、そのエディション別バリアント
- `edit` によるパッチ。セレクタやアドレスが選ぶメンバーを、その行を書き換えずに変えます
- ソースごとのロックファイルと、バージョン別のブロック ID、改名前の旧名、意味トークンを収めたレジストリパック。正規トークンはターゲットごとの綴りで書き出されます
- 論理として書くレッドストーン。`cairn compile` は各 struct の回路を床下に実寸で組み上げてセンサーとアクチュエーターにつなぎ、`assert truth`、`latency`、`always` の各行をエディションごとにその回路で検査します。
- 診断と補完、`cairn info` と `cairn diff-blocks` のレポートを提供する `cairn-lsp` と [VS Code 拡張](editors/vscode/)
- tree-sitter 文法 (言語サーバーなしでハイライトだけ使いたいエディタ向け)

**実験的な機能。** `cairn synth --experimental-logic-synth` は、レッドストーンのパイプラインの各ステージ (合成、ネットリスト構築、エディション選択、配置、配線、遅延挿入、合法化) を JSON で出力します。出力の形式は今後変わる可能性があります。

**未実装の機能。**

- テーマのセレクタ束縛。`window[class=small] -> frame=@spruce_wood` は窓にマッチしますが何も塗らず、`cairn check` がそのことを報告します。
- 流水 (`flow=` と `level=` を取る `water` キーワード)
- 取り込んだものを壁や窓やテーマへ引き上げるリフト。逆方向の変換はこれを LLM に任せ、`cairn diff-blocks` が確かめます
- ブラウザで動くプレイグラウンド (`cairn-lang-wasm` クレートは、まだ何も export しないプレースホルダです)

## 基本の考え方

- **ブロックステートではなく意図を書く。** 切妻屋根は、自分の階段がどちらを向き、それぞれがブロックの上半分と下半分のどちらに置かれるかを知っているので、`facing=` を書く必要はありません。ブロックステートはコンパイラが導出します。自分で指定するのは、その値自体が意図であるときだけです。
- **書く順番は問わない。** メンバーはどの順に書いても構いません。コンパイラが固定のフェーズ (massing → envelope → openings → fixtures → fluids → redstone → raw) に並べ替えます。
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
