# Cairn

> 言語: **日本語** ([English](README.md))
>
> 英語版が source of truth です。日本語版は内容差分が出た場合、英語版を正として読み直してください
> ([CONTRIBUTING.md](CONTRIBUTING.md) を参照)。

**Cairn** は Minecraft の建築物を記述するための言語です。意図 (intent) — 壁、屋根、窓、対称性、テーマ、
さらにはレッドストーン回路まで — を宣言すると、コンパイラがブロックステート、向き、座標計算、信号配線、
エディション・バージョンごとのブロック ID を解決します。

ケルン (cairn) とは「場所を示すために意図的に積み上げられた石」のことです。Minecraft の建築物そのもの、
すなわち意図的に配置されたブロック群と同じ意味です。名前がそのままテーゼになっています。

> ステータス: **設計仕様、ドラフト `2026.06`**。言語仕様はオープンに設計中で、リファレンス実装は
> まだありません。正規仕様・チュートリアル・開発者ガイドは
> [ドキュメントサイト](https://cairn.kage1020.com/ja/) にあります。

## なぜ

Minecraft の NBT/SNBT は AI が直接扱うには非効率で (バイナリ、1ブロック1レコードのフラット列)、人や
AI が建築を考える粒度 (壁・屋根・対称性) とも噛み合っていません。Cairn は **建築的意図を Minecraft の
ボクセル世界と対応付ける中間言語** です。AI が「見て」「手を動かす」ための目と手です。

アプローチは **generation-first (lossy)** です。NBT との完全なラウンドトリップ忠実度は捨て、AI が正確に
建築物を生成・編集できることを最優先にします。可搬な成果物は常に Cairn ソースであり、出力された
NBT/schematic はターゲットに固定されたビルド成果物 (バイナリ相当) です。

## 例

```
@requires version>=1.20

theme medieval:
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  window[class=small] -> frame=@spruce_wood

struct cottage size=9x7
  floor  mat_slot=floor
  walls  class=outer mat_slot=wall height=4
  door   side=front at=center
  window class=small side=front offset=2 y=2 size=2x2 sym=true
  roof   kind=gable mat_slot=roof overhang=1
```

```sh
cairn compile cottage.crn --edition java --target 1.21.4
```

## 核となる考え方

- **ブロックステートではなく意図を宣言する**。階段の向き、ドアの向き、ガラスペインの接続、ベッドの
  頭・足は、コンパイラが導出します。値そのものが意図である場合のみ、ユーザーが上書きします。
- **フェーズ順評価**。コマンドはフラットに順不同で書き、コンパイラが固定のフェーズ
  (massing → envelope → openings → fixtures → redstone → raw) に振り分けます。
- **CSS 的なテーマ**。構造体は `mat_slot` を持ち、`theme` がスロットとセレクタにマテリアルを束ねます。
  「どこ」と「何で」を分離します。
- **Java と Bedrock を 1 ソースから**。エディションはコンパイル時のターゲット軸です。正規マテリアル
  語彙とエディションごとのバックエンドが ID / 状態の差分を吸収します。トランスコードではなく再
  コンパイルです。
- **論理的なレッドストーン**。信号グラフを記述し、コンパイラが実際の dust/repeater/torch をエディション
  ごとに合成・配置・配線します。
- **lint をファーストクラスのループに**。コンパイラは建築 linter でもあります。精度は自己修正ループで
  獲得するもので、ワンショット生成で得るものではありません。
- **エコシステム連携**。`.nbt`, `.litematic`, `.schem`, `.mcstructure` への書き出しと、schematic の忠実な
  低レベル写し取りからの LLM による意図的リフトをサポートします。

## ドキュメント

<https://cairn.kage1020.com/ja/> がプロジェクトの散文ドキュメントの正規ホームです:

- [仕様書](https://cairn.kage1020.com/ja/spec/) — 15 章 + 横断
  [用語集](https://cairn.kage1020.com/ja/spec/glossary/)。
- [チュートリアル](https://cairn.kage1020.com/ja/tutorial/) — [`examples/`](examples/) の
  `.crn` (cottage、themed-tower、redstone-door、village) を順に辿ります。
- [開発者ガイド](https://cairn.kage1020.com/development/) — Rust ワークスペース構造、
  クレート依存グラフ、ビルド/テスト/lint コマンド (英語のみ)。

Markdown ソースは [`website/src/content/docs/`](website/README.md) にあり、コードと同じレビュー
フローで編集します。`main` への push ごとに Cloudflare Pages の Git 連携が自動デプロイします。

## バージョニング

Cairn のリリースは **日付ベースバージョニング (CalVer)** `YYYY.0M[.PATCH]` を採用します (例:
`2026.06`, `2026.06.1`)。これは「言語仕様 + リファレンスコンパイラ + 標準ライブラリ +
レジストリ/制約パック」をひとまとめにしたバンドルのバージョンであり、Minecraft のターゲットバージョン
(`--target`) とは **別軸** です。両者は常にフィールド/フラグ/キーワードで区別され、フォーマットでは
区別しません。詳細は仕様書を参照してください。

バージョン上げで「何を壊してよいか」の契約は
[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/) が規定します: すべての公開面は
**Stable**、**Evolving**、**Internal** のいずれかに属し、`Evolving` の breaking は月次 minor のみ、
`Stable` の breaking は `W_DEPRECATED` で 1 リリースぶんの猶予を経てから入ります。

## ロードマップ

[ロードマップ](https://cairn.kage1020.com/ja/roadmap/) に M1〜M6 のマイルストーンと `2027.06.0`
までの月別スコープを掲載しています:

- **M1** (`2026.07.0`) — ソースが parse できる
- **M2** (`2026.10.0`) — 最小ビルド (単室、Java、lockfile)
- **M3** (`2027.01.0`) — examples が Java で end-to-end 動く
- **M4** (`2027.02.0`) — Java/Bedrock パリティ
- **M5** (`2027.03.0`) — `cairn-lsp` と VS Code 拡張
- **M6** (`2027.05.0`) — レッドストーン論理層、place-and-route、tick simulator

月次 minor は毎月 1 日の GitHub Actions cron が自動で PR を立てます。patch は適格コミットが `main`
に入ったときに随時開きます。

## コントリビュート

Cairn は設計段階です。議論、批判、具体的な提案を歓迎します。詳細は
[CONTRIBUTING.md](CONTRIBUTING.md) と [Code of Conduct](CODE_OF_CONDUCT.md) を参照してください。
仕様書および本プロジェクトドキュメントの正規言語は英語ですので、変更提案はまず英語版に対して
行ってください。

## ライセンス

[Apache License 2.0](LICENSE) © kage1020 および Cairn の著者。
