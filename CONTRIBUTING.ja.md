# Cairn へのコントリビュート

> 言語: **日本語** ([English](CONTRIBUTING.md))
>
> 英語版が source of truth です。本ファイルは英語版の二次コピーで、内容差分が出た場合は英語版を正と
> します。

Cairn に興味を持っていただきありがとうございます。本プロジェクトは **設計段階** にあります。正規
[仕様書](https://cairn.kage1020.com/ja/spec/) (ソースは
[`website/src/content/docs/spec/`](website/src/content/docs/spec/)) はありますが、リファレンス実装は
まだありません。現時点で最も価値のあるコントリビュートは、設計に対する批判、具体的な提案、そして
実用例です。

## コントリビュートの仕方

- **設計議論**。仕様内の特定の判断に対する反論、抜けているケースの指摘、代替案の提案を Issue で
  受け付けます。該当する章/節を指してください。
- **実用例**。実在する建築物を `.crn` で書いてみて、言語が不便・曖昧・不十分な箇所を指摘してください。
  これが語彙の駆動力です。
- **仕様編集**。誤りの修正、表現の明確化、例の改善。各章は自己完結を保ち、相対リンクで相互参照して
  ください。
- **先行研究**。レッドストーンコンパイラ、schematic フォーマット、ボクセル/CAD の place-and-route、HDL
  合成への参照は議論で歓迎します。

## 作業言語

仕様書および本プロジェクトドキュメントの正規言語は **英語** です。翻訳は明確にラベル付けされた二次
コピーとして歓迎しますが、英語が source of truth です。日本語版のドキュメント (`*.ja.md`) は英語版の
反映を保つために随時更新します。

## 規約

- 仕様書が source of truth です。後から読み直したときに外部文脈が必要になるような、セッション固有の
  識別子、Issue/PR 番号、参照は導入しないでください。
- 定義された用語 (`intent_state` / `resolved_state`, `mat_slot`, canonical token など) を一貫して使って
  ください。新しい用語はその場限りではなく該当章で導入してください。
  [用語集](https://cairn.kage1020.com/ja/spec/glossary/) を参照。
- 設計原則は `P1`–`P5` として参照してください
  ([設計原則](https://cairn.kage1020.com/ja/spec/principles/))。
- 例は具体的かつ最小限に。エラーメッセージは「何が間違っているか / 有効な代替候補 / 推奨される修正」の
  形に揃え、自己修正ループに乗るようにしてください。

### マイルストーン / PR タグの扱い

上のセッション固有識別子禁止には意図的な例外がいくつかあり、毎回のレビューで判定し直さなくて済む
ように本節で固定します。区分は「面ごとの役割」に従います:
[CHANGELOG.md](CHANGELOG.md) と
[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/) C.2 節の表は履歴とロードマップ
語彙を保持する場で、Rust ソースと spec 散文は「実装済み挙動」を述べる場なので、特定 PR への参照
を残してはいけません。

**`MN-PRk` / `pre-MN` / `later PR` / 特定の `YYYY.MM.0` 言及を残してよい場所:**

- [CHANGELOG.md](CHANGELOG.md) / [CHANGELOG.ja.md](CHANGELOG.ja.md)。`[Unreleased]` セクション内も
  含む。`release-plz` は追記のみで既存タグは触りません。
- [ロードマップ](https://cairn.kage1020.com/ja/roadmap/)
  (`website/src/content/docs/roadmap.md` および `ja/` ミラー) — ロードマップ自体がマイルストーン
  語彙の定義元。
- [`spec/compatibility.md`](website/src/content/docs/spec/compatibility.md) C.2 節のマイルストーン
  列 (`現在 (M1 前) | M2 (minimal build) | M3 (examples work) | M5 (DX) | M6 (redstone)`) と
  英語版同等行。これは表の軸ラベルです。
- Git のリリースタグ (`v2026.MM.0`) と `release-plz.toml`。

**これらのタグを残してはいけない場所:**

- `crates/**/*.rs` の Rust ソース (コメント、docstring)。
- `website/src/content/docs/spec/**/*.md` の spec 本文および `ja/` ミラー。ただし上記 C.2 表頭は
  例外。
- [`examples/`](examples/) の `.crn` ファイル。
- README、本ファイル、その他 docs の本文。

**書き換えガイド**。PR 座標は、その PR が代理表現していた「実装事実」に置き換えます。

- 旧: `// M3-PR4 only exposes ports on door members (window / stair / roof ports land in a later PR).`
- 新: `// Ports are currently exposed only on door members. Window / stair / roof ports are reserved for a future extension.`

要は「今コードが何をするか、何を意図的にまだ含めないか」を書き、「誰がいつ入れた PR か」は書かない。
コメントが古くなった場合 (deferred 機能が後で land した場合) は、その機能を land する PR の中で
コメントも同時に更新します。

**チェック**。レビュアー (人間でもツールでも) は承認前に以下を走らせてください:

```sh
rg 'M[0-9]-PR[0-9]+|pre-M[0-9]|\blater PR\b|\bfuture PR\b' \
  --glob '!CHANGELOG*' \
  --glob '!**/compatibility.md' \
  --glob '!target/**'
```

空 hit が契約です。CI には未組み込み (リポジトリ規模上、human review で十分)。

## 確定した判断を覆したい場合

いくつかの判断は意図的に確定しています (例: 位置引数ではなく key=value、フェーズ順評価、
recompile-don't-transcode、サイレントな置換ではなく fail-loud)。これを覆したい場合は次の内容を含む
Issue を立ててください:

1. その判断と仕様内の所在。
2. それで扱えない具体ケース。
3. 代替案 (構文/IR/メッセージの例つき)。
4. 評価メトリクスへの影響
   ([評価フレームワーク](https://cairn.kage1020.com/ja/spec/evaluation/))。

## ブランチとプルリクエスト

Cairn は **`canary` トランク + `main` リリースポインタ** 構成を採用します。進行中のすべての作業は
`canary` に乗り、`main` はリリースが公開された直後にのみ自動で更新されます。`main` の履歴は公開済み
リリースの列そのものになります。

### ブランチ

| ブランチ | 用途 | 寿命 |
|---|---|---|
| `canary` | トランク。機能、修正、ドキュメント、`release-plz-*` ローリング release PR がすべて乗る。保護ブランチ。 | 永続 |
| `main` | リリース済み状態。リリース成功直後に `canary` に自動で fast-forward される。保護ブランチ。直接 push 不可、PR 受付なし。 | 永続 |
| `<type>/<short-kebab>` | 単一の変更のための作業ブランチ。`canary` 宛。 | PR マージまで、マージ後削除 |
| `release-plz-*` | 月次 minor と patch のために `release-plz` が `canary` に対して自動で開く。 | PR マージまで |

`<type>` は、その作業がマージ時に乗ることになる Conventional Commits の type に揃えてください
(`feat/parser-lexer`、`fix/wall-corner-shape`、`docs/roadmap-2027`、`refactor/ir-pivot`)。

### プルリクエスト

- **すべての PR は `canary` を宛先にする**。`main` 宛の PR は受け付けません。`main` はリリース
  パイプラインによってのみ更新されます。
- **PR タイトルは [Conventional Commits](https://www.conventionalcommits.org/) 形式とする (MUST)**。
  例: `feat(core): add lexer`、`fix(formats): correct big-endian NBT length`、
  `docs(spec): clarify §6.3`、`feat(redstone)!: rewrite tick simulator`。feature ブランチ内の個別
  コミットは自由形式で構いません。
- **マージ方式は squash merge のみ**。PR タイトルが `canary` のコミットメッセージとして残り、
  `release-plz` がそれを `release-plz.toml` の `release_commits` で解析して patch リリースの要否を
  判断します。
- breaking change には `!` 接尾辞 (例: `feat(core)!: replace lexer`) を付けてください。これにより
  [互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/) C.3 節の "Breaking changes"
  扱いになります。
- メンテナ 1 名の承認を必須とし、CI (fmt + clippy + test の Linux/macOS/Windows 3 OS) はすべて
  通過してからマージします。
- リリース PR (`release-plz-*` → `canary`) も同じレビュー規約に従います。月次 minor PR は毎月 1 日
  に cron が立ち上げ、人間レビュー後にマージされます。マージで publish が走り、同時に `main` が
  fast-forward されます。

Cairn で使う Conventional Commits の type:

| type | 用途 | patch リリースを誘発するか |
|---|---|---|
| `feat` | 新機能、新しい公開 API、新サブコマンド | する |
| `fix` | spec に挙動を合わせるバグ修正 | する |
| `perf` | 性能改善 | する |
| `refactor` | 挙動を変えない内部リファクタリング | する |
| `build` | ビルド、パッケージング、Cargo 依存 | する |
| `docs` | ドキュメント、spec 散文、README、サンプル | しない |
| `test` | テストコードのみ | しない |
| `ci` | GitHub Actions、release-plz、workflow 設定 | しない |
| `chore` | 利用者に届かない雑多な変更 | しない |
| `style` | フォーマット / lint のみ | しない |

括弧内のスコープは影響する crate や spec 領域を示します: `feat(core)`、`fix(nbt)`、`docs(spec)`、
`build(deps)`。

## バージョニング

Cairn は日付ベースバージョニング (CalVer) `YYYY.0M[.PATCH]` を採用します。主要な変更は
[CHANGELOG.md](CHANGELOG.md) に記録されます。バージョン番号ではなく
[互換性ティア](https://cairn.kage1020.com/ja/spec/compatibility/) が各面の互換契約を定めます。

## Code of Conduct

本プロジェクトは [Contributor Covenant](CODE_OF_CONDUCT.md) に従います。参加にあたっては本規約の
順守が期待されます。
