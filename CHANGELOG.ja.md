# 変更履歴

> 言語: **日本語** ([English](CHANGELOG.md))
>
> 英語版が source of truth です。

書式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に従います (release-plz が
リリースエントリを綺麗に追記できるようにするため)。Cairn は日付ベースバージョニング (CalVer)
`YYYY.M[.PATCH]` を採用します。これは「言語仕様 + リファレンスコンパイラ + 標準ライブラリ +
レジストリ/制約パック」をまとめたバンドルのバージョンであり、Minecraft のターゲットバージョンとは
別軸です。

## [Unreleased]

### 追加

- Redstone Placement IR と `cairn synth --stage placement
  --edition <java|bedrock>`（M6-PR4）— M6 redstone-simulates
  パイプラインの4枚目。`cairn-lang-redstone` に
  `compile_placement(&ScopedEditionNetlistIr, &IntentModule)`
  エントリポイントを追加し、M6-PR3 の Edition Netlist IR を走査して
  各 edition タグ付きセルをスコープの `circuit region=` 予約領域に
  配置する — `spec/redstone` §14.5 の5段パイプライン
  （Placement → Steiner routing → Delay insertion → Crossing
  legalization → Edition legalization）の第1段。セルは
  Edition Netlist IR が既に持つトポロジカル順（`cells[i]` 内の
  `NetRef::Cell(j)` は `j < i` を満たす）で並び、`x = i`, `y = 0`,
  `z = 0` に固定される — 1D 配置で、クロスやファンアウトが絡む
  pseudo-2.5D へのリフトは routing pass 側の担当。§14.4 の
  「delay は routed wire length から決まる」に従い、`PlacedCellNode`
  の `wire_length` / `delay_ticks` は `Option` として予約され今段では
  常に `None` — 続く PR での値埋めは field write であって schema
  変更ではないので、下流 JSON consumer は今日から stable な wire
  shape を見る。`CircuitRegionReservation` は `region=<label>
  void=<N>` の予約情報と、囲むスコープの `size=WxH` foot print を
  Intent IR から丸ごとコピーして持つので、routing pass が消費する
  型は 1 つに集約される。`spec/lint` §11 の self-correction
  triple に沿った 2 つの新規 diagnostic コード:
  `E_NO_CIRCUIT_REGION` は「配置すべきセルがあるのに `circuit
  region=` 行が無い（あるいは囲むスコープに `size=` が無い）」
  ケースを、`E_ROUTE_CONGESTION` は「netlist の必要面積が予約領域を
  上回った」ケースを検出する。後者の primary は比率と予約 shape を
  引用する（`synthesized netlist needs ~1.3x the reserved area
  (void=1, region 3x3)`）— footer は §14.5 が挙げる 3 つの修正
  （`increase void, enlarge region, or split into multiple
  circuit blocks`）をそのまま提示する。congestion / missing-region で
  失敗したスコープは出力から drop されるので、下流 consumer が
  partial layout を silent に受け取ることは無い（synth pass の
  未束縛シグナル cascade 抑制と同じ fail-loud ポリシー）。
  `cairn-lang-core` には `intent::circuit_regions(&IntentModule)
  -> Vec<CircuitRegion>` API を薄く追加 — 既に検証済みの
  `circuit region=` fixture を Intent IR から取り出す共通エントリ
  で、redstone crate が `member.intent_state` を再度パースする
  必要が無い。block-array pass 側の `recognize_circuit_region` は
  引き続き per-shape の `W_DEFERRED_MEMBER` を担当するので、
  2 consumer が同じ source line に対して diagnostic を二重発火する
  ことは無い。CLI の `cairn synth --stage` に `placement` 値を追加。
  `--edition <java|bedrock>` フラグは `edition` と同様に必須で、
  edition-neutral な `logic` / `netlist` stage では引き続き exit 2
  で拒否される。今回のスコープ外: Steiner routing / wire length
  確定、delay insertion（リピータバッファ）、crossing legalization、
  edition legalization、block-array voxel 落とし、physical tile
  （3層目）cell library、tick simulator、`assert truth|always|
  latency` の評価、シーケンシャルマクロ (`latch` / `pulse` /
  `delay` / `edge_*` / `counter`)、QC/BUD 拒否
  (`E_NO_PORTABLE_IMPL`) — それぞれ後続 PR が本 PR で確定した
  Placement IR shape の上に積む。
- Redstone Edition Netlist IR と `cairn synth --stage edition
  --edition <java|bedrock>`（M6-PR3）— M6 redstone-simulates
  パイプラインの3枚目。`cairn-lang-redstone` に
  `compile_edition_netlist(&ScopedNetlistIr, Edition)` エントリポイントを
  追加し、M6-PR2 の Netlist IR を走査して各 `LogicalCell` を
  ターゲットエディションでの実装へ落とす — `spec/redstone` §14.6 の
  3層セルライブラリ (`Logical Cell → Edition Cell → Physical Tile`)
  の中段。純粋な構造リライトで、driver / `NetRef` / inputs / outputs /
  `signal_defs` は源の Netlist IR から丸ごとコピーされ、トポロジカル
  不変量 (`cells[i]` 内の `NetRef::Cell(j)` は `j < i`) は構成で保存される。
  `EditionCell` はターゲットエディションと物理実装ファミリの両方を名前に
  持ち、Java AND セルを Bedrock トーチタイルに誤って組み合わせるバグは
  ランタイムエラーではなく型エラーになる — `and` は Java `ComparatorAnd`
  / Bedrock `TorchAnd`、`or` は Java `RepeaterOr` / Bedrock `TorchOr`、
  `not` は Java / Bedrock 双方の `InverterTorch`（構造は共通だが後段の
  配置器が正しいタイル向きを選べるようエディションタグは保持、§14.6 の
  エディション吸収済み差分の一つ「orientation」に相当）。パーサ未到達な
  セル (`xor` / `nand` / `nor` / `mux`) は edition-agnostic な catch-all
  ではなく、per-edition の `*Unpinned` プレースホルダバリアント
  (`JavaXorUnpinned` / `BedrockXorUnpinned` / ...) にそれぞれ落ちるので、
  コンテナ / セルの edition 整合は命名で強制され、後続のパーサ変更は
  「対応する 1 match arm で placeholder をピン留め名にリネーム」で済む。
  `(Edition, LogicalCell)` の照合はワイルドカード無しで完全網羅なので、
  第 3 の `Edition` バリアント（Education）追加時は全マッピング箇所で
  コンパイルエラーになり、silent な Java フォールスルーは起こらない。
  §14.4 / §14.8 のとおり Edition Netlist IR も delay を持たず、リピータ
  挿入は Placement IR 側で行う。CSE / 巡回検出 / 未束縛シグナル報告は
  M6-PR1 で、Logical Cell 選択は M6-PR2 で済んでいるので、この pass も
  独自の diagnostic を出さない純構造書き換え。CLI の `cairn synth
  --stage` に `edition` 値を追加、同モードでは `--edition <java|bedrock>`
  フラグが必須で、`logic` / `netlist` に渡された場合は exit 2 で拒否する
  (silent に無視すると stage-vs-edition の軸が呼び出し側の頭の中で
  ずれるため)。今回のスコープ外: place-and-route、リピータ挿入、
  tick simulator、`assert truth|always|latency` の評価、シーケンシャル
  マクロ (`latch` / `pulse` / `delay` / `edge_*` / `counter`)、
  `circuit region=... void=N` の congestion 検出 (`E_ROUTE_CONGESTION`)、
  QC/BUD 拒否 (`E_NO_PORTABLE_IMPL`) — それぞれ後続で本 PR が確定した
  Edition Netlist IR shape の上に積む。
- Redstone 組合論理 Netlist IR と `cairn synth --stage netlist`（M6-PR2）—
  M6 redstone-simulates パイプラインの2枚目。`cairn-lang-redstone` に
  `compile_netlist(&ScopedLogicIr)` エントリポイントを追加し、
  M6-PR1 で得た Logic IR の各 `GateNode` を `LogicalCell`
  （現状は `and` / `or` / `not`。`xor` / `nand` / `nor` / `mux` は
  Logic IR 側と同じく enum に予約）でタグ付けした `CellNode` に書き換える。
  セルはカノニカルなポート順 (`[A, B]` / `[A]` / `[Sel, A, B]`) で
  driver を保持するので、後段のシミュレータや配置器は `PortName` を
  見ずに位置インデックスで扱える。`NetRef` は Logic IR の arena 型
  `SignalRef` と同型で、`cells[i]` に含まれる全 `NetRef::Cell(j)` が
  `j < i` を満たすトポロジカル不変量を単一の forward walk で保存する。
  `spec/redstone` §14.6 に従い、cell library の3層構造
  (`Logical Cell → Edition Cell → Physical Tile`) のうち最上段のみをここで選び、
  Java `ComparatorAND` / Bedrock `TorchAND` の Edition Cell 選択は
  後段に譲るため IR は edition-neutral のまま。§14.4 / §14.8 のとおり
  Netlist IR も delay を持たず、リピータ挿入は Placement IR 段まで
  行わない。CSE / 巡回検出 / 未束縛シグナル報告は M6-PR1 で済んでいるので
  netlist pass は独自の diagnostic を出さない純粋な構造書き換え。
  CLI の `cairn synth` に `--stage <logic|netlist>` フラグを追加（既定は
  後方互換のため `logic`）、依然として `--experimental-logic-synth`
  ゲート配下。今後の placement / route / simulator 段もこのフラグに
  乗せていくのでサブコマンドは増やさない。今回のスコープ外:
  Edition Cell 選択、place-and-route、tick simulator、
  `assert truth|always|latency` の評価、シーケンシャルマクロ
  (`latch` / `pulse` / `delay` / `edge_*` / `counter`)、
  `circuit region=... void=N` の congestion 検出（`E_ROUTE_CONGESTION`）、
  QC/BUD 拒否（`E_NO_PORTABLE_IMPL`）— それぞれ後続 PR で本 PR が確定した
  Netlist IR shape の上に積む。
- Redstone 組合論理 Logic IR と `cairn synth`（M6-PR1）— M6 redstone
  simulates パイプラインの最初のスライス。`cairn-lang-redstone` に
  `synthesize(&IntentModule)` エントリポイントを追加し、全ての
  struct / def / site body を走査してセンサ束縛 (`pressure_plate ...
  -> sig.X` および将来的な `-> sig.Y` 尾を持つ任意のセンサ) を
  `InputPort` として、アクチュエータ引数 (`opened_by=` / `powered_by=`
  / `lit_by=` / `fired_by=`、`spec/redstone` §14.2 準拠) を
  `OutputPort` として収集し、各 `logic sig.X = <expr>` 行をトポロジ
  順に並んだ `GateNode` DAG へ lower する。組合論理プリミティブは
  `and` / `or` / `not` を synth 経路に含め（現在の AST から到達可能
  な範囲）、`xor` / `nand` / `nor` / `mux` は `GateKind` enum 上に
  用意して後続 PR での関数呼出構文サポートを受け入れる準備を整える。
  共通部分式除去 (CSE) により、2 行の `logic` が同じ `sig.a or sig.b`
  を書いた場合は 1 個の OR ゲートに統合され、下流の placement が
  ソースの意図しないファンアウトコストを払わない設計。診断コードは
  4 種を新設し、`spec/lint` §11 の self-correction triple 形式に
  従う: `E_LOGIC_UNBOUND_SIGNAL`（センサ・先行 `logic` のいずれにも
  定義されていない参照、`Valid signals in scope: ...` 脚注で候補
  一覧を提示）、`E_LOGIC_MULTIPLE_DRIVERS`（2 行の `logic` で同一
  LHS または `logic` LHS がセンサと衝突）、`E_LOGIC_CYCLE`
  （組合論理依存チェーンが自己ループを構成）、`W_LOGIC_UNUSED_SIGNAL`
  （LHS がアクチュエータからも下流 `logic` からも参照されない
  bare-ref / gate 生成 bind）。カスケード抑制のため failed-LHS
  セットを維持し、根本原因 1 件に対する診断が消費側で複製されない
  ようにしている。CLI 側には internal-tier の
  `cairn synth <file> --experimental-logic-synth` サブコマンドが載り、
  スコープ単位の Logic IR を JSON で dump する（pipeline が stable tier
  に達するまでフラグは必須）。本 PR のスコープ外: Netlist IR、
  cell library、place-and-route、tick simulator、`assert truth|always`
  評価、sequential macros（`latch` / `pulse` / `delay` / `edge_*` /
  `counter`）— いずれも本 PR で確定した Logic IR shape を土台にする
  後続 PR で追加する。
- Cairn VS Code 拡張機能と `cairn-lsp` バイナリ配布（M5-PR3）— M5
  developer experience マイルストーンをクローズする。新規
  `editors/vscode/` TypeScript 拡張（本 PR では Marketplace ではなく
  `.vsix` 単位で配布）は `onLanguage:cairn` /
  `workspaceContains:**/*.crn` で activate し、`cairn.serverPath` 設定
  または OS の `PATH` から `cairn-lsp` を解決する（見つからない場合は
  silent no-op せず、Release ページへのリンク付き通知 1 件を出す）。
  `vscode-languageclient@9` を介して stdio 上で spawn し、activate 時に
  サーバの `--version` 文字列を Output panel に記録するので、バグ報告に
  バージョンが自然と含まれる。最小 TextMate 文法（`source.cairn`）は
  コメント (`#`)、ディレクティブ (`@cairn`/`@requires`/`@intended_targets`)、
  トップレベルキーワード (`theme`/`def`/`site`/`struct`)、メンバ
  キーワード（`cairn-lang-core::intent::known_keywords` のミラー:
  `floor`/`walls`/`door`/`window`/`roof`/`stair`/`level`/`pressure_plate`/
  `circuit`/`place`/`connect`）、material token (`@name.dotted`)、
  attribute key (`k=`)、`->` slot binding 矢印、および文字列を色付けする。
  シンタックスは M5-PR1/PR2 で既に届いた LSP 由来の診断・補完の隣で動く。
  `cairn-lsp` は小さな `--version`（および `-h`/`--help`）フラグを獲得し
  — `cairn --version` に整合、`crates/cairn-lang-lsp/tests/version_flag.rs`
  の新規統合テストで固定 —、拡張機能とサポート triage が起動せずとも
  サーバを識別できる。`.github/workflows/publish.yml` は 6 リリース
  ターゲットすべてで `cairn` に加えて `cairn-lsp` をクロスコンパイルし、
  1 アーカイブに両バイナリを同梱する。既存の sigstore 署名がペアを覆う
  ので、アセット数・`.sha256`・`.sigstore` レイアウトは変わらない。
  スコープ外: Marketplace / Open VSX 公開、`.vsix` へのバイナリ同梱、
  semantic-tokens プロバイダ — いずれも M6 または後続 PR に持ち越す。
- `cairn-lsp` completion（M5-PR2）— 言語の closed vocabulary に対する
  `textDocument/completion`。`initialize` でトリガー文字 `@`・`=`・`.`
  とともに広告される。カーソルの 4 コンテキストを認識する: 行頭キーワード
  （トップレベルの `theme`/`def`/`site`/`struct`、`struct`/`def`/`site`
  ボディ内のメンバーコマンド、`theme` ボディ内の `slot` + セレクタ
  キーワード）、`mat_slot=` の値（ドキュメント内の全テーマが宣言する
  slot 名の union — `_java`/`_bedrock` 変種テーマも自然に union され、
  edition 未指定の `cairn check` の slot 存在検査と同じ扱い）、そして
  `@` 材料トークン（組み込みレジストリの union、java ∪ bedrock）:
  各 abstract token は解決先の canonical id を item detail に持ち、
  加えてカタログ value 列から重複排除した canonical id 群を返す
  （canonical の完全な語彙はまだ存在しないレジストリ blocks テーブル
  待ち）。コンテキスト判定は行ローカルなテキストヒューリスティック —
  Cairn は厳密に行指向なので行プレフィックスが文法的に十分 — で、
  キーストローク途中の常態であるパース不能なドキュメントでも補完が
  動き続ける。`slot NAME -> TARGET` の行スキャンは全出荷サンプルに
  対してパーサの見解と一致することをドリフトガードテストが固定する。
  各 item は `TextEdit`（UTF-16 で正しい range）でカーソル下の部分
  トークンを置換し、宣言順/カタログ順を凍結する `sortText` を持つ。
  プレフィックスフィルタはクライアントに委ね、closed set が無い位置
  （コメント、自由形式の値、ヘッダディレクティブ）は語彙を捏造せず
  空を返す（principles P3）。サーバーは `DocumentStore`（URI → 最終
  同期テキスト）を保持するようになり、変更通知の外でもドキュメントを
  読めるようになった。未 open のドキュメント、またはドキュメント末尾を
  1 行超えて外れた position へのリクエストは `InvalidParams` で loud に
  拒否する（1 行超過までは応答する — `didChange` とリクエストは競合
  し得る）。`cairn-lang-lsp` はレジストリパックのため
  `cairn-lang-formats` に依存するようになった。
- `cairn-lsp`（M5-PR1）— 言語サーバーの最初の動作版。`cairn-lang-lsp` の
  `[[bin]]` ターゲットとして標準 LSP を stdio 上で話す。`initialize` で
  全文同期（full-content sync）を広告し、`didOpen`/`didChange` のたびに
  `cairn check` と同じ `parse → lower → check` パイプライン（edition 未指定
  のため slot 存在検査はエディション別テーマ変種を union）を実行して
  `textDocument/publishDiagnostics` を push する。`didClose` は空集合を
  publish して古い squiggle を残さない。check の所見は安定コード
  `E_*`/`W_*` 文字列を LSP `code` フィールドに、`source: "cairn"` とともに
  保持し、span 付き note は `relatedInformation` に、span なし note
  （valid candidates / Suggested fix のフッタ）は `note:` 行として
  message に畳み込まれ、self-correction triple がそのままエディタへ届く。
  構造化 `data` ペイロードは将来の quick-fix 向けにパススルーされる。
  parse/lex 失敗は check パスを pre-empt し、当該行の行末までを range と
  する error diagnostic をちょうど 1 件だけ生成する。位置は新設の
  `line_index::LineIndex` が core の byte span からプロトコルの 0-based
  行 / UTF-16 コードユニット座標へ変換し、UTF-16 の知識を
  `cairn-lang-core` の外に保つ。トランスポートは `lsp-server` +
  `lsp-types`（rust-analyzer の同期 stdio 基盤 — 非同期ランタイムは
  ワークスペースに入らない）。completion は M5-PR2 として続いた（上記）。
  VS Code 拡張が M5 の残り（M5-PR3）で、publish パイプラインへの
  バイナリ配布は拡張と同時に着地する。
- `cairn-lang-formats::portability` — `cairn info` の
  `edition_portability` 軸を支えるパレットエントリ単位のポータビリティ
  カウンタ（spec versioning-editions §10.5）。`portability_for_bedrock` は
  air 以外のパレットエントリを `bedrock_state::translate_states` に通し、
  結果を `{portable, degraded, unsupported}` に集計します — 劣化ノートなしの
  変換は portable、劣化ノート付き（現状は stair の `shape != straight`）は
  degraded、`BedrockStateError` は unsupported として数えます。
  `portability_for_java` は air 以外を全て portable として報告します
  （§10.3 の「Java is the base」に従う）。カウント粒度はパレットエントリ
  単位で、`.mcstructure` ライターが実際に書き出す粒度と一致します —
  lowering が複数の異なるパレットエントリを intern するメンバー（コーナー
  stair を含む切妻屋根など）は、エントリ単位で 1 行ずつ寄与します。
- `cairn-lang-core::Edition` — Resolver と CLI で共有される横断的な
  エディション marker (`Java` / `Bedrock`)。将来 3 番目のエディションを
  追加するときも 1 か所に variant を足すだけで済みます。`FromStr` は
  未知のエディション文字列を loud に拒否し
  (`unknown edition `{input}`. Valid: java, bedrock. Fix: ...`)、
  `cairn info --editions foo` は dry-run lowering を走らせる前に exit 2 で
  拒否するようになりました（未知のエディションが 0 埋めの portability 行に
  無音でフォワードされる従来の穴を塞ぐ）。
- `cairn-lang-core::resolve` — per-edition テーマフォールバック
  （spec versioning-editions §10.7 代替階層 #2）。名前が `_java` /
  `_bedrock` で終わるテーマは論理テーマの edition 変種と扱われます
  （`theme shop_java:` と `theme shop_bedrock:` は論理名 `shop` を共有）。
  `resolve` は `edition: Option<Edition>` を引数に取るようになり、
  struct/def スコープごとに対応する変種を自動選択します。指定された
  variant がない場合は同一論理名の未サフィックステーマにフォールバック
  します。既存の未サフィックステーマ（`theme medieval:` のような
  従来形）は両エディションで従来通り解決されます。`resolve(ir, None)`
  — エディション未指定の `cairn check` 経路 — では両 variant のスロット
  名を union し、片方の variant にしか宣言されていないスロットへの
  `mat_slot=NAME` 参照が誤って `E_UNRESOLVED_SLOT` を出さないように
  します。selector マッチは選ばれた variant にのみスコープされ、§7 の
  per-theme DI コントラクトを維持します。`resolve(&ir)` の呼び出しは
  `resolve(&ir, edition)` に、`check(&module, &ir)` は
  `check(&module, &ir, edition)` に移行しました。
- `cairn info --editions java,bedrock` は `degraded` / `unsupported`
  列を per-edition dry-run lowering から生成するようになりました
  （リクエストされたエディションごとに `lower_to_block_array` を 1 回走らせ、
  対応する built-in pack で materials を解決し、パレットを
  `portability_for_*` に流す）。ハードコードされたゼロは廃止です。
  `themed-tower.crn` では軒の `shape=outer_left` stair が
  `Bedrock: degraded: >=1` として表面化し、`cottage.crn` は両軸とも 0 の
  ままです。`EditionPortability` の JSON / テキスト形状は変わらないため、
  `--format json` の消費者はワイヤ破壊なく実データを受け取ります。
  `cairn-lang-core::resolve::compute_axes` は per-edition 集計を呼び手から
  受け取る `Vec<EditionPortability>` 引数を持つようになりました
  （`core` は `formats` に依存しないため、集計は CLI 層で作って渡す形）。
- `cairn check --edition java|bedrock` — オプショナルな edition ピン。
  指定された variant にしか宣言されていないスロットへの `mat_slot=X`
  参照は `E_UNRESOLVED_SLOT` として発火します。`--edition` 未指定時は
  Resolver が両 variant のスロット名を union するため、後にどちらの
  エディションでコンパイルされてもファイルは `check` を通過します。
- `examples/edition-fallback.crn`（+ `.crn.lock`） — 論理テーマ `shop` を
  `shop_java`（`floating_text` スロットを `@sign.oak` にバインド）と
  `shop_bedrock`（`@sign.oak_wall` にバインド）の 2 variant に分割し、
  spec §10.7 代替階層 #2 をエンドツーエンドで示す例。spec の
  例示的な `text_display` パターンが必要とするエンティティ概念を導入せず、
  既存の block-only パイプラインだけで完結します。Java コンパイルは
  palette に `oak_sign`、Bedrock コンパイルは `oak_wall_sign` を書き出します。
  新しい material token `sign.oak` / `sign.oak_wall` は両 built-in pack に
  追加されました。
- `cairn-lang-formats::bedrock_state` — Bedrock バックエンド向けの
  per-edition blockstate 変換。`.mcstructure` ライターが後続とした対応です。
  `translate_states` は **stair family**（現状 lowering がプロパティ付きで
  intern する唯一のブロック種）を、Java の `facing` / `half` 文字列
  プロパティから Bedrock の型付き `states` へマップします —
  `weirdo_direction`（`east=0, west=1, south=2, north=3`、wiki の
  `Stairs/BS` 一覧で検証）と `upside_down_bit`（`top=1, bottom=0`）。stair の
  `shape` に対応する Bedrock 状態はないため、`straight`（Bedrock の既定）は
  劣化なしで落とし、コーナー shape は `ParityNote` として落とし、CLI が
  `warning[W_INTENT_DEGRADED]` として表示します（spec versioning-editions
  §10.3 `dropped_states: [shape]` / §10.7。§10.4 の無音削除禁止を満たす）。
  マップ対象外の family でプロパティを持つブロックや、Java ドメイン外の
  stair 状態値は、従来通り自己修正トリプル付きで fail-loud します。
  `build_mcstructure_tag` は `(Compound, Vec<ParityNote>)` を返すようになり、
  palette entry ごとに空 compound ではなく実際の `states` を書き出します。
  `cottage.crn`（すべて `straight` の切妻屋根）は `--edition bedrock` で
  クリーンにコンパイルされ、`themed-tower.crn` は非 straight の軒コーナーで
  `W_INTENT_DEGRADED` を 1 件出してコンパイルされます。
  `BedrockStructureError::StatefulPaletteEntry` のハードエラーは透過的な
  `BedrockStructureError::State(BedrockStateError)` に置き換わりました。
- `cairn-lang-nbt::bedrock::write_bedrock_uncompressed` — Bedrock の
  非圧縮 `.mcstructure` 向けリトルエンディアン NBT ライター。バイト列
  エンコーダを Endian パラメータ化した単一のコア (`writer.rs`) に抽出し
  Java ライターと共有したため、両ダイアレクトはスカラーのバイト順のみ
  異なり、検証ルール (`InvalidString` / `HeterogeneousList` /
  `LengthOverflow`) が乖離しなくなりました。Java 側の公開 API
  (`write_java_uncompressed` / `write_java_gzip`) とエラー型は不変です。
- `cairn-lang-formats::bedrock_structure` — `java_structure` を鏡写しに
  した `.mcstructure` シリアライザ。`build_mcstructure_tag` が
  `BlockArray` を Bedrock のルート形状 (`format_version`、`size`、
  Z 最速の 2 層 `structure.block_indices` で第 2 層は `-1` 埋めの
  waterlog 層、`{ name, states, version }` からなる
  `structure.palette.default.block_palette`、`structure_world_origin`)
  に lower し、`write_mcstructure` が非圧縮で書き出します。この初回分は
  **stateless な palette のみ**を対象とし、blockstate プロパティを持つ
  palette entry は `BedrockStructureError::StatefulPaletteEntry` で
  fail-loud します (spec versioning-editions §10.4 は無音の置換/削除を
  禁止)。メッセージは自己修正トリプルを持ちます。per-edition の state
  マッピング (`facing` / `half` / `shape`) は後続で対応します。
- `cairn-lang-formats` の組み込み Bedrock レジストリパック
  (`registry-data/bedrock/`)、`builtin_bedrock` / `load_builtin_bedrock`、
  `data_version::{BedrockTarget, resolve_bedrock_target}`。パックの
  `data_versions` 列は `.mcstructure` の block-palette `version` 整数
  (`(major << 24) | (minor << 16) | (patch << 8) | revision`) を保持し、
  materials カタログは Java パックが lift するのと同じ abstract token を
  カバーします。ターゲット解決は Java パックの機構 (`latest` エイリアス、
  Damerau-Levenshtein の suggestion) を再利用し、`UnsupportedTarget` は
  参照したバージョンテーブルのエディション名を含めるようになりました。
- `cairn compile --edition bedrock` が `.mcstructure` 成果物と、
  `target.edition = bedrock`・`data_version = block_version`・
  `registry_pack_hash` に Bedrock パックのバイトを固定した lockfile を
  書き出します。Java `.nbt` 経路はバイト単位で不変です。`ResolvedTarget`
  enum がエディションを成果物名 (`OutputExt`)・タグ構築・ライター
  (gzip か非圧縮か)・lockfile へと通すため、将来のエディション追加が
  1 箇所で済みます。

- `cairn-lang-core::block_array::lower` — `level y=N` ブロックが
  block-array lowering の phase-bucket に参加するようになりました。
  新しい `flatten_members` 事前パスが各 `level` を
  `(y_offset, child)` ペアに展開するため、`level` 直下にネストされた
  `walls` / `door` / `window` / `stair` は authored `y` を level の
  `y=` 分ずらして massing / openings / envelope の各フェーズに届きます。
  `max_wall_height` は `max_wall_top` に改名し、フラット化後のリストを
  集約するようになったので、`level y=N walls id=X height=H` は
  `y = N + H` まで struct のロープラン (roof plane) を伸ばします。
  level のネスト (2 段以上) は `W_DEFERRED_MEMBER` で defer します。
- `cairn-lang-core::block_array::lower` — `MemberRole::Stair` を最小
  実装 (`fill_stair`) しました。`themed-tower.crn` の軒 (eave) パターン
  (`kind=stairs`、`side=front|back|left|right`、`half=top|bottom`、
  `facing=out|in`、`shape=straight|outer_left|outer_right`、`y=`) を
  カバーします。stair band は壁のオーバーハング行 (壁の外側 1 voxel) に
  `y = y_offset + local_y` で並び、base id は解決された `mat_slot=` の
  BlockState から取得します (未解決なら `spruce_stairs`)。それ以外の
  `kind=` / `half=` / `facing=` / `shape=` は該当箇所を指す
  `W_DEFERRED_MEMBER` で defer します。
- `cairn-lang-core::block_array::lower` — `fill_window` が themed-tower
  の 2 階矢狭間 (arrow-slit) パターン `repeat=N step=M` をサポートします。
  同じ矩形を `N` 回、`step` voxel ずつずらして塗ります。`repeat` を
  省略すると 1 とみなし、`repeat>=2 step=0` は defer します
  (インスタンスが重なるため)。`mat_slot=` を持たない window は無音の
  drop ではなく空気を彫るようになったので、`class=arrow_slit` のスリット
  が壁に本物の穴を空けます。`mat_slot=` 明示のある window は変化なし。
- `crates/cairn-lang-formats/tests/themed_tower_level_lower.rs` — 新規
  統合テスト。`examples/themed-tower.crn` を built-in レジストリパック
  経由で end-to-end に lower し、dims、palette (`dark_oak_stairs` /
  `dark_oak_planks` を含む解決済み 5 種)、2 階の壁リング、軒 stair band、
  矢狭間の空気彫りパターン、そして「`W_DEFERRED_MEMBER` 0 件」の契約を
  pin します。materials resolver に built-in パックが必要で、
  `cairn-lang-core` が `cairn-lang-formats` に依存できない (循環)
  ため配置は `cairn-lang-formats` の tests/。
- `cairn-lang-core::block_array::lower` — `MemberRole::Circuit` を
  最小認識 (`recognize_circuit_region`) しました。`redstone-door.crn` の
  `circuit region=floor void=2` のように、`region=<label>` (領域名を
  指す `Ident` または `Str`) と `void=<N>` (`u32` かつ `N >= 1` の
  service-layer 高さ) を持つ回路領域マーカーを surface 形式のみ
  検査し、voxel は一切置きません (spec/redstone.md §14.5 / §14.8 で
  dust / repeater / cell の配置は `logic_synth → logic_place →
  logic_route` に委ねられているため)。`region=` 欠落、`region=` が
  非 label 種別 (integer / boolean / size / token / reference / list)、
  `region=""` (空文字列)、`void=` 欠落、`void=0`、`void` が `u32` に
  収まらない — これらは対象キーを指す primary 付きで
  `W_DEFERRED_MEMBER` を発火します (kind mismatch の primary には
  該当 kind 名も含みます)。
- `crates/cairn-lang-formats/tests/redstone_door_pressure_plate_lower.rs`
  — 新規 `redstone_door_circuit_line_emits_no_deferred_warning` テスト。
  `circuit region=floor void=2` 行に対する
  「`W_DEFERRED_MEMBER` 0 件」契約を pin します
  (隣接する `pressure_plate` の 0 件テストと同じ形)。
- `cairn-lang-core::block_array::lower` — `MemberRole::Door` のうち
  surface 行が selector 形式 (`door[id=X] opened_by=…`) のものを、
  phase-bucket に入る前に **アクチュエータパッチ** として認識する
  ようにしました。新設の `recognize_actuator_patch` ガードが
  patch 行を `openings` フェーズから外すので、`carve_door` の
  `side_of` が patch 行に対して「`side=` 欠落」を誤検知しません。
  レコグナイザは surface 形式のみ (spec/redstone.md §14.2) を検査
  します: `[selector]` は物理 door を指す `id=<label>` を持たねばならず
  (level ネストされた door も選択可能)、`opened_by=` は 2 セグメント
  の `sig.<name>` `DotRef` に解決しなければなりません。`id=` の
  欠落・非 label 値・未宣言 id、`opened_by=` の欠落、`sig.<name>` 以外の
  `opened_by=` 値は、それぞれ対象キーを指す primary 付きで
  `W_DEFERRED_MEMBER` を発火します。未知 id の primary は同じ
  スコープに宣言されている物理 door の id をすべて列挙するので、
  near-miss を目視で発見できます。今回対応するのは
  `door[id=…] opened_by=` のみで、`lamp lit_by=` / `piston powered_by=`
  / `dispenser fired_by=` は各キーワードが役割テーブルに載る PR で
  追加予定です。selector 内の未知属性・intent 側の未知キーも silent
  受理せず defer するため、将来 `powered_by=` が実装されたときに
  既存ソースの意味を暗黙に変えることを防ぎます。これで
  `redstone-door.crn` のアクチュエータパッチ行
  `door[id=front] opened_by=sig.open` が clean に compile され、
  同 example で最後まで残っていた `W_DEFERRED_MEMBER` が消えました。
- `cairn-lang-core::block_array::walkway` — `connect` walkway 用の
  地面平面ルーター `route_path` を新設しました。2 ポート間の直進
  Manhattan L が placement の床を横切る場合、`lower_connects` は
  衝突セルをスキップする代わりに迂回路を探索します:
  `(セル, 進行方向)` を状態とする Dijkstra で、コストは辞書式
  `(経路長, 曲がり回数)` — 障害物を回る最短経路のうち曲がりが最少の
  ものを選びます。タイブレークは固定の展開順と単調増加のキュー連番で
  決まり、hash の反復順には依存しないため、同じソースは常に同じ
  strip を敷設し lockfile の再現性が保たれます。探索領域は歩行平面上の
  blocked セルと両端点の bounding box を 1 セル膨張した矩形で、
  400 万セルの上限を超える病的な入力は skip-and-warn フォールバックに
  degrade します。これまで home1 の床に 7 セルの穴を開けていた
  `village.crn` の `home1.entry ↔ home3.entry` 行は home1 の東面を
  迂回するようになり、example 全体が警告ゼロで compile されます。
  `route_path` は `Result<_, RoutePathError>` (ポート埋没 / 到達不能 /
  面積上限 / 座標 overflow) を返すため、呼び出し側は警告 note を実際の
  原因に対応付けられます。また `BlockedIndex` (lowering ごとに 1 回
  構築) を受け取る設計にしたので、平面ごとの bounding box は blocked
  集合の単一スキャンから得られ、`connect` 行ごとのフルスキャン
  (衝突行が多い大規模 site ではユーザ入力起点の実質 DoS になる) を
  排除しています。

### 変更

- `cairn-lang-core::block_array::lower` — `fill_roof` は `mat_slot=`
  が roof kind の canonical id 以外に解決されても `W_DEFERRED_MEMBER`
  を出さなくなりました。代わりに解決された id をそのまま palette に
  焼き込みます (`gable` / `shed` / `hip` / `flat` 全てで有効)。これで
  `themed-tower.crn` の `slot roof -> @roof.dark_wood` が warning 無しで
  dark-oak stairs 屋根になります。ただし `properties` が非空の
  `mat_slot=` 状態は依然として defer します
  (geometry generator が `facing` / `half` / `shape` を所有するため)。
- `crates/cairn-lang-cli/tests/cli_compile.rs` — `c14b`
  ("themed-tower に W_DEFERRED_MEMBER が残る" pin) を `c14e`
  ("themed-tower が defer 無しで compile される" pin) に置き換え、
  cottage の `c14` / village の `c21` と同じ品質ラインに揃えました。
- `crates/cairn-lang-cli/tests/cli_lower.rs::lower_3_deferred_member_warnings_print_to_stderr`
  は themed-tower (現在 clean) から離れ、`pressure_plate` を含む
  簡易ソースを in-line で使うようになりました。deferred-warning 経路の
  regression 保護は維持されます。
- `crates/cairn-lang-cli/tests/cli_lower.rs::lower_3_deferred_member_warnings_print_to_stderr`
  は `circuit` (現在は無音で認識) から離れ、
  `stair kind=stairs side=front shape=inner_left` の in-line ソースに
  移りました。stair の lowering は `straight` / `outer_left` /
  `outer_right` のみサポートし、`inner_left` / `inner_right` は
  依然 defer するため、それが deferred-warning の regression キャリアを
  引き継ぎます。
- `crates/cairn-lang-cli/tests/cli_compile.rs::c14f_redstone_door_pressure_plate_paints_without_deferring`
  は `circuit` / `pressure_plate` の substring チェックを廃止し、
  `warning[W_DEFERRED_MEMBER]` プライマリ行数を baseline 1 に pin
  する形に変更しました (残る 1 件は line 25 の
  `door[id=front] opened_by=…` に対する `carve_door` の
  `missing side=`)。substring チェックは catalog note が全ロール名を
  列挙する形式で false-positive し、また将来 primary から
  当該ロール名を除くリファクタで false-negative するため、baseline pin
  で両方を捕捉します。
- `crates/cairn-lang-formats/tests/redstone_door_pressure_plate_lower.rs::redstone_door_circuit_line_emits_no_deferred_warning`
  も同じ baseline pin に切り替え、`DeferredMember` の総数を 1 に pin
  します。`void=` u32 溢れ経路の primary は `nonneg_int_or_defer` 側
  に属し `"circuit"` を含まないため、substring フィルタでは溢れ経路の
  regression を検出できないという指摘への対応です。
- `crates/cairn-lang-cli/tests/cli_compile.rs::c14f_redstone_door_pressure_plate_paints_without_deferring`
  を `c14f_redstone_door_compiles_without_deferred_warnings` に改名し、
  baseline 1 の pin を廃止して
  `stderr.matches("W_DEFERRED_MEMBER").count() == 0` を pin する形に
  切り替えました (cottage の `c14` / themed-tower の `c14e` と同じ形)。
  `gatehouse.nbt` の存在確認は残しているので、lowering が silent に
  regress したケースも成果物欠落で fail-loud します。
- `crates/cairn-lang-formats/tests/redstone_door_pressure_plate_lower.rs::redstone_door_circuit_line_emits_no_deferred_warning`
  を `redstone_door_lowers_without_deferred_warnings` に改名し、
  「唯一の defer はアクチュエータパッチ」の baseline 1 を廃止して
  「defer 0 件」に切り替えました。plate paint と circuit region
  マーカーに加え actuator patch も認識されたので、example 全体が
  clean に lower されます。
- `W_WALKWAY_BLOCKED` は、迂回探索が 2 ポート間に遮られない経路を
  **一つも** 見つけられなかった場合 (ポートが他 placement の床に
  埋まっている、到達先が完全に囲まれている、面積上限超過) にのみ
  発火するようになりました。その場合は従来どおり直進 L に
  フォールバックして衝突セルをスキップするため、
  `data: { kind: "walkway_blocked", skipped: N }` ペイロードと
  "skipped N cells" のプライマリ文言は不変です。note は具体的な原因 —
  どちらのポートが埋まっているか、到達先の閉塞、探索面積上限 (実測値と
  上限値の両方を明記)、座標 overflow — をそれぞれの対処法とともに
  書き分けるようになり、4 原因中 3 つには効かない「gap を広げる」
  一択の提案を廃止しました。
- `crates/cairn-lang-core/src/block_array/lower.rs` —
  `walkway_blocked_cells_skip_with_w_walkway_blocked_count` の fixture
  に `from` ポートを床で埋める 3 つ目の placement を追加しました
  (旧 2-place fixture は迂回可能になったため、新設の
  `walkway_routes_around_obstructed_l_path_without_warning` /
  `walkway_detour_is_deterministic_across_lowerings` テストに
  移りました)。
- `crates/cairn-lang-core/tests/village_lower.rs` — home1↔home3
  walkway の pin を直進 strip (`footprint 1×15`) から home1 東面の
  迂回路 (`footprint 6×15`、anchor は home3 の front ポートのまま) に
  更新し、「village が警告ゼロで compile され、25 セルの途切れない
  gravel strip が敷かれる」契約を pin する
  `village_emits_zero_walkway_blocked_warnings` テストを新設しました。

最初の公開ナンバー付きリリースは **`2026.7.0`** (予定) です。それまでの間、本節はそのリリースに
向けてリポジトリに積まれた内容を記録します。`cairn-lang-*` クレートはまだ crates.io に公開されて
おらず、`canary` のワークスペースバージョンは `0.0.0` プレースホルダのままです。`cargo publish`
が動くのは、実際の CalVer バージョンを持つ月次 minor リリース PR がマージされたときだけです。

### Changed

- **BREAKING (lockfile schema):** `build.cairn.lock` の `LockWalkway.from`
  / `LockWalkway.to` が `"PLACE.PORT"` 連結文字列ではなく
  `{ place, port }` オブジェクトになりました。1 エンドポイントの wire 形式は
  ```yaml
  - site: hamlet
    from:
      place: home1
      port: entry
    to:
      place: home2
      port: entry
  ```
  になります。`[Unreleased]` 期間中に walkway lowering と同時に導入された
  セクションのため、外部に出回っている lockfile はまだなく、互換シムは提供
  していません。
- `cairn-lang-core::ids` — `PlaceId` / `PortId` / `SiteName` /
  `WalkwayEndpoint` / `WalkwayScopeKey` の newtype 群を新設し、resolver
  (`PortRef` / `ValidatedConnect`)、block-array IR (`Walkway` / `Placement`
  / `BlockArrayIr.walkways` のキー)、lockfile DTO (`LockPlacement` /
  `LockWalkway`) の 3 層が同じ語彙を共有するようにしました。各識別子
  newtype は構築時に `.` / `:` / 空白を拒否するので、port id に `.` が混入
  したときに `walkway::SITE::a.b.c__...` が暗黙に別の `(place, port)` 対へ
  曖昧化する旧来の silent disaster が型境界で塞がります。識別子スカラの
  wire 形式は `#[serde(transparent)]` のおかげで変わりません。
- `cairn-lang-core::resolve::ResolvedConnect` を `ValidatedConnect` に改名
  しました。`path` は `ValueWithSpan` のまま据え置きで、per-edition の
  `BlockState` への lift は registry pack resolver を持つ `cairn-lang-formats`
  クレートが下流に位置する以上、`resolve` 層では行いません。
- `cairn-lang-core::block_array::Walkway` の `dims: Dims` を
  `footprint: Footprint { x, z }` に置換しました。walkway は常に 1 ブロック
  厚なので、`y = 1` の invariant が型に出るようになり、`Footprint::to_dims_y1`
  が lockfile 書き出し時 1 箇所だけで暗黙の `y` を補います。
- `cairn-lang-core::block_array::build_walkway_array` の戻り値を
  `(BlockArray, (i32, i32, i32), usize)` の生 3-tuple から
  `WalkwayLayout { array, origin, blocked_count }` の named struct に
  変更しました。呼び出し側が origin と blocked_count を暗黙にスワップする
  事故を型レベルで防ぎます。

### Added

- `door at=` で `center` に加えて `left` と `right` の名前付きアンカーを
  受け付けるようになった。`left` は openings カットと walkway ポートの
  両方を壁ローカル軸の原点 (`u = 0`) に、`right` は遠端
  (`u = wall_length - 1`) に固定する。`center` の挙動は不変
  (`u = wall_length / 2`, 偶数長は round-down) なので、既存の example
  やロックファイルは影響を受けない。`super::walkway::door_anchor_offset`
  と `super::lower::carve_door` が同じ語彙を共有するため、walkway ポート
  と openings カットは常に同じ列に解決される。数値オフセット (`at=N`)
  は将来拡張用に予約されたままで、`W_DEFERRED_MEMBER` を介して deferred
  になる。その defer メッセージは 3 つの許容アンカーを列挙するように
  更新された。新規 `examples/at-side-walkway.crn` と
  `crates/cairn-lang-core/tests/at_side_walkway_lower.rs` が両端アンカー
  を統合境界で固定する。詳細は `spec/components-editing-sites.md`
  §9.3.5 と `spec/syntax.md` §5.4 を参照。
- `cairn-lang-core::block_array::walkway::port_world_position` — walkway
  のポート端点を `door` メンバーに加えて `window` メンバーでも宣言
  できるようになった (door の挙動は変更なし)。`window` の壁ローカル
  アンカーは矩形の幾何中心 (`offset + size.w / 2`) を採用し、ポート
  位置は placement の地面段 (`place_origin.1`) に固定したままなので、
  歩道の 1 voxel 厚平坦 strip 不変 (`from.y == to.y`) は window の
  宣言済み `y=` に依らず保持される。window は水平方向
  (`offset + size.w ≤ wall_length`) と垂直方向
  (`y + size.h ≤ walls.height`) の両方で壁内に収まる必要がある。
  openings パスがカットできない window はポートも構築できず、行は
  `W_DEFERRED_MEMBER` で破棄され、ノートには door / window / 予約
  ロールの契約が順番に列挙される。`sym=true` の window はプライマリ
  `offset` 側の 1 点だけがポートとなる。stair / roof のポートは将来
  拡張用に予約されたまま。詳細は `spec/components-editing-sites.md`
  §9.3.5 を参照。引数 `port_id` は `&str` から `&PortId` に切り替わり、
  #34 の newtype 移行で残っていた最後の `String`-primitive 穴を塞いだ。
- `cairn-lang-core::check::DiagnosticData` — `Diagnostic` に機械可読
  ペイロードを載せる新しい公開 enum を追加。最初のバリアント
  (`WalkwayBlocked { skipped }`) は `W_WALKWAY_BLOCKED` と同時に
  使われ、`cairn check --format json` の出力に `data.skipped` として
  スキップ件数を公開する。これにより LSP のクイックフィックスや CI
  アノテーターは人間向け `primary` メッセージから `"skipped N cells"`
  部分文字列を抽出する必要が無くなる。ペイロードを持たない診断では
  `data` キーごと省略されるため、既存の JSON 消費者に対しては
  完全に additive な変更となる。`spec/lint.md` §11.2 に JSON
  シェイプ全体を記載。`Diagnostic` 本体にも `#[non_exhaustive]`
  を付与したため、今後フィールドを追加してもクレート外利用者に
  対する破壊的変更にならない (クレート内構築箇所は従来どおり
  struct literal で更新)。
- `cairn-lang-core::block_array::lower` — walkway 端点 skip のカスケード
  警告を追加。`connect` 行が指す placement が lowering されなかった
  (def に `size=` が無い、theme 参照が上流で失敗、など) 場合、
  `lower_connects` は静かに strip を落とすのではなく、欠落側を名指しした
  `W_DEFERRED_MEMBER` を発するようになった。修正ヒントとして元の
  `W_DEF_NO_SIZE` / `W_DEFERRED_MEMBER` / `E_UNRESOLVED_PLACE_REF` を
  追跡するよう note 化した。健全な入力に対する walkway IR / lockfile
  出力は変わらない。
- `crates/cairn-lang-core` の回帰テストを拡充し、walkway 表面を end-to-end
  でピン留めした: `W_WALKWAY_BLOCKED` の skip 数契約、abstract token を
  walkway パスとして lift / deferred / 未知 token の 3 経路
  (`walkway_abstract_path_*`)、端点カスケード警告、`from`/`to` 対称の
  `E_UNRESOLVED_PORT` / `E_UNRESOLVED_PLACE_REF` (span anchor アサート
  込み)。`village.crn` のテストでは walkway の `origin`/`dims` も
  ピンしたので、overhang シフトの軸スワップや off-by-one が
  per walkway 単位で fail loud になる。
- `cairn-lang-core::block_array::walkway` — `connect a.PORT to b.PORT
  path=@MATERIAL` 行を walkway BlockArray に lowering する。新規 IR キー
  `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT` のもとで
  `village.crn` が `cairn compile --edition java` を end-to-end で
  通過するようになった (placement 1 配置 = 1 `.nbt`、`connect` 行 1 行
  = walkway 1 本 = 1 `.nbt`)。ポートモデルは「`door` の `side=` 壁の
  外側 1 ブロック、地面段」とし、M3-PR4 ではポート公開を `door`
  メンバーに限定 (window / stair / roof のポートは後続 PR)、
  `at=center` のみをサポート、`front`/`back`/`left`/`right` は
  `+z`/`-z`/`-x`/`+x` (`spec/components-editing-sites.md` §9.3.1) に
  対応する。歩道は両ポートで一致する Y で Manhattan L 字経路 (先に
  x 軸、次に z 軸) を辿る — 3D 経路探索や階段アプローチはポート面を
  一度に着地させるため意図的に範囲外とした。既存構造の床と重なる
  セルはスキップし、行ごとに `W_WALKWAY_BLOCKED` 警告を 1 件出す。
  `BlockArrayIr` には並列の `walkways: IndexMap<…, Walkway>` を追加
  し、ワールド原点・寸法・canonical パスマテリアル (`mat_slot=` と
  同じ `resolve_block_state` パイプラインで lift、`@gravel` のような
  concrete token と `@path.gravel` のような registry-backed abstract
  token の両方に対応) を記録する。Lockfile には既存 `placements:`
  セクションに対応する `walkways:` セクションを追加した。
- `cairn-lang-core::resolve` — site スコープ解決が、検証済みの
  `connect` 行ごとに `ResolvedConnect` を生成する (`Resolution.connects`)。
  両端の `PortRef` と `path=` 値を `ValueWithSpan` として保持する。
  解決パスは右側のポート ID が def に存在しない場合に
  `E_UNRESOLVED_PORT` (Error、近接候補ノート付き)、同名 `id=` が def
  内に複数あれば `E_AMBIGUOUS_PORT` (Error)、`path=` が欠落していれば
  `E_MISSING_PATH_MATERIAL` (Error) を発火する。左側の place ID は
  既存の `E_UNRESOLVED_PLACE_REF` を再利用するため、未知の place
  コード系列の単一発生源を保つ。失敗した connect は `connects` から
  除外され、walkway voxeliser は安全に敷ける行のみを処理する。
- lowering 側に 2 つの advisory コードを追加。
  `W_WALKWAY_BLOCKED` (Warning) は L 字経路が既存構造の床を貫通した
  ときに発火する。衝突セルは air のままで、残りの strip はそのまま
  敷設される。`W_DUPLICATE_WALKWAY` (Warning) は同じ `(from, to)`
  ポート組が同一 site で既に敷設済みのときに発火し、重複行は静かに
  破棄される。重複ガードは両端を sort してから保持するため、
  `a.entry → b.entry` と `b.entry → a.entry` は同じ walkway として
  集約される。
- `cairn-lang-formats::java_structure::output_filename` が
  `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT` という IR
  キー形を解釈し、ファイル名 `SITE_walkway_FROM_PLACE_FROM_PORT__TO_PLACE_TO_PORT.nbt`
  を返すようになった。`.` 区切りをフラットにし、ディスク上の名前を OS
  間で単一の識別子トークンに保つ。

- `cairn-lang-core::block_array::lower` — site lowering により
  `village.crn` の往復が完成。`lower_to_block_array` は既存の struct ループ
  の後に `intent.sites` を走査し、各 `place` について `use=DEF` を
  モジュールの defs から引き、place ローカルの `theme=` を def の body に
  適用 (スコープ跨ぎテーマ解決) し、`site::SITE::PLACE_ID` という新キーで
  per-place `BlockArray` を発行する。既存の `prepare_artifacts` →
  `write_compound_gzip` 経路がそのまま 1 配置 = 1 `.nbt` を書き出す
  (`home1.nbt`、`home2.nbt`、`home3.nbt`)。トポロジカル座標ソルバは
  `front` が `+z` 規約 (`spec/components-editing-sites.md` §9.3.1) に従い、
  `at=origin` / `east_of=ID gap=N` / `north_of=ID gap=N` を絶対座標
  `(x, y, z)` に変換する: `east` は直前配置の inflated `dims.x` プラス
  gap だけ `+x` 方向に進み、`north` は `dims.z` プラス gap だけ `-z`
  方向に後退する。解いた per-place origin は新規
  `BlockArrayIr.placements: IndexMap<…, Placement>` とロックファイル新設
  トップレベル `placements` セクションに記録され、下流の consumer は
  ソルバを再実行せずに村のレイアウトを再構築できる。(`connect` 行の
  解決と voxelization は上の M3-PR4 walkway エントリでカバーされる)
- `cairn lower` と `cairn compile` が resolver 由来の診断
  (`E_UNRESOLVED_PLACE_REF`、`E_UNRESOLVED_THEME_REF`、
  `E_DUPLICATE_PLACE_ID`、`E_INVALID_PLACE_ORIGIN`、`W_UNUSED_DEF`、
  `E_UNRESOLVED_SLOT` 等) を lowering の deferrals と並んで stderr に
  surface するようになった。resolver の `Error` 重大度は compile の
  exit code を非 0 にするため、`place use=cottag` タイポが `.nbt` ゼロ
  exit 0 で素通りすることはなくなる。
- site 表面をカバーする 6 つの診断コードを追加:
  `E_UNRESOLVED_PLACE_REF` (Error) は `place use=X` の `X` が未宣言の def
  である場合、または `east_of=Y` / `north_of=Y` の `Y` が同一 site の先行
  place id でない場合に発火し、既存の `suggest::nearest_match` による
  近接マッチ note を伴う; `E_UNRESOLVED_THEME_REF` (Error) は
  `place theme=X` の `X` が未宣言の場合に発火し、同様に候補 note 付き;
  `E_DUPLICATE_PLACE_ID` (Error) は同一 site 内で 2 つの `place` が `id=`
  を共有した場合に発火し、最初の宣言へのスパンポインタを note で示す;
  `E_INVALID_PLACE_ORIGIN` (Error) は `place` 行に原点セレクタがない、
  `at` / `east_of` / `north_of` を 2 つ以上併用、または `at=` が `origin`
  以外を取った場合に発火する; `W_UNUSED_DEF` (Warning) はどの
  `place use=NAME` からも参照されない `def` に対して発火し、`use=` 側の
  タイポが空ビルドを密かに生む事故を防ぐ; `W_DEF_NO_SIZE` (Warning) は
  `place` から参照された `def` に `size=WxH` ヘッダがない場合に発火する
  (voxel footprint を導出できないため当該配置はスキップ)。原点検査は
  `return false` するため、構造的に不正な placement は `.nbt` を残さず
  exit 非 0 で完全にスキップされる。spec §9.3.2 / §9.3.3 が これらコードの
  守る規約を列挙する。
- `cairn-lang-core::lock::LockPlacement` と
  `Lockfile.placements: Vec<LockPlacement>` — トポロジカル制約チェインから
  解いた per-`place` ワールド座標を `member_version_sensitivity` の隣に
  ロックファイルへ記録する。各エントリは `site`、`id`、`def`、`theme`、
  `origin: [i32; 3]` (`north_of` 配置で負の `z` をとる)、
  `dims: [u32; 3]` を pin する。フィールドは
  `skip_serializing_if = "Vec::is_empty"` で、cottage / themed-tower の
  ロックファイルは PR3 以前とバイト一致する。既存の `hash_resolved_ir` は
  serde-json の構造走査で新フィールドを自動的にハッシュへ取り込む。
  spec §9.3.4 が「再解決不要な site レイアウトの単一情報源」として
  この section を文書化している (2027.1.0)。
- `cairn-lang-formats::java_structure::output_filename` が
  `site::HAMLET::home1` → `home1.nbt` のマッピングを既存の
  `struct::cottage` → `cottage.nbt` 規則と並んで習得した。per-place 配置は
  兄弟 struct と同じ出力ディレクトリを共有する。複数 site の
  フラット名前空間衝突は M3 の対象外で、spec でも明示的に carve-out
  されている。
- `cairn-lang-formats::registry::materials` — Java registry pack に抽象
  マテリアルカタログコンポーネントを追加。`spec/materials-themes.md` §7.2
  の `@KIND.FAMILY.SPECIES` 抽象トークンを正規の Minecraft ブロック ID に
  マッピングするフラットな `(token, block)` テーブル。組み込みカタログは
  `data/registry/java/materials.json` に置かれ、`data_versions.json` と
  並んで `include_str!` で埋め込まれる。`pack.json::files.materials` は
  `Option<String>` コンポーネントなので、`--registry-pack <dir>` が
  `materials` エントリを持たない場合でも依然として読み込める (古い pack は
  `MaterialsIndex::empty` に乗る)。`MaterialsIndex::from_catalog` は
  `token` 重複を `RegistryError::Materials` / `MaterialsError::DuplicateMaterialEntry`
  で load 時に拒否し、サイレント上書きを許さない。エントリが明示的に
  `namespace:` を含めばオーバーライドし、bare ID ならカタログのトップ
  レベル `namespace` を継承する (正規トークンの `BlockState` 解決と同じ
  ルール)。カタログのバイト列は `pack_hash` のマルチコンポーネント経路で
  `RegistryPack::bytes_hash` に流れ込むため、materials catalog を差し
  替えればロックファイルの `inputs.registry_pack_hash` が動く。
- `cairn-lang-core::block_array::AbstractMaterialResolver` — block-array
  lowering pass が抽象マテリアルトークン (`@floor.wood.broadleaf`) を
  canonical `BlockState` に lift するために呼び出す trait。
  `cairn-lang-formats::registry::MaterialsIndex` が実装し、
  `core → formats` の逆方向 import を避けつつ CLI が組み込み pack を
  lowering に渡せるようにする。`MaterialDeferred` に
  `UnknownAbstract { token, suggestion }` variant を追加 (pack は
  あるがそのトークンが無い場合)。`Abstract` variant は維持し、
  library 経路 (LSP highlight、resolver 未渡しの `cairn check`) で
  従来通り deferred 扱いできるようにする。`lower_to_block_array` は
  `materials: Option<&dyn AbstractMaterialResolver>` を取るため、CLI 面
  で `builtin_java().materials` を一発で配線できる。
- `E_UNKNOWN_ABSTRACT_TOKEN` (Error) — `mat_slot=` が registry pack の
  materials catalog に無い抽象トークンに解決された時に発火。診断には
  `nearest_match` (`2026.12-PR2` で `--target` バージョンや slot 名と
  同じ Damerau-Levenshtein 閾値・タイブレークルール) が拾った
  `did you mean \`@X\`?` note と `spec/materials-themes.md` §7.2 への
  ポインタが付く。`cairn lower` および `cairn compile` は lowering 段階で
  `Severity::Error` の診断が 1 件でも出れば exit `1` で終了するように
  なり、fail-loud の期待が parse/resolve だけでなく lowering にも適用
  される。組み込みカタログは `examples/themed-tower.crn` が bind する
  全トークン (`floor.wood.broadleaf` → `oak_planks`、`wall.stone.cobble`
  → `cobblestone`、`wood.dark` → `dark_oak_planks`、`roof.dark_wood`
  → `dark_oak_stairs`) を網羅するため、themed-tower は今や
  `W_ABSTRACT_TOKEN_DEFERRED` ゼロで lowering を通過する。屋根の
  ハードコードによる `W_DEFERRED_MEMBER` と `level` ブロックの保留は
  残るが、抽象トークン解決自体はクリーンになった (2027.1.0)。
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
  (2027.1.0)。
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
- リリース戦略: 月次 minor (`YYYY.M.0`) は毎月 1 日 04:17 UTC の GitHub Actions cron、
  patch (`YYYY.M.N`) は適格コミットの `canary` push で随時。リリース PR
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
  `2027.6.0` までの月別スコープを掲載。

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
