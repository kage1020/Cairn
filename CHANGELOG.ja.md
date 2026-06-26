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
  ペル walkway 単位で fail loud になる。
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
  この section を文書化している (2027.01.0)。
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
  残るが、抽象トークン解決自体はクリーンになった (2027.01.0)。
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
