# Cairn へのコントリビュート

> 言語: **日本語** ([English](CONTRIBUTING.md))

Cairn は、規範的な[仕様](https://cairn.kage1020.com/ja/spec/)を背後に持つ、動くコンパイラです。どちらもまだ動いている最中で、どちらもコントリビュートを受け付けています。

## 手伝えること

- **バグ報告。** `.crn` ファイル、実行したコマンド、期待した結果。クラッシュするファイルと同じくらい、間違ったブロックにコンパイルされるファイルにも価値があります。
- **サンプル。** 実際の建築を書いて、言語が扱いにくかった・曖昧だった・力尽きた箇所を教えてください。語彙を育てるのはこれです。メンバーをひとつずつ扱う小さなファイルも、大きなものと並べて [`examples/`](examples/) に置いています。
- **コード。** パーサ、lowering、バックエンド、レッドストーン、LSP、周辺ツール。[ロードマップ](https://cairn.kage1020.com/ja/roadmap/)から選んでも、自分の困りごとから始めても構いません。
- **仕様の修正。** 誤りの修正、記述の明確化、例の改善。各章は単体で読めるように保ち、相対リンクで相互参照してください。
- **設計への批判。** 決定に異議を唱える、抜けているケースを指摘する、代案を出す。該当する章と節を指した issue を立ててください。
- **先行事例。** レッドストーンコンパイラ、schematic フォーマット、ボクセル/CAD の place-and-route、HDL 合成 — 設計議論での情報提供は歓迎します。

仕様とドキュメントの正典は英語です。翻訳は、二次的なコピーであることが明示されている限り歓迎します。

## 開発環境

[`rust-toolchain.toml`](rust-toolchain.toml) が正確なコンパイラを固定しており、`rustup` がそれを自動で拾います。チェックアウトして `cargo build` すれば、CI と同じツールチェーンになります。

```sh
cargo build --workspace
cargo test --workspace
cargo run -p cairn-lang-cli -- check examples/cottage.crn --edition java --target 1.21.4
```

`check` は何も書き出しません。`compile` はソースの隣に構造ファイルとロックファイルを書くので、`examples/` にビルド成果物を残したくなければ `--out` と `--lock` をツリーの外に向けてください。

PR を出す前に、CI と同じものを回してください。Linux・macOS・Windows で走るのはこの 4 コマンドで、いずれも `RUSTFLAGS=-D warnings` の下で実行されます。

```sh
export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --locked
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

クレート間の依存ルールは[開発者ガイド](https://cairn.kage1020.com/development/) (英語のみ) が詳しく扱っています。

## 書き方

原則は、**How** はコードに、**What** はコミットメッセージに、最小限の **Why** をコメントに。

- 仕様が正典です。同義語を作らず、仕様の用語 (`intent_state` / `resolved_state`、`mat_slot`、canonical token) をそのまま使ってください。新しい用語は、それが属する章で導入します。
- 設計原則は [Design Principles](https://cairn.kage1020.com/ja/spec/principles/) の `P1`–`P5` として参照します。
- エラーメッセージは「何が誤りか / 何なら妥当か / 修正案」の形を取ります。この形が「書く → チェックする → 直す」のループを成立させているので、一文増えるだけの価値があります。
- 例は具体的かつ最小限に。

### セッション固有の参照を残さない

Rust のソース、仕様本文、サンプル、ドキュメントは、何年か後に単体で読めなければなりません。issue や PR の番号、`M3-PR4` のような座標、裸のマイルストーンラベル、「後続の PR で対応」といった記述は入れないでください。座標は、それが代弁していた事実に置き換えます。

> 変更前: `// M3-PR4 only exposes ports on door members (window / stair / roof ports land later).`
>
> 変更後: `// Ports are currently exposed only on door members. Window / stair / roof ports are reserved for a future extension.`

先送りされていたものが実装されたら、それを入れる PR の中で同時にコメントを更新します。

例外は 3 つだけです。マイルストーンの語彙こそがその目的である面 — [CHANGELOG.md](CHANGELOG.md)、[ロードマップ](https://cairn.kage1020.com/ja/roadmap/)、[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/)表のマイルストーン列。それ以外はレビュー時に次で確認できます。

```sh
rg '\bM[1-6]\b|M[0-9]-PR[0-9]+|pre-M[0-9]|\bPR[0-9]+\b|\blater PR\b|\bfuture PR\b' \
  --glob '!CHANGELOG*' --glob '!CONTRIBUTING*' \
  --glob '!**/compatibility.md' --glob '!**/roadmap.md' --glob '!target/**'
```

結果が空であることが約束事です。

### 仕様は番号ではなく名前で参照する

節番号も同じ種類の座標です。`§14.5` が正しいのは「Place-and-route」がたまたま第 14 章の 5 番目の節である間だけで、章を差し込んだり節を分割したりすれば、それを名指していたコメントはすべて黙って嘘になります。そして見つける手段はソースツリー全体を読み直すことだけです。仕様を直すのにその代償を払う必要はありません。章のファイルと、特定の節を指すならその見出しの名前を書いてください。

> 変更前: ``/// Stable per `spec/lint.md` §11.3: errors are things the compiler refuses to guess at.``
>
> 変更後: ``/// Stable per `spec/lint` "Error vs warning": errors are things the compiler refuses to guess at.``

章は拡張子なしのファイル名（`spec/redstone`、`spec/versioning-editions`）で書き、節の見出しはそのまま写します。章全体を指すときは章だけを挙げます。一度その形で引いたあとは、同じコメントの後続の文では「that pipeline」「the phase order」のように散文で受けてください。同じ引用を繰り返す必要はありません。

これは `cargo test --workspace` が `crates/`、`examples/`、`.github/` の 3 つの木に対して守ります。その中に `§` や "section 11.3" が残っていないこと、`spec/<章>` が実在する章を指していること、引用符で囲まれた見出しがその章に実在することの 3 点です。仕様を採番し直しても Rust 側は 1 行も動きません。節の見出しを変えた場合はテストが引用箇所をファイルと行番号で名指して落ちますが、それが狙いです。見出しの変更は意味の変更であり、それに寄りかかっていたコメントは読み直す価値があります。

残りは人の目で見るしかありません。`website/` を読まないのは、そこが番号を定義している側だからです。仕様の隣に並ぶガイドページは今も番号を埋め込んだアンカーで節を参照しており、それをほどくのは見出しそのものを変える変更と一緒にやる仕事です。`editors/`、このファイル、CHANGELOG は単に走査範囲の外です。

### クレートの README

各クレートの `README.md` は crates.io に表示されるものであり、リポジトリを一度も見たことがない人にとっての表紙です。ステージが実装されたら、同じ PR でそのクレートの README も更新してください。表紙がまだ「これから」と言っているうちは、そのステージは出荷されていません。

コードと突き合わせられる一覧を持つクレートの README は 4 つあり、そのすべてがレビューではなくテストで守られています。`cairn-lang-core` のモジュール表はクレートの `pub mod` 宣言と、`cairn-lang-cli` のサブコマンド表は `cairn --help` が並べるものと、`cairn-lang-nbt` と `cairn-lang-formats` の `## Public API` 表は各クレートルートの再エクスポートと照合されます。`pub mod`・サブコマンド・`pub use` のいずれかを追加して行を足さなければ `cargo test --workspace` が落ち、足りていないものを名指しします。どの照合も双方向なので、もう出荷されていないものの行が残っていても落ちます。

`## Public API` が指すのは、クレートルートの `pub use` 再エクスポートだけです。モジュール内でしか `pub` でない項目 — `registry::AliasCatalog` や `registry::blocks` 以下のもの — は意図的に表に載せません。説明が要るなら散文で書いてください。行の第一セルはコードスパン 1 つにつき `module::item` の完全パスを 1 つ書き、1 つの説明文を共有する項目どうしなら複数のスパンを並べて構いません。まだ表を持たないクレートに `## Public API` 表を足すときは、`readme_public_api_tables.rs` の `TABLED_CRATES` にそのクレートを追加してください。忘れても同じテストが教えてくれます。

どのテストも読めないのは散文のほうです。行の説明、「Status」の段落、表を*予定*と呼ぶ見出し、「まだここにない」一覧 — どれも誰かが覚えていたから正しかっただけで、公開済みの 2 クレートではコンパイラが動いたあとも長らく未実装のスケルトンだと説明し続けていました。「まだ」から出したものは README からも出してください。今日出荷されているものを書き、これから先のものは独立した節に置く。そうすればコードを動かさなくても読者が両者を見分けられます。

## ブランチとプルリクエスト

トランクは `canary` です。`main` はリリース済みの状態で、publish のたびにパイプラインが `canary` からの promote-to-main PR を開き auto-merge を有効にします。したがって `main` は 1 リリースごとに承認済みのマージコミット 1 つ分だけ進み、リリースより先へは行きません。

| ブランチ | 役割 |
|---|---|
| `canary` | 機能・修正・ドキュメントのすべてがここに入る。保護対象 |
| `main` | パイプラインの promote-to-main PR だけが動かし、メンテナが承認する。直接 push は不可。コントリビューターが `main` 宛に PR を出すことはない |
| `<type>/<short-kebab>` | 変更ひとつ分の作業ブランチ。`canary` を対象にし、マージ後に削除 |
| `release-plz-*` | 月次マイナーとパッチのために自動で開かれる |

ブランチ名は、その作業が最終的に載る Conventional Commits の type に合わせます (`feat/parser-lexer`、`fix/wall-corner-shape`、`docs/roadmap-2027`)。

**PR タイトルは [Conventional Commits](https://www.conventionalcommits.org/) の 1 行でなければなりません。** squash merge が唯一のマージ方式なので、このタイトルがそのまま `canary` 上のコミットになり、`release-plz` がパッチリリースの要否を判断するために読むのもこれです。ブランチ上の個々のコミットは自由形式で構いません。破壊的変更には `!` を付け (`feat(core)!: replace lexer`)、スコープには対象のクレートか仕様領域を書きます (`feat(core)`、`fix(nbt)`、`docs(spec)`、`build(deps)`)。

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

自分で開く PR はすべて `canary` を対象にします。`main` 宛の PR はパイプラインの promote-to-main だけです。メンテナ 1 名の承認と CI のグリーンが必須です。リリース PR も同じルールで、これをマージすると公開が走り、`main` が fast-forward されます。

## 決着した決定を蒸し返す

いくつかの決定は意図的に閉じています。位置引数ではなく `key=value`、フェーズ順の評価、変換ではなく再コンパイル、黙って代替せず大きく失敗する、など。これを再び開くには、その決定と仕様上の在り処を示し、それが扱えない具体的なケースを挙げ、構文・IR・メッセージの例を伴う代案を出し、[評価指標](https://cairn.kage1020.com/ja/spec/evaluation/)への影響に触れた issue を立ててください。

## ツールチェーンの更新

固定は意図的にチャンネル追従ではありません。`stable` にしていると、Rust のリリースひとつで開いているすべてのブランチが同時に赤くなります。しかもその指摘は、そのブランチが触ってもいないファイルに出ます。固定は新しい lint を避けるためのものではなく、それが「誰かが意図して開いた PR」として届くようにするためのものです。

`channel` を変え、上の CI 3 コマンドを回し (新しいコンパイラは clippy の lint だけでなく *rustc* の警告も出します)、見つかったものを同じ PR で直し、コミットの type は `ci` にします。「新しいコンパイラと、それが要求した修正だけ」という差分は、レビュアーが実際に読める差分です。

固定は MSRV ではありません。ワークスペースマニフェストの `rust-version` は、利用者が Cairn をビルドするのに必要な下限であり、固定はつねにそれより新しいコンパイラです。安定化されたばかりの API に手を伸ばした変更は固定側では緑で、下限では壊れます。その一部は clippy が既に見ています。`clippy::incompatible_msrv` は `rust-version` を読んで、それより上で安定化された**標準ライブラリ**の項目を拒み、`-D warnings` がそれを致命的にします。ただしこれは lint なので `#[allow]` 一行で黙り、さらに**依存先**自身の `rust-version` が我々の下限より上である場合については何も言いません。後者は cargo のハードエラーで、固定側では決して現れません。両方を捕まえるのが、下限でコンパイルする CI の `MSRV` ジョブです。`cargo metadata` でマニフェストから `rust-version` を読み直し (古くなる二つ目のコピーを作らないため)、そのコンパイラを入れて `cargo check --workspace --locked --all-features` を回します。`test` ではなく `check` なのは、下限が問うているのが「利用者が依存するクレートがコンパイルできるか」だからです。dev-dependencies やテストハーネスは、ライブラリ本体より新しいコンパイラを要求してかまいません。`--all-features` を付けるのは、`rust-version` がパッケージごとに一つしか書けず、「この下限、ただしその feature を有効にした場合を除く」と cargo に伝える手段がないからです。`--locked` は、リポジトリをクローンした利用者が解決するのが、コミットされたロックファイルそのものだからで、壊れているのが Cairn のコードではなく依存先自身の下限であるとき、cargo がそのパッケージ名を挙げてくれます。

`rust-version` を上げることは、誰が Cairn をビルドできるかを変えることなので、マニフェストを黙って書き換えて済ませません。コミットの type は `build` にし (パッチリリースが切られるので、新しい下限が crates.io まで届きます)、どのコンパイラが必要になり何がそれを要求したのかを `CHANGELOG.md` に書きます。新しい下限より下に固定している利用者は、どのみち cargo から知らされます。理由を伝えるのがこのエントリです。

## リリースプロファイルの変更

ワークスペースマニフェストの `[profile.release]` はリリースアーカイブのサイズに合わせて調整してあり、各設定にはそこにある理由が書いてあります。ほとんどの設定はビルド時間しか使いません。例外は `opt-level` で、これはブロック配列への lowering と配置配線 (place-and-route) のスループットと引き換えになります。どちらもビルドが時間を費やす処理です。その側を測るベンチが二つあり、どちらもプロセス起動よりパスの処理が重くなる大きさのソースを生成して使います。`cairn-lang-core` の `lowering` と、`cairn-lang-redstone` の `place_and_route` です。ベンチはリリースプロファイルを継承するので、計測されるのは実際に配布されるコードです。ベースラインを保存し、プロファイルを変えて (ファイルを編集せずに環境変数で一つだけ上書きしてもかまいません) 比較します。

```sh
cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route -- --save-baseline before
CARGO_PROFILE_RELEASE_OPT_LEVEL=s cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route -- --baseline before
```

二つのベンチは名前で指定してください。`--bench` を付けないと、`cargo bench` は各ライブラリの単体テストハーネスも実行し、そちらは criterion の `--save-baseline` を受け付けません。差を信じる前に、一度ベースラインをそれ自身と比べてください。共有マシンでは、同一のビルドどうしでも小さいベンチは数パーセント動きます。

サイズの変化は gzip 後のバイナリで比べます。リリースアーカイブが `.tar.gz` と `.zip` だからです。数値はプロファイルのコメントではなくコミットメッセージに書きます。コメントに書くと、次の依存更新で黙って古くなります。CI はどちらのベンチも実行しません。共有ランナーでの時間計測のゲートはノイズにしかならず、プロファイルの変更は回帰を監視する対象ではなく、一度下す決定だからです。CI が守っているのは、`clippy --all-targets` を通じて両ベンチがコンパイルできることです。`cargo test -p cairn-lang-core --bench lowering` (`place_and_route` も同様) は各ベンチマークを一度ずつ実行する確認になります。各ベンチは生成したソースがどのスコープも失わずに全パスを通ることを確かめるので、パスまで届かなくなった生成器は、黙って少ない処理を計測するのではなく、そこで失敗します。

## バージョニング

日付ベースの `YYYY.M[.PATCH]` です。主要な変更は [CHANGELOG.md](CHANGELOG.md) に記録します。バージョンを上げるときに何を壊してよいかは番号そのものではなく、[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/)が定めます。

## 行動規範

このプロジェクトは [Contributor Covenant](CODE_OF_CONDUCT.md) に従います。参加する時点で、これを守ることに同意したものとみなされます。
