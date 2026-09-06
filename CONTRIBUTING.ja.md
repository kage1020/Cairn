# Cairn へのコントリビュート

> 言語: **日本語** ([English](CONTRIBUTING.md))

Cairn は、規範的な[仕様](https://cairn.kage1020.com/ja/spec/)を背後に持つ、動くコンパイラです。
どちらもまだ動いている最中で、どちらもコントリビュートを受け付けています。

## 手伝えること

- **バグ報告。** `.crn` ファイル、実行したコマンド、期待した結果。クラッシュするファイルと同じくらい、
  間違ったブロックにコンパイルされるファイルにも価値があります。
- **サンプル。** 実際の建築を書いて、言語が扱いにくかった・曖昧だった・力尽きた箇所を教えてください。
  語彙を育てるのはこれです。メンバーをひとつずつ扱う小さなファイルも、大きなものと並べて
  [`examples/`](examples/) に置いています。
- **コード。** パーサ、lowering、バックエンド、レッドストーン、LSP、周辺ツール。
  [ロードマップ](https://cairn.kage1020.com/ja/roadmap/)から選んでも、自分の困りごとから始めても構いません。
- **仕様の修正。** 誤りの修正、記述の明確化、例の改善。各章は単体で読めるように保ち、相対リンクで
  相互参照してください。
- **設計への批判。** 決定に異議を唱える、抜けているケースを指摘する、代案を出す。該当する章と節を
  指した issue を立ててください。
- **先行事例。** レッドストーンコンパイラ、schematic フォーマット、ボクセル/CAD の place-and-route、
  HDL 合成 — 設計議論での情報提供は歓迎します。

仕様とドキュメントの正典は英語です。翻訳は、二次的なコピーであることが明示されている限り歓迎します。

## 開発環境

[`rust-toolchain.toml`](rust-toolchain.toml) が正確なコンパイラを固定しており、`rustup` がそれを自動で
拾います。チェックアウトして `cargo build` すれば、CI と同じツールチェーンになります。

```sh
cargo build --workspace
cargo test --workspace
cargo run -p cairn-lang-cli -- check examples/cottage.crn --edition java --target 1.21.4
```

`check` は何も書き出しません。`compile` はソースの隣に構造ファイルとロックファイルを書くので、
`examples/` にビルド成果物を残したくなければ `--out` と `--lock` をツリーの外に向けてください。

PR を出す前に、CI と同じものを回してください。Linux・macOS・Windows で走るのはこの 3 コマンドで、
いずれも `RUSTFLAGS=-D warnings` の下で実行されます。

```sh
export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

### リポジトリの構成

| パス | 中身 |
|---|---|
| `crates/cairn-lang-core` | 字句解析、構文解析、Intent IR、check パス、解決、ブロック配列への lowering |
| `crates/cairn-lang-nbt` | NBT コーデック — Java はビッグエンディアン、Bedrock はリトルエンディアン |
| `crates/cairn-lang-formats` | 構造ファイルの書き出し (`.nbt` / `.mcstructure`)、レジストリパック、可搬性 |
| `crates/cairn-lang-redstone` | Logic IR、ネットリスト、配置、配線、遅延、交差の合法化 |
| `crates/cairn-lang-cli` | `cairn` バイナリ |
| `crates/cairn-lang-lsp` | 言語サーバー `cairn-lsp` |
| `crates/cairn-lang-tree-sitter` | tree-sitter 文法。npm 向けには `tree-sitter-cairn` として梱包 |
| `crates/cairn-lang-wasm` | 将来のプレイグラウンド向けブラウザバインディング。現状はプレースホルダ |
| `editors/vscode` | VS Code 拡張 — `cairn-lsp` の薄いクライアント |
| `website` | Astro + Starlight のドキュメントサイト。仕様・チュートリアル・開発者ガイドを英語と日本語で |
| `examples` | `.crn` ソース (ロックファイルは生成物で、追跡していません) |

クレート間の依存ルールは[開発者ガイド](https://cairn.kage1020.com/development/) (英語のみ) が詳しく
扱っています。

## 書き方

原則は、**How** はコードに、**What** はコミットメッセージに、最小限の **Why** をコメントに。

- 仕様が正典です。同義語を作らず、仕様の用語 (`intent_state` / `resolved_state`、`mat_slot`、
  canonical token) をそのまま使ってください。新しい用語は、それが属する章で導入します。
- 設計原則は [Design Principles](https://cairn.kage1020.com/ja/spec/principles/) の `P1`–`P5` として
  参照します。
- エラーメッセージは「何が誤りか / 何なら妥当か / 修正案」の形を取ります。この形が「書く →
  チェックする → 直す」のループを成立させているので、一文増えるだけの価値があります。
- 例は具体的かつ最小限に。

### セッション固有の参照を残さない

Rust のソース、仕様本文、サンプル、ドキュメントは、何年か後に単体で読めなければなりません。issue や
PR の番号、`M3-PR4` のような座標、裸のマイルストーンラベル、「後続の PR で対応」といった記述は
入れないでください。座標は、それが代弁していた事実に置き換えます。

> 変更前: `// M3-PR4 only exposes ports on door members (window / stair / roof ports land later).`
>
> 変更後: `// Ports are currently exposed only on door members. Window / stair / roof ports are reserved for a future extension.`

先送りされていたものが実装されたら、それを入れる PR の中で同時にコメントを更新します。

例外は 3 つだけです。マイルストーンの語彙こそがその目的である面 — [CHANGELOG.md](CHANGELOG.md)、
[ロードマップ](https://cairn.kage1020.com/ja/roadmap/)、
[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/)表のマイルストーン列。
それ以外はレビュー時に次で確認できます。

```sh
rg '\bM[1-6]\b|M[0-9]-PR[0-9]+|pre-M[0-9]|\blater PR\b|\bfuture PR\b' \
  --glob '!CHANGELOG*' --glob '!CONTRIBUTING*' \
  --glob '!**/compatibility.md' --glob '!**/roadmap.md' --glob '!target/**'
```

結果が空であることが約束事です。

## ブランチとプルリクエスト

トランクは `canary` です。`main` はリリース済みの状態で、各リリース直後に自動で fast-forward される
ため、その履歴は公開リリースの一覧そのものになります。

| ブランチ | 役割 |
|---|---|
| `canary` | 機能・修正・ドキュメントのすべてがここに入る。保護対象 |
| `main` | リリースパイプラインだけが更新する。直接 push も PR も不可 |
| `<type>/<short-kebab>` | 変更ひとつ分の作業ブランチ。`canary` を対象にし、マージ後に削除 |
| `release-plz-*` | 月次マイナーとパッチのために自動で開かれる |

ブランチ名は、その作業が最終的に載る Conventional Commits の type に合わせます
(`feat/parser-lexer`、`fix/wall-corner-shape`、`docs/roadmap-2027`)。

**PR タイトルは [Conventional Commits](https://www.conventionalcommits.org/) の 1 行でなければ
なりません。** squash merge が唯一のマージ方式なので、このタイトルがそのまま `canary` 上のコミットに
なり、`release-plz` がパッチリリースの要否を判断するために読むのもこれです。ブランチ上の個々の
コミットは自由形式で構いません。破壊的変更には `!` を付け (`feat(core)!: replace lexer`)、スコープには
対象のクレートか仕様領域を書きます (`feat(core)`、`fix(nbt)`、`docs(spec)`、`build(deps)`)。

| Type | 使う場面 | パッチリリースを切るか |
|---|---|---|
| `feat` | 新機能、公開 API、サブコマンドの追加 | はい |
| `fix` | 挙動を仕様に合わせ直す修正 | はい |
| `perf` | 性能改善 | はい |
| `refactor` | 挙動を変えない内部の再構成 | はい |
| `build` | ビルドシステム、パッケージング、Cargo の依存 | はい |
| `docs` | ドキュメント、仕様本文、README、サンプル | いいえ |
| `test` | テストコードのみ | いいえ |
| `ci` | ワークフロー、release-plz、`rust-toolchain.toml` | いいえ |
| `chore` | 利用者に届かないその他すべて | いいえ |
| `style` | 整形・lint だけの変更 | いいえ |

すべての PR は `canary` を対象にします。`main` に対する PR は受け付けません。メンテナ 1 名の承認と
CI のグリーンが必須です。リリース PR も同じルールで、これをマージすると公開が走り、`main` が
fast-forward されます。

## 決着した決定を蒸し返す

いくつかの決定は意図的に閉じています。位置引数ではなく `key=value`、フェーズ順の評価、
変換ではなく再コンパイル、黙って代替せず大きく失敗する、など。これを再び開くには、その決定と仕様上の
在り処を示し、それが扱えない具体的なケースを挙げ、構文・IR・メッセージの例を伴う代案を出し、
[評価指標](https://cairn.kage1020.com/ja/spec/evaluation/)への影響に触れた issue を立ててください。

## ツールチェーンの更新

固定は意図的にチャンネル追従ではありません。`stable` にしていると、Rust のリリースひとつで開いている
すべてのブランチが同時に赤くなります。しかもその指摘は、そのブランチが触ってもいないファイルに出ます。
固定は新しい lint を避けるためのものではなく、それが「誰かが意図して開いた PR」として届くように
するためのものです。

`channel` を変え、上の CI 3 コマンドを回し (新しいコンパイラは clippy の lint だけでなく *rustc* の
警告も出します)、見つかったものを同じ PR で直し、コミットの type は `ci` にします。「新しい
コンパイラと、それが要求した修正だけ」という差分は、レビュアーが実際に読める差分です。

固定は MSRV ではありません。ワークスペースマニフェストの `rust-version` は、利用者が Cairn を
ビルドするのに必要な下限であり、コードが本当に新しいコンパイラを必要とし始めたときにだけ動かします。
CI はその下限ではビルドしないので、安定化されたばかりの API に手を伸ばした変更は固定側では緑のまま、
宣言した下限にいる利用者だけが壊れます。それを捕まえるのが `cargo +<下限> check --workspace` で、
新しい API に触れたときには回す価値があります。

## バージョニング

日付ベースの `YYYY.M[.PATCH]` です。主要な変更は [CHANGELOG.md](CHANGELOG.md) に記録します。
バージョンを上げるときに何を壊してよいかは番号そのものではなく、
[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/)が定めます。

## 行動規範

このプロジェクトは [Contributor Covenant](CODE_OF_CONDUCT.md) に従います。参加する時点で、これを
守ることに同意したものとみなされます。
