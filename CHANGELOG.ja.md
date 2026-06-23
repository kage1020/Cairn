# 変更履歴

> 言語: **日本語** ([English](CHANGELOG.md))
>
> 英語版が source of truth です。

書式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に従います (release-plz が
リリースエントリを綺麗に追記できるようにするため)。Cairn は日付ベースバージョニング (CalVer)
`YYYY.0M[.PATCH]` を採用します。これは「言語仕様 + リファレンスコンパイラ + 標準ライブラリ +
レジストリ/制約パック」をまとめたバンドルのバージョンであり、Minecraft のターゲットバージョンとは
別軸です。

## [Unreleased]

最初の公開ナンバー付きリリースは **`2026.07.0`** (予定) です。それまでの間、本節はそのリリースに
向けてリポジトリに積まれた内容を記録します。`cairn-lang-*` クレートはまだ crates.io に公開されて
おらず、`[workspace.package].publish` は `false` のため `0.0.0` プレースホルダが外部に漏れる
ことはありません。`2026.07.0` のリリース PR で publish を `true` にフリップします。

### Added

- `cairn-lang-core::block_array::roof` — 既存の `gable` ジェネレータに加え
  `shed` / `hip` / `flat` 屋根ボクセライザを追加し、`spec/compilation.md`
  §4.3 で保留扱いだった「より広い屋根タクソノミ」のカーブアウトを解消した。
  `RoofKind::from_ident` が `kind=gable|shed|hip|flat` をパースし、
  `block_array::lower` の `fill_roof` ディスパッチャが各 kind を専用の
  ジェネレータと intern テーブルへルーティングする。`kind=shed` は
  新しい `slope_to=front|back|left|right` 引数（屋根の高い側）を要求し、
  壁の頂上から `slope_span` ボクセル積み上がり、stair は高い側を向く。
  `kind=hip` は `ceil(short_span / 2)` ボクセル昇り、各層は inset
  された矩形枠で四隅は `shape=outer_left|outer_right`、長方形 footprint
  ではリッジ層が長軸方向の行になる。`kind=flat` は `wall_top + 1` の
  単一層で、inflate された roof bounding box 全域を
  `minecraft:spruce_planks` で埋める。すべての kind は既存の overhang
  ルールを共有し、ハードコード ID と `mat_slot=` のミスマッチ検知も
  踏襲する（斜め屋根は `minecraft:spruce_stairs`、flat は
  `minecraft:spruce_planks` を出力。per-theme 屋根樹種は registry pack
  で後追い）。新しい `examples/roof-shed.crn`, `examples/roof-hip.crn`,
  `examples/roof-flat.crn` fixtures が CLI 経由で新 kind を pin する
  (2027.01.0)。
- `cairn-lang-core::suggest` — `nearest_match(input, candidates)` は
  Damerau-Levenshtein 距離でクローズドな語彙から最近接候補を返す
  ユーティリティ。閾値は入力長スケール (1〜3 文字なら 1 編集以下、4〜6 文字
  なら 2、それ以上は 3)、DSL 識別子は case-sensitive なので大文字小文字も 1
  編集として扱い、距離同点なら候補列挙の先頭が勝つ。これを 3 つの診断面で
  利用するようにし、閾値内に候補があれば notes 先頭に
  `did you mean \`X\`?` を付与する。閾値外なら既存のクローズドセット列挙
  (`E_UNKNOWN_KEYWORD` の `expected one of: ...` 行、`E_UNRESOLVED_SLOT`
  の slot 修正提案行) だけが残り、ノイズになる推測は出さない。
  `E_UNKNOWN_KEYWORD` の候補プールは `known_keywords()` 全件、`mat_slot=`
  リゾルバの候補プールは適用された theme が宣言する slot のみ (別 theme の
  slot は `mat_slot=` で結べないため、提案しても直しようがない)。
  `cairn-lang-formats::data_version` の `UnsupportedTarget` には
  `suggestion: String` フィールドを追加し、`thiserror` の `Display` テンプ
  レートに `"did you mean \`1.21.4\`? "` 前置を埋め込むので、CLI で
  `cairn compile --target 1.21.5` が targeted な修正案つきで終了するように
  なる。候補プールは登録 `mc_version` 全件 + `"latest"` エイリアス。
  `spec/glossary.md` "Fail-loud" の後半 — 「エラーは候補集合と修正案の両方
  を返さねばならない」 — を満たす (2026.12.0)。
- `cairn-lang-formats::registry` — registry pack ローダ。マニフェスト
  (`pack.json`) と `(mc_version, DataVersion)` テーブル
  (`data_versions.json`) を読み込む。ビルトインの Java パックは
  `data/registry/java/` 配下に置き、`include_str!` でバイナリに埋め込む。
  `load_from_dir` は後続 PR で導入予定の `--registry-pack <dir>` フラグの
  接続点。`PackFiles` は将来 blocks / items / tags / semantic-sensitivity
  カタログを `Option` で受け入れる拡張余地を持ち、古いパックも読み続けら
  れる。ロード時に schema_version の上限、空の versions、`versions` に
  含まれない `latest`、エディション不一致をすべて拒否する。パックの
  バイト列ハッシュ (`sha256` over manifest + 各コンポーネント) は
  `RegistryPack::bytes_hash` で取得でき、lockfile の
  `inputs.registry_pack_hash` に格納される。
- `cairn compile examples/cottage.crn --edition java` が cottage 一式
  (床、壁、overhang 付き gable 屋根、正面のドア開口、左右対称な正面窓 2 枚)
  を出力するようになった。block-array lowering pass が
  `spec/compilation.md` §4.1 のフェーズ順評価 (massing → envelope → openings)
  を実装し、ソースで `door` を `walls` より前に書いても実際の開口が壁に穿たれる。
  `Dims` は x/z 軸を `2 * overhang` 拡張し、床・壁・開口を `+overhang` シフトする
  ことで、ソース上の `size=WxH` の意味を保ったまま屋根の張り出しを表現する。
  gable 屋根は `minecraft:spruce_stairs` をハードコードし、`facing` を傾斜方向から
  導出 (`-z` 面は `south`、`+z` 面は `north`)、棟頂点は奇数 span なら `half=top`
  1 ブロック、偶数 span なら左右対称の `half=top` 2 ブロックで閉じる (旧実装は
  偶数 span 時に棟が開いた V 字になっていた)。ドアは壁高を超えて掘らないように
  キャップされ、壁を持たない struct では deferred 警告を出して掘らない。
  `at=center` は偶数幅の壁で round-half-up に変更。`sym=true` の窓ミラーが
  主矩形と重なる場合は `W_DEFERRED_MEMBER` を出してミラーをスキップ。
  door/window で `side=` が欠落・型違反の場合は黙って drop せず明示的に診断する。
  `roof kind=gable` の `mat_slot=` が `minecraft:spruce_stairs` 以外に解決される
  場合、ハードコード材との不一致を deferred 警告として通知する。
  cottage example は `W_DEFERRED_MEMBER` 警告ゼロで lowering 完了。
  他の屋根 kind (`shed`, `hip`, `flat`) と door ブロック自体の配置は後続 PR に残る。
  M2 の cottage end-to-end マイルストーン (2026.11.0) を達成。
- `cairn info <file>` CLI サブコマンドが `.crn` ソースに対する 3 軸のバージョン情報
  (registry-compatible range、edition 間ポータビリティ、semantic-sensitive members) を
  出力する。`spec/versioning-editions.md` §10.5 のサンプル形式に準拠。
  `--editions java,bedrock` で対象エディションを制御 (デフォルト `java,bedrock`)、
  `--format text|json` で人間向けレポートと `VersionAxes` JSON を切り替え。M2-PR3 では
  registry range を `@requires version>=X` ヘッダから導出。ポータビリティと
  semantic-sensitivity catalog のデータは registry pack (2026.12.0) と同時に投入予定。
- `cairn_lang_core::resolve` モジュール — Intent IR 上のセマンティックレイヤ。
  `theme` / `def` / `struct` / `site` を走査し、各 `mat_slot=NAME` を theme の
  `slot NAME -> VALUE` と束ね、theme セレクタとメンバを照合し、slot ターゲットを
  canonical / abstract material token として分類する (`spec/materials-themes.md` §7.2)。
  `cairn check` はこの `resolve()` をパイプライン末尾で実行し、theme 束縛の問題を
  構文 diagnostic と並べて報告する。
- 新規 diagnostic コード 3 種: `E_UNRESOLVED_SLOT` (Error; 適用 theme に存在しないスロット
  への `mat_slot=` 参照)、`E_UNKNOWN_SLOT_TARGET` (Warning; `slot X -> VALUE` の VALUE が
  canonical でも abstract でもない)、`E_THEME_SELECTOR_UNMATCHED` (Warning; どのメンバとも
  マッチしないセレクタ)。`DiagnosticCode::severity()` は variant 毎の判定に変更。
- コアモデル: 意図を宣言し、コンパイラがブロックステート、座標、物理を解決する。
- 三層 IR (Intent → Semantic/Theme → block-array pivot)、フェーズ順評価。
- 構文: 先頭キーワード + 必須の `key=value`、セレクタ、任意ヘッダ (`@cairn`, `@requires`,
  `@intended_targets`)。
- ブロックステート: デフォルトは導出、override-promotion、`intent_state` / `resolved_state`。
- マテリアル & テーマ: `mat_slot` スロット、二段の正規語彙、CSS 的なテーマバインディング。
- エンティティ: ファーストクラスの装飾エンティティと汎用 `spawn`、アンカー規約。
- コンポーネント、編集 (安定アドレス + パッチ文法)、複数建築の `site` 配置。
- バージョニング & エディション: `(edition, version)` のコンパイル時ターゲット、recompile-don't-
  transcode、近い妥当値を伴う fail-loud、DataVersion を正規順序キーとする (Minecraft の日付ベース
  バージョン移行を吸収)、provenance + lockfile。
- Java/Bedrock を 1 ソースから、エディションごとのバックエンドと QC フリーの安全セルライブラリで。
- レッドストーン: 論理サブ言語 (signal graph → 合成 → place-and-route)、組み合わせ + 厳選された
  順序マクロ、ヘッドレス tick simulator による検証。
- エコシステム連携: 主要フォーマットへの書き出し、忠実な写し取りと LLM によるリフトの import。
- 評価: ヘッドレスな幾何/レッドストーン simulator が定量的な仕様反復を駆動する。
- ドキュメント: クレート別 README、
  [開発者ガイド](https://cairn.kage1020.com/development/)、
  [チュートリアル](https://cairn.kage1020.com/tutorial/)、
  [実用例](https://cairn.kage1020.com/examples/)、横断
  [用語集](https://cairn.kage1020.com/spec/glossary/)。
- ユーザー向け文書の日本語ミラー (README、CONTRIBUTING、CHANGELOG、仕様各章、用語集、
  チュートリアル、サンプル目次)。英語が source of truth。
- [`website/`](website/README.md) のドキュメントサイト (Astro + Starlight、英語 + 日本語)。
  Cloudflare Pages の <https://cairn.kage1020.com/> にデプロイ。仕様書、チュートリアル、開発者
  ガイド、サンプル目次は [`website/src/content/docs/`](website/src/content/docs/) で直接編集
  します。`cairn-lang-wasm` バインディングを将来取り込むためのプレイグラウンドプレースホルダ、
  `main` への push で自動デプロイする Cloudflare Git 連携付き。
- リリース戦略: 月次 minor (`YYYY.0M.0`) は毎月 1 日 04:17 UTC の GitHub Actions cron、
  patch (`YYYY.0M.N`) は適格コミットの `canary` push で随時。リリース PR
  (`release-plz-*` → `canary`) は人間レビューを経てマージされ、release-plz が publish を行い、
  workflow が `main` を `canary` に fast-forward することで `main` は公開済み状態のみを映す。
- ワークスペースのバージョンは `[workspace.package].version` と `[workspace.dependencies]` で
  一元管理。バイナリは Linux/macOS/Windows × `x86_64`/`aarch64` でクロスコンパイル、sigstore
  keyless で署名し GitHub Release に添付する。
- クレート接頭辞: `cairn-lang-*` (`cairn-lang-core`、`cairn-lang-cli`、`cairn-lang-nbt`、
  `cairn-lang-formats`、`cairn-lang-redstone`、`cairn-lang-lsp`、`cairn-lang-wasm`)。
  `cargo install cairn-lang-cli` でインストールされるユーザー向けバイナリ名は引き続き `cairn`。
- [spec/compatibility](https://cairn.kage1020.com/ja/spec/compatibility/) に互換性ティアを記載:
  公開面はすべて **Stable**、**Evolving**、**Internal** のいずれかに属し、各面がいつ Stable に
  昇格するかをマイルストーン別の表で明示する。
- [ロードマップ](https://cairn.kage1020.com/ja/roadmap/) を公開。M1〜M6 のマイルストーンと
  `2027.06.0` までの月別スコープを掲載。

### Changed (Java バックエンド Rust API — `cairn-lang-formats` 利用者へ影響)

- `cairn_lang_formats::JavaTarget` は `Copy` を実装しなくなった。
  `mc_version` を `&'static str` から `String` に変更し、registry pack
  から実行時に取り出した文字列を所有する形になったため、型は `Clone`
  のみ。`build_structure_tag` / `write_structure_gzip` を直接呼ぶ
  コードは値ではなく `&JavaTarget` を渡すこと。CLI のサーフェスは変更
  なし。

### Added (M1 — *source parses* の実行可能スライス)

- `cairn-lang-core::lex` — インデントを認識する lexer。トークンにバイトスパンと
  1 始まりの行/列位置を付与する。タブインデントと奇数スペースのインデントは拒否。
- `cairn-lang-core::ast` — 表層レベル AST (`Module`, `Header`, `Item`, `ThemeRule`,
  `Command`, `Arg`, `Value`, `Extra`, `Expr`)。全型に `serde::Serialize` を derive。
- `cairn-lang-core::parse` — ハンドロールの再帰下降パーサ。ヘッダ (`@cairn`, `@requires`,
  `@intended_targets`)、`theme` / `def` / `site` / `struct` ブロック、ネストされたコマンド、
  ブラケットセレクタ、センサーの `-> binding` 末尾、位置引数 (`connect a to b`)、
  `logic` / `assert truth|always` 特殊形をカバー。
- `cairn parse <file> [--format json|debug]` — `clap` derive で実装した CLI サブコマンド。
  エラー出力は `gcc`/`clang` スタイル (`error: file:line:col: メッセージ`) で、エディタの
  ジャンプ機能から直接エラー位置を開ける。
- エンドツーエンドのカバレッジ: lexer テスト 17 件、parser ユニットテスト 27 件、
  `examples/` 配下に対する `insta` スナップショット 4 件、すべての example をバイナリ経由で
  ラウンドトリップさせる CLI 統合テスト 6 件。

### 堅牢化

- Lexer は `\n` / `\r\n` / 単独 `\r` を等価に 1 つの論理改行として扱う (Windows で
  `core.autocrlf=true` の checkout でも Linux と同じく字句解析できる)。
- 列カウンタはバイトではなく Unicode スカラー値 (`char`) で進む。文字列リテラル内の
  日本語が後続トークンの列番号を破壊しない。
- `UnexpectedChar` は実際の `char` (マルチバイト UTF-8 含む) を報告する。
  以前のバイトを単純に `char` キャストしていた挙動を廃止。
- 1 コマンド行に `-> binding` 末尾は 1 つまで。2 回目の `->` は黙って上書きせず
  ハードエラー。
- `@cairn` / `@requires` / `@intended_targets` は空値を拒否、
  `@intended_targets` はリスト後の末尾トークンも拒否。
- パーサのエラーメッセージは `TokenKind` の人間向け Display を使用
  (`expected `=`, got identifier `foo``)。Rust `Debug` の生表記は露出しない。
- `ast` / `lex` / `error` の公開 enum はすべて `#[non_exhaustive]` 化。後続マイルストーンで
  variant を追加しても下流クレートの破壊的変更にならない。
- `LexError` / `ParseError` に `position()` / `user_message()` アクセサを追加。CLI や
  将来の LSP が Display 文字列を再パースせずに診断を組み立てられる。

### Changed（AST 表面 — `cairn parse` の JSON / YAML 出力に影響）

- `TruthRow.output` の JSON シリアライゼーションが整数 `0` / `1` から論理値 `true` / `false`
  に変更。`cairn parse --format json` の出力をツールから読み込み、当該フィールドを整数前提で
  扱っているコードは更新が必要。
- `Position.line` / `Position.col`、`Value::Size.w` / `Value::Size.h`、`assert always(...)`
  の `within` バウンドは Rust 側で `NonZeroU32` 化。ワイヤ上の表現は引き続き素の整数なので
  JSON / YAML 形状は変わらない。
- `@cairn` / `@requires` ヘッダの値は Rust 側で `RawVersion` / `RawRequirement` ニュータイプに
  ラップ。`serde(transparent)` なので外部消費側から見ると素の文字列のままで形状変化なし。
