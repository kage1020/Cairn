# Cairn へのコントリビュート

> 言語: **日本語** ([English](CONTRIBUTING.md))

Cairn は実際に動くコンパイラで、その挙動は規範となる[仕様](https://cairn.kage1020.com/ja/spec/)で定めています。コンパイラも仕様もまだ発展途上で、どちらへのコントリビュートも歓迎します。

## 手伝えること

- **バグ報告。** `.crn` ファイル、実行したコマンド、期待した結果を添えてください。間違ったブロックにコンパイルされるファイルは、クラッシュするファイルと同じくらい役に立ちます。
- **サンプル。** 実際の建築を書いてみて、書きにくかった箇所、曖昧だった箇所、言語では表現しきれなかった箇所を教えてください。語彙を育てるのは、こうしたフィードバックです。メンバーを 1 つずつ試す小さなファイルも、大きなサンプルと一緒に [`examples/`](examples/) に置いています。
- **コード。** パーサ、lowering、バックエンド、レッドストーン、LSP、周辺ツール。[ロードマップ](https://cairn.kage1020.com/ja/roadmap/)から選んでも、自分が困っていることから始めても構いません。
- **仕様の修正。** 誤りの修正、記述の明確化、例の改善。各章は単独で読めるようにし、他の章へはルートからの絶対パスのリンク (`/ja/spec/syntax/`) で参照してください。
- **設計への批判。** 決定への異議、抜けているケースの指摘、代案の提案。該当する章と節を示して issue を立ててください。
- **先行事例の紹介。** レッドストーンコンパイラ、schematic のフォーマット、ボクセルや CAD の配置配線、HDL の論理合成など。設計の議論に役立つ情報を歓迎します。

仕様とドキュメントは英語版を基準とします。翻訳は、二次的なコピーであることを明記していれば歓迎します。Chromium は日本語の文字の間の改行を空白として表示するので、`website/src/content/docs/` の日本語のページは 1 段落を 1 行に書きます。英語のページは 100 桁で折り返します。

## 開発環境

[`rust-toolchain.toml`](rust-toolchain.toml) でコンパイラのバージョンを固定しており、`rustup` が自動でそれを使います。リポジトリをチェックアウトして `cargo build` するだけで、CI と同じツールチェーンになります。

```sh
cargo build --workspace
cargo test --workspace
cargo run -p cairn-lang-cli -- check examples/cottage.crn --edition java --target 1.21.4
```

`check` は何もファイルを書き出しません。`compile` はソースと同じ場所に構造ファイルとロックファイルを書き出します。`examples/` にビルド成果物を残したくない場合は、`--out` と `--lock` でリポジトリの外を指定してください。

PR を出す前に、CI と同じコマンドを実行してください。CI は Linux・macOS・Windows で、すべて `RUSTFLAGS=-D warnings` を設定して次のコマンドを実行します。

```sh
export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
cargo test -p cairn-lang-core --bench lowering
cargo test -p cairn-lang-redstone --bench place_and_route
```

`--workspace` ではベンチターゲットが選ばれないので、最後の 2 つで各ベンチを時間計測なしのテストとして 1 回ずつ実行します。何を確かめているかは「[リリースプロファイルの変更](#リリースプロファイルの変更)」で説明します。

CI はこのほかに、Linux 上で rustdoc の警告をエラー扱いにして API ドキュメントを 2 回ビルドします。非公開の項目へのドキュメントリンクや、解決できなくなったパスへのリンクは clippy では検出できないためです。1 回目は公開 API が対象で、公開ドキュメントから非公開項目へのリンクを検出できるのはこちらだけです。2 回目は、コントリビューター向けのドキュメントも対象にします。

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features --document-private-items
```

CI の `Lint` ジョブは、挙動のテストではなく、ファイル同士の整合性を検査します。README の一覧とコード、仕様への参照と仕様本体、診断コード表と `DiagnosticCode`、各所に書かれたバージョンとワークスペースのマニフェストを突き合わせます。指摘にはファイル名と行番号が出ます。各検査がなぜ必要かは、`xtask/src/rules/` にある各ルールのモジュールに書いてあります。

```sh
cargo xtask lint            # すべてのルール
cargo xtask lint --list     # ルールの一覧と、それぞれが検査する内容
cargo xtask lint versions   # 1 つのルールだけ
cargo xtask lint --site website/dist site-parity   # website/ で `pnpm run build` を実行した後
```

### リポジトリの構成

| パス | 内容 |
|---|---|
| `crates/cairn-lang-core` | 字句解析、構文解析、Intent IR、check パス、解決、block-array lowering |
| `crates/cairn-lang-nbt` | NBT のエンコードとデコード。Java はビッグエンディアン、Bedrock はリトルエンディアン |
| `crates/cairn-lang-formats` | 構造ファイルの書き出し (`.nbt` / `.mcstructure`)、レジストリパック、エディション間の可搬性 |
| `crates/cairn-lang-redstone` | Logic IR、ネットリスト、配置、配線、遅延、交差の合法化 |
| `crates/cairn-lang-cli` | `cairn` バイナリ |
| `crates/cairn-lang-lsp` | 言語サーバー `cairn-lsp` |
| `crates/cairn-lang-tree-sitter` | tree-sitter 文法。npm には `tree-sitter-cairn` として公開 |
| `crates/cairn-lang-wasm` | 将来のプレイグラウンド向けのブラウザバインディング。現在はプレースホルダ |
| `editors/vscode` | VS Code 拡張。`cairn-lsp` の薄いクライアント |
| `website` | Astro + Starlight のドキュメントサイト。仕様・チュートリアル・ガイドを英語と日本語で、開発者ガイドを英語で公開。サイトの Markdown ページを `cairn docs` 用に埋め込む `cairn-lang-docs` クレートもここにあります |
| `examples` | `.crn` ソース (ロックファイルは生成物なので追跡しません) |

クレート間の依存関係のルールは[開発者ガイド](https://cairn.kage1020.com/development/) (英語のみ) で詳しく説明しています。

## 書き方

基本は、**How** をコードに、**What** をコミットメッセージに、必要最小限の **Why** をコメントに書くことです。

- 用語は仕様に合わせ、同義語を作らないでください。仕様の用語 (`intent_state` / `resolved_state`、`mat_slot`、正規トークン (canonical token)) をそのまま使います。新しい用語は、それが属する章で定義します。
- 設計原則は、[設計原則](https://cairn.kage1020.com/ja/spec/principles/)の章にある `P1`–`P5` で参照します。
- エラーメッセージには「何が誤りか」「何なら有効か」「どう直せばよいか」の 3 点を書きます。この形があるから、書いて、チェックして、直すというループが回ります。一文長くなっても書く価値があります。
- 例は具体的で最小限のものにします。

### その場限りの参照を残さない

Rust のソース、仕様の本文、サンプル、ドキュメントは、何年か後に読んでも意味が通じなければなりません。issue や PR の番号、`M3-PR4` のような作業上の番号、単独のマイルストーン名、「後続の PR で対応」といった書き方は避けてください。番号の代わりに、それが指していた事実そのものを書きます。

> 変更前: `// M3-PR4 only exposes ports on door members (window / stair / roof ports land later).`
>
> 変更後: `// Ports are currently exposed only on door members. Window / stair / roof ports are reserved for a future extension.`

先送りしていたものを実装したら、その PR の中でコメントも更新します。

例外は、マイルストーンの語彙を扱うこと自体が目的のファイルです。[CHANGELOG.md](CHANGELOG.md) とそのフラグメントを置く [changelog.d/](changelog.d/README.md)、[ロードマップ](https://cairn.kage1020.com/ja/roadmap/)、マイルストーン列のある[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/)のページの 3 つです。それ以外の追跡対象のテキストファイルは、`cargo xtask lint session-refs` がこのルールで検査します。ただし、書かないでほしい番号を例として引用するこのガイド、PR が閉じる issue を記入させる PR テンプレート、changelog スクリプトのテスト、ロックファイルは対象外です。

### 仕様は番号ではなく名前で参照する

節番号も、作業上の番号と同じく古くなります。`§14.5` が正しいのは、「Place-and-route」が第 14 章の 5 番目の節である間だけです。章を追加したり節を分割したりすると、その節を番号で指していたコメントはすべて、誰にも気付かれないまま誤りになります。そして、それを見つけるにはソースツリー全体を読み直すしかありません。仕様を編集するたびにそんな手間をかけるべきではありません。章はファイル名で書き、特定の節を指すときはその見出しを書いてください。

> 変更前: ``/// Stable per `spec/lint.md` §11.3: errors are things the compiler refuses to guess at.``
>
> 変更後: ``/// Stable per `spec/lint` "Error vs warning": errors are things the compiler refuses to guess at.``

章は拡張子を除いたファイル名で書き (`spec/redstone`、`spec/versioning-editions`)、節の見出しは一字一句そのまま写します。章全体を指すときは章だけを書きます。同じコメントの中で一度引用したら、後の文では「that pipeline」「the phase order」のように普通の言葉で受けてください。同じ引用を繰り返す必要はありません。

`cargo xtask lint spec-citations` が、`crates/`、`examples/`、`website/rust/`、`xtask/`、`.github/`、`.claude/`、`CLAUDE.md` にある `.rs`、`.md`、`.crn` ファイルを対象に、次の 3 点を検査します。

- `§` や "section 11.3" のような番号での参照が残っていないこと
- `spec/<章>` が実在する章を指していること
- 引用符で囲んだ見出しが、`cairn docs` でその章から表示できるもの (見出し、章のリスト項目が太字で定義する用語 (`- **Term.**`)、章のタイトル) であること

この 3 点が守られているので、引用はそのまま `cairn docs spec/lint "Error vs warning"` として実行できます。章の中に同じタイトルが複数あると、どれを指すのか決まらないので、ルールが失敗します。仕様の番号を振り直しても、Rust のコードは 1 行も変わりません。節の見出しを変えると、それを引用している箇所ごとにファイル名と行番号付きでルールが失敗しますが、これは意図した動作です。見出しの変更は意味の変更であり、それを頼りにしていたコメントは読み直すべきだからです。

これ以外の場所は、人が目で確認するしかありません。`website/` のページは、番号を定義している側なので検査しません。仕様と並ぶガイドページは、今も番号を含むアンカーで節にリンクしています。これを直すのは、見出しそのものを変えるときに合わせて行う作業です。`editors/`、このファイル、`CHANGELOG.md`、`changelog.d/`、そして `.github/scripts/` のスクリプトのような他の種類のファイルは、単に検査の範囲外です。

### クレートの README

各クレートの `README.md` は crates.io に表示され、リポジトリを見たことがない人にとっての表紙になります。ステージを実装したら、同じ PR でそのクレートの README も更新してください。表紙に「これから実装予定」と書いてあるうちは、そのステージはリリースされていないのと同じです。

コードと突き合わせられる一覧を持つクレートの README は 4 つあります。どれもレビューではなく `cargo xtask lint` が検査します。

- `cairn-lang-core` のモジュール表と、クレートの `pub mod` 宣言
- `cairn-lang-cli` のサブコマンド表と、`cairn --help` が表示する一覧
- `cairn-lang-cli` の `cairn synth` の節と、`--stage` が受け付けるステージの一覧、その順序、`--edition` が必須になるステージ
- `cairn-lang-nbt` と `cairn-lang-formats` の `## Public API` 表と、各クレートルートの再エクスポート

`pub mod`、サブコマンド、ステージ、`pub use` のいずれかを追加して表に行を足さないと、lint が失敗して足りない項目を示します。検査は双方向なので、もうリリースしていないものの行が残っていても失敗します。

`## Public API` の対象は、クレートルートの `pub use` による再エクスポートだけです。モジュールの中でだけ `pub` な項目 (`registry::AliasCatalog` や `registry::blocks` 以下の項目など) は、意図的に表に載せていません。説明が必要なら本文に書いてください。表の行の最初のセルには、コードスパン 1 つにつき `module::item` の完全なパスを 1 つ書きます。同じ説明を共有する項目なら、複数のスパンを並べて構いません。まだ表がないクレートに `## Public API` 表を追加するときは、`xtask/src/rules/readme.rs` の `PUBLIC_API_CRATES` にそのクレートを追加してください。忘れても、同じルールが教えてくれます。

一方、これらの一覧以外の文章は、どのルールも検査しません。行の説明、「Status」の段落、表を*予定*と呼ぶ見出し、「まだ実装されていないもの」の一覧などが正しく保たれていたのは、誰かが覚えていたからにすぎません。実際、公開済みの 2 つのクレートの README は、コンパイラが動くようになった後も長い間、未実装のスケルトンだと説明し続けていました。「未実装」から外したものは、README からも外してください。今リリースされているものを書き、これから先のものは別の節にまとめます。そうすれば、読者はコードを動かさなくても両者を見分けられます。

## ブランチとプルリクエスト

開発の幹となるブランチは `canary` です。リリースを公開するのは `main` からだけで、ほかのブランチからは何も公開しません。毎月 1 回、リリースのワークフローが `canary` から `main` への昇格 PR (promote-to-main PR) を開きます。これをマージすると `main` にリリース PR が開かれ、そのリリース PR をマージすると `main` が公開されます。続けてワークフローは `main` を `canary` に戻す PR を開くので、次の昇格までに、リリースのコミットとホットフィックスはすべて `canary` にも入ります。

| ブランチ | 役割 |
|---|---|
| `canary` | 機能追加、修正、ドキュメントの変更はすべてここに入る。保護されている |
| `main` | 昇格 PR、リリース PR、ホットフィックスの PR で更新され、どれもメンテナが承認する。直接の push は不可 |
| `<type>/<short-kebab>` | 1 つの変更のための作業ブランチ。`canary` に向けて PR を出し、マージ後に削除する |
| `hotfix/<short-kebab>` | セキュリティ修正など、次の昇格を待てない修正のためのブランチ。`main` から切って `main` に向けて PR を出し、メンテナに頼まれたときだけ作る。マージ後に削除する |
| `release-plz-*` | リリースのワークフローが `main` に向けて作る。リリースが必要になる昇格やホットフィックスが `main` に入るたびに作られる |

ブランチ名には、その変更を取り込むときの Conventional Commits の type を使います (`feat/parser-lexer`、`fix/wall-corner-shape`、`docs/roadmap-2027`)。

**PR のタイトルは [Conventional Commits](https://www.conventionalcommits.org/) の形式で 1 行にしてください。** リリースのワークフローが `canary` と `main` の間に開く 2 つの PR を除いて、マージはすべて squash merge なので、このタイトルがそのまま PR の向け先のブランチのコミットになります。`release-plz` がパッチリリースの要否を判断するときに読むのもこのタイトルです。作業ブランチ上の個々のコミットは自由な形式で構いません。スコープには対象のクレートか仕様の領域を書きます (`feat(core)`、`fix(nbt)`、`docs(spec)`、`build(deps)`)。

| Type | 使う場面 | パッチリリースが必要になるか |
|---|---|---|
| `feat` | 新機能、公開 API やサブコマンドの追加 | はい |
| `fix` | 挙動を仕様どおりに直す修正 | はい |
| `perf` | 性能の改善 | はい |
| `refactor` | 挙動を変えない内部構造の整理 | はい |
| `build` | ビルドシステム、パッケージング、Cargo の依存関係 | はい |
| `docs` | ドキュメント、仕様の本文、README、サンプル | いいえ |
| `test` | テストコードだけの変更 | いいえ |
| `ci` | ワークフロー、release-plz、`rust-toolchain.toml` | いいえ |
| `chore` | 利用者に届かないその他の変更 | いいえ |
| `style` | 整形や lint 対応だけの変更 | いいえ |

出すべきパッチリリースは、メンテナがリリースのワークフローを `step: promote` で手動で動かしたとき、またはホットフィックスが `main` に入ったときに出ます。スケジュールで動くのは、月次の昇格だけです。

**破壊的変更では、コロンの直前に `!` を付けます。** 位置は type の後ろ、スコープがあればスコープの後ろです (`feat(core)!: replace lexer`、`fix!: …`)。`!` と CHANGELOG は必ずセットにします。`changelog.d/<slug>.breaking.md` のフラグメントを追加する PR はタイトルに `!` を付け、タイトルに `!` を付けた PR はこのフラグメントを追加します。

フラグメントに手で書くエントリは、[compatibility の「破壊的変更の告知方法」](https://cairn.kage1020.com/ja/spec/compatibility/)が求めているものです。リリース時に `### Breaking changes` の下にまとめられ、`CHANGELOG.md` の読者が目にします。`!` は同じ事実を、`release-plz` が解析するコミットの側に書いたものです。片方しかない PR は `Changelog` チェックが指摘します。このチェックが必須になるまでは、片方しかない PR を見つけたレビュアーが、マージ前にもう片方を求めます。

`!` を付けてもバージョンは決まりません。semver なら `!` はメジャーバージョンを上げる合図ですが、Cairn では違います。リリースワークフローが日付と既存のタグから次の `YYYY.M.PATCH` を計算し、`release-plz` を実行する前に `Cargo.toml` に書き込みます。`release-plz` は、公開済みのものとすでに異なるバージョンには手を付けません。リリースするかどうかも `!` では決まらず、上の表の type で決まります。`!` が変えるのは、生成されるリリースノートだけです。`release-plz` はそのコミットの行の先頭に `[**breaking**]` を付けます。また `protect_breaking_commits` により、普段はリリースノートから省かれる type (`docs!:`、`ci!:`) でもその行が残ります。

自分で開く PR は、メンテナにホットフィックスを頼まれたときを除いて、すべて `canary` に向けます。マージにはメンテナ 1 名の承認が必要で、CI が通っていることも求められます。ただし、ブランチのルールセットはまだどのチェックも必須にしていません。リリース PR も同じルールに従います。

ホットフィックスは `main` から `hotfix/<short-kebab>` として切り、ほかの PR と同じようにタイトルを付けてフラグメントを追加します。`main` にマージすると次のパッチのリリース PR が開かれ、その中身は `main` にあるものだけです。リリース PR をマージすると修正が公開され、続いて `main` を `canary` に戻す PR が修正を `canary` に届けます。

昇格 PR と、`main` を `canary` に戻す PR は、メンテナが手でマージコミットを作ってマージします。squash すると `main` のコミットが `canary` の履歴に入らず、`canary` にないコミットが `main` にある間は、次の昇格が止まります。どちらの PR も自動マージを有効にしません。ワークフローのトークンに代わって GitHub が行ったマージは、ワークフローを起動しないからです。昇格のマージは、リリース PR を開くワークフローを起動しなければなりません。

## CHANGELOG のエントリ

PR では [CHANGELOG.md](CHANGELOG.md) を直接編集しません。代わりに [changelog.d/](changelog.d/README.md) に、エントリ 1 つにつき 1 ファイルのフラグメントを追加します。ファイル名は `<slug>.<kind>.md` で、kind によって見出しが決まります (`breaking`、`deprecations`、`added`、`changed`、`fixed`、`performance`、`build`)。

フラグメントに分けるのは衝突を避けるためです。20 本の PR が同時に開いていて、どれも同じ `[Unreleased]` の見出しを編集すると、マージのたびに次の PR が衝突します。ファイルが別々なら衝突しません。フラグメントは、月次でもパッチでも、リリース PR が `release-plz` の書いた新バージョンの節にまとめ、その後削除します。

`Changelog` ワークフローは、PR が追加または変更したフラグメントの名前と形式を検査します。issue や PR の番号、`ci` だけをスコープにしたエントリ、リリース PR 以外による `CHANGELOG.md` の変更も指摘します。詳しいルールと、同じ検査をローカルで実行するコマンドは [changelog.d/README.md](changelog.d/README.md) にあります。この仕組みの対象はルートの `CHANGELOG.md` だけです。`editors/vscode/CHANGELOG.md` は拡張機能専用のもので、これまでどおり手で編集します。

## 決定済みの事項を見直す

いくつかの事項は、意図的に決定済みとしています。たとえば、位置引数ではなく `key=value` を使うこと、フェーズ順に評価すること、変換せずに再コンパイルすること、黙って代替せずにエラーにすることです。これらを見直したいときは、次の内容を含む issue を立ててください。

- 見直したい決定と、仕様のどこに書かれているか
- その決定ではうまく扱えない具体的なケース
- 構文・IR・メッセージの例を添えた代案
- [評価指標](https://cairn.kage1020.com/ja/spec/evaluation/)への影響

## ツールチェーンの更新

ツールチェーンは、意図的にチャンネルではなく特定のバージョンに固定しています。`stable` を指定していると、Rust の新しいリリースが出た瞬間に、開いているすべてのブランチの CI が一斉に失敗します。しかも指摘されるのは、そのブランチが触ってもいないファイルです。固定する目的は、新しい lint を避けることではなく、新しい lint への対応を独立した PR にすることです。

Rust の新しいリリースが出ると、Dependabot が `channel` を書き換える PR を type `ci` で開きます。そのブランチで上の CI のコマンドを実行し、見つかった問題の修正を同じブランチに push してください。新しいコンパイラは clippy の lint だけでなく *rustc* や *rustdoc* の警告も出すので、固定の更新で `Docs` ジョブが失敗することもあります。「新しいコンパイラと、それが求めた修正だけ」の差分なら、レビュアーも無理なく読めます。

固定したバージョンは MSRV ではありません。ワークスペースのマニフェストにある `rust-version` は、利用者が Cairn をビルドするのに必要な最低バージョンで、固定したバージョンは常にそれより新しくなるので、最近安定化された API を使う変更は、固定したバージョンでは通っても、最低バージョンでは壊れます。

この問題の一部は clippy が検出します。`clippy::incompatible_msrv` は `rust-version` を読み、それより新しいバージョンで安定化された**標準ライブラリ**の項目を拒否します。`-D warnings` があるので、これはエラーになります。ただし lint なので `#[allow]` 1 行で黙らせられます。また、**依存クレート**自身の `rust-version` が Cairn の最低バージョンより新しい場合は何も言いません。こちらは cargo のハードエラーで、固定したバージョンでビルドしている限り表に出ません。

両方を検出するのが、最低バージョンでコンパイルする CI の `MSRV` ジョブです。このジョブは `cargo metadata` でマニフェストから `rust-version` を読み取り (別の場所にコピーを持つと古くなるためです)、そのバージョンのコンパイラをインストールして `cargo check --workspace --locked --all-features` を実行します。オプションの理由は次のとおりです。

- `test` ではなく `check` を使うのは、最低バージョンで問うべきなのが「利用者が依存するクレートをコンパイルできるか」だからです。dev-dependencies やテストハーネスは、ライブラリ本体より新しいコンパイラを必要としても構いません。
- `--all-features` を付けるのは、`rust-version` はパッケージごとに 1 つしか書けず、「feature を有効にした場合だけは除く」と cargo に伝える方法がないからです。
- `--locked` を付けるのは、リポジトリをクローンした利用者が使うのはコミット済みのロックファイルそのものだからです。失敗の原因が Cairn のコードではなく依存クレート自身の最低バージョンにある場合は、cargo がそのパッケージ名を示します。

ワークスペースには、Cargo の MSRV を考慮するリゾルバ `resolver = "3"` を指定しているので、`cargo update` や依存クレートの追加では、依存クレート自身の `rust-version` が Cairn の最低バージョン以下である版のうち、最新のものが選ばれます。最低バージョンを超える依存クレートは、そもそも `Cargo.lock` に入りません。このキーを明示しているのは、仮想マニフェストではメンバーの edition からリゾルバが決まらず、書かなければリゾルバ 1 になってしまうからです。なお、リゾルバが除外するのは `rust-version` を宣言している候補だけです。宣言のない依存クレートや、`cargo update --precise` で指定した版はロックファイルに入ります。それを検出するのは、やはり `MSRV` ジョブです。

`rust-version` を上げると Cairn をビルドできる人が変わるので、マニフェストを黙って書き換えて済ませてはいけません。コミットの type は `build` にします (パッチリリースを出すべき変更になるので、新しい最低バージョンが crates.io まで届きます)。さらに `changelog.d/<slug>.build.md` に、どのコンパイラが必要になったのか、何がそれを必要としたのかを書きます。新しい最低バージョンより古いコンパイラを使っている利用者は、いずれにせよ cargo のエラーで気付きます。その理由は、このエントリでわかります。

## リリースプロファイルの変更

ワークスペースのマニフェストにある `[profile.release]` は、リリースアーカイブのサイズを基準に調整しています。各設定の理由はコメントに書いてあります。ほとんどの設定はビルド時間にしか影響しません。例外は `opt-level` で、サイズを優先するレベルにするとアーカイブは小さくなりますが、block-array lowering と配置配線 (place-and-route) が遅くなります。ビルドの時間の大半を占めるのは、この 2 つの処理です。

この処理速度を測るベンチが 2 つあります。`cairn-lang-core` の `lowering` と、`cairn-lang-redstone` の `place_and_route` です。どちらもパスをプロセス内から直接呼び出します。サンプルに対して CLI の時間を測ると、ほとんどがプロセスの起動時間になってしまうからです。また、どちらもサンプルではなく生成したソースを使います。サンプル程度の小さなソースはパスをすぐに通り抜けてしまい、変更による差をタイマーで見分けられないからです。

ベンチはリリースプロファイルの設定を引き継ぐので、`opt-level`、`lto`、`codegen-units` は配布時と同じ設定で計測されます。例外は `panic` です。Cargo はテストと同じく、ベンチターゲットとその依存クレートをアンワインドありでビルドします。

計測するときは、まずベースラインを保存し、プロファイルを変えてから比較します。ファイルを編集せず、環境変数で 1 つの設定だけを上書きしても構いません。

```sh
cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route -- --save-baseline before
CARGO_PROFILE_RELEASE_OPT_LEVEL=s cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route -- --baseline before
```

2 つのベンチは名前で指定してください。`--bench` を付けないと、`cargo bench` は各ライブラリの単体テストのハーネスも実行し、そちらは criterion の `--save-baseline` を受け付けません。差を信用する前に、一度ベースラインをそれ自身と比較してください。共有マシンでは、まったく同じビルドでも、小さなベンチの結果が数パーセント揺れます。

サイズの変化は gzip 圧縮後のバイナリで比べます。リリースアーカイブが `.tar.gz` と `.zip` だからです。計測した数値は、プロファイルのコメントではなくコミットメッセージに書きます。コメントに書くと、次に依存クレートを更新したときに、誰にも気付かれないまま古くなります。

CI はどちらのベンチも時間を計測しません。共有ランナーでの時間計測をゲートにしても、ノイズにしかならないからです。プロファイルの変更は一度決めれば済む判断で、回帰を監視し続ける対象ではありません。CI が実行するのは、各ベンチを時間計測なしのテストとして 1 回ずつ走らせる `cargo test -p cairn-lang-core --bench lowering` と `cargo test -p cairn-lang-redstone --bench place_and_route` です。

各ベンチは計測を始める前に、生成したソースが想定どおりの処理を通ることを確かめます。`lowering` は、すべての配置と通路が block-array lowering から出力されることを確かめます。`place_and_route` は、すべてのスコープがすべてのパスを通り、ゲート 1 つにつき配置済みのセルが 1 つあることを確かめます。生成器がパスまで届かなくなった場合は、黙って少ない処理を計測するのではなく、CI で失敗します。

## バージョニング

日付ベースの `YYYY.M[.PATCH]` を使います。主な変更は [CHANGELOG.md](CHANGELOG.md) に記録します。バージョンを上げるときに何を壊してよいかは、番号そのものではなく[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/)で決まります。

## 行動規範

このプロジェクトは [Contributor Covenant](CODE_OF_CONDUCT.md) に従います。参加した時点で、これを守ることに同意したものとみなします。
