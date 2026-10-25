---
title: "用語集"
---

仕様書の各章で定義している用語をまとめたページです。詳しい定義がある章にはリンクを張っています。

このページ **だけでは規範になりません**。定義はリンク先の各章にあります。それでも、実装でも `.crn` を書くときでも、ここにある綴りをそのまま使ってください (SHOULD)。語彙は意図的に閉じています ([原則 P3](/ja/spec/principles/))。同じものを別の言葉で呼び始めると、lint のループが働かなくなります。

## アーキテクチャと IR

- **block-array IR**: 3 層の IR の最下層にある、すべての形式に共通のピボットです。ボクセルの格子、パレット、block entities、entities からなり、形式・エディション・バージョンのどれにも依存しません。各形式のフロントエンドとバックエンド、差分、IoU、シリアライズは、すべてこの層でつながります。[アーキテクチャ §3.1](/ja/spec/architecture/#31-block-array-ir-はすべてのフォーマットに共通のピボット) を参照してください。
- **Intent IR**: 最上層です。`id` / `class` / `role` / `mat_slot` / `intent_state` / `resolved_state` を持つ、名前付きのメンバーからなります。不変条件を持つ独立した型で、schematic の取り込みから直接は作られません。[アーキテクチャ §3.2](/ja/spec/architecture/#32-intent-ir-は情報が豊富で不変条件を持つ) を参照してください。
- **Semantic / Component-Theme IR**: テーマ、コンポーネント (`def`)、複数建築 (`site`) を解決して Intent IR にする中間層です。[コンポーネント・編集・複数建築](/ja/spec/components-editing-sites/) と [マテリアルとテーマ](/ja/spec/materials-themes/) を参照してください。
- **Logic IR / Netlist IR / Placement IR**: Intent IR と block-array IR の間に入る、レッドストーン用の 3 つのサブ層です。遅延は Logic IR と Netlist IR には **なく**、Placement IR だけが持ちます。[レッドストーン §14.8](/ja/spec/redstone/#148-ir-とフェーズの関係) と [アーキテクチャ §3.3](/ja/spec/architecture/#33-レッドストーンのサブ層) を参照してください。
- **`semantic_level`**: 取り込んだ成果物がどこまで進んだかを示すラベルです。`raw` (1 行に 1 ボクセル)、`grouped` (L1 の空間的な圧縮)、`lifted` (L2 の意味の付与) の順に進みます。コンパイラが担うのは L1 までで、L2 は LLM の仕事です。[エコシステム連携 §12.3](/ja/spec/ecosystem-interop/#123-写し取りの-3-段階) を参照してください。

## メンバーとブロックステート

- **メンバー (Member)**: Intent IR の名前付きの要素です。型は `block`、`block_entity`、`entity` の 3 つです。IR は型を区別しますが、書き手はどの型にも同じセレクタ文法を使います ([原則 P4](/ja/spec/principles/))。
- **`id` / `class` / `role`**: メンバーに付ける、同一性、グループ、建築上の役割のタグです。セレクタと安定したアドレスに使います。
- **`mat_slot`**: メンバー上のマテリアルの注入ポイントです。`theme` が値を割り当てます。構造はスロットだけを持ち、どのブロックを使うかはテーマが持ちます。[マテリアルとテーマ §7.1](/ja/spec/materials-themes/#71-依存性注入としてのスロット) を参照してください。
- **`intent_state`**: 書き手が意図したブロックステートです。編集の差分は **ここだけ** を比べます。[ブロックステート §6.2](/ja/spec/blockstate/#62-intent_state-と-resolved_state) を参照してください。
- **`resolved_state`**: コンパイラが導いたブロックステート (向き、接続、waterlogged) です。手で書くことはありません。
- **上書きによる昇格 (Override-promotion)**: 意図になりうるブロックステートを書くと、そのブロックステートは `resolved_state` から `intent_state` に昇格します。仕様上のルールは「既定では導出し、意図になりうるブロックステートはどれも上書きできる」です。[ブロックステート §6.1](/ja/spec/blockstate/#61-既定では導出し上書きで昇格する) を参照してください。
- **アンカーと bbox (Anchor / bbox)**: どのプリミティブも、IR の中に `anchor` (基準点)、宣言上の bbox、実際の bbox、取り付ける面の 4 つを持ちます。絵画、額縁、アーチ窓のように、宣言したサイズと実際に占める AABB が食い違うプリミティブを、この 4 つで扱います。[エンティティ §8.2](/ja/spec/entities/#82-アンカーの規約) を参照してください。

## マテリアルとテーマ

- **正規トークン (Canonical token)**: スロットやテーマのセレクタに割り当てる値です。生のブロック ID ではなく、**意味** を表すトークンです。`(edition, version)` ごとの ID や状態名は、バックエンドが解決します。[マテリアルとテーマ §7.2](/ja/spec/materials-themes/#72-正規トークン) を参照してください。
- **正規ブロックトークン (Canonical block token)**: 特定のブロックの意味を表すトークンです (`@oak_planks`、`@water_cauldron`、`@oak_log[axis=x]`)。意味を壊すダウングレード (`@water_cauldron` を `cauldron` にするなど) を黙って行うことは禁止です。
- **抽象マテリアルトークン (Abstract material token)**: 見た目の好みを表すトークンです (`@floor.wood.broadleaf`、`@roof.dark_wood`)。テーマのポリシーでダウングレードしてかまいません (MAY) (オークをシラカバにするなど)。
- **`theme`**: スロットやセレクタの値を正規トークンに割り当てる、CSS のような仕組みです。**構造** (どこに壁があるか) と **スタイル** (どのブロックを使うか) を分けます。
- **`def`**: スロットを持つコンポーネントの定義 (再利用できる struct) です。再帰は禁止で、パラメータ化は許されます。ただし、その仕組みが決まるまで、ヘッダに書けるのは `size=` と `class=` だけです。組み合わせたものの最小バージョンは、構成要素の最小バージョンのうち最も新しいものになります。[コンポーネント・編集・複数建築 §9.1](/ja/spec/components-editing-sites/#91-コンポーネントを定義する-def) を参照してください。
- **`site`**: 複数の建物をまとめるコンテナです。`def` から作った構造物を、絶対座標ではなく位置関係 (`east_of`、`gap=`、`connect`) で配置します。[コンポーネント・編集・複数建築 §9.3](/ja/spec/components-editing-sites/#93-site-による複数建築) を参照してください。

## コンパイル

- **フェーズ (Phase)**: コンパイラが各コマンドを振り分ける、順番の決まった評価の枠です。`massing → envelope → openings → fixtures → logic_synth → logic_place → logic_route → raw` の順に評価します。ソース上の順番は関係なく、意味はフェーズの順番で決まります ([コンパイルモデル §4.1](/ja/spec/compilation/#41-フェーズ評価))。
- **フェーズ内の後勝ち (Last-wins, local)**: 同じフェーズの中では、後に書いたコマンドが先のコマンドを上書きします。プログラム全体で後勝ちにする考え方 (「ペイントモデル」) は採りません。[原則 P2](/ja/spec/principles/) を参照してください。
- **ターゲット軸 (Target axes)**: `(edition, version)` の組です。これを知っているのはバックエンドだけで、DSL のソースには決して現れません。`--edition` は必須で、`--target` だけを指定することはできません。[コンパイルモデル §4.2](/ja/spec/compilation/#42-ターゲット軸) と [バージョンとエディション](/ja/spec/versioning-editions/) を参照してください。
- **lowering**: スコープのメンバーを block-array IR に変換するパスです。壁、開口部、屋根、通路、マテリアルが、パレットとインデックスの配列になります。`cairn compile`、`cairn lower`、`cairn info`、`cairn check` のどれもこのパスを通ります。[アーキテクチャ §3.1](/ja/spec/architecture/#31-block-array-ir-はすべてのフォーマットに共通のピボット) を参照してください。
- **DataVersion**: Mojang が Java 版の各バージョンに振る、単調に増える整数です。Cairn はこれをバージョンの順序を決めるキーに使うので、Minecraft のバージョンが semver から日付ベースに移っても、`since/until` や `@requires` は壊れません。[バージョンとエディション §10.1](/ja/spec/versioning-editions/#101-ターゲットはコンパイル時のパラメータ) を参照してください。

## ヘッダと来歴

- **`@cairn`**: そのファイルが前提とする **Cairn 言語のバージョン** (CalVer `YYYY.M[.PATCH]`) を宣言するヘッダです。来歴を記録するためだけのもので、省略できます。
- **`@requires`**: Minecraft のターゲットに求める機能の下限です (例: `version>=1.20`)。推定した値と矛盾すると、ハードエラーになります。
- **`@intended_targets`**: どの Minecraft バージョン向けに作ったかを示すヒントです。検証の記録ではありません (記録はロックに残ります)。それでもファイル自身の下限とは突き合わせるので、下限が拒否するバージョンを挙げていれば、コンパイルするまで気づかないままにはならず、その場で報告されます。
- **ロック (`*.cairn.lock`)**: コンパイラが生成する、再現性のための記録です。`source_hash`、`cairn_version`、`target(mc_version + data_version)`、`registry_pack_hash`、`constraint_catalog_hash`、`resolved_ir_hash`、`verified: true` を持ちます。[バージョンとエディション §10.6](/ja/spec/versioning-editions/#106-来歴-provenance-とロック) を参照してください。
- **来歴の記録 (Provenance stamp)**: 取り込むときに block-array IR に記録する `(edition, version)` です。値はファイル形式から決まります (`.litematic` → java、`.mcstructure` → bedrock、`.schem` → java)。[エコシステム連携 §12.4](/ja/spec/ecosystem-interop/#124-取り込み時の記録と落とし穴) を参照してください。

## レッドストーン

- **論理セル、エディションセル、物理タイル (Logical cell / edition cell / physical tile)**: 3 段のセルライブラリです。Logic IR が論理セルを選び、エディションごとのセルライブラリがそれを物理タイルに lowering します。Java 版と Bedrock 版の違いは、このライブラリの中に閉じ込めます。[レッドストーン §14.6](/ja/spec/redstone/#146-エディションの違い) を参照してください。
- **組み合わせ回路と順序回路 (Combinational vs sequential)**: v1 が提供するのは、決まった種類の組み合わせゲートと、厳選した順序回路のマクロ (`latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter`) です。任意の FSM や CPU は v1 の範囲外です。
- **真理値、レイテンシ、時相のアサーション (Truth / latency / temporal assertion)**: 3 種類の検証です。時相のアサーションは、範囲を区切った `eventually within N` だけで、完全な LTL ではありません。[レッドストーン §14.7](/ja/spec/redstone/#147-検証) を参照してください。
- **QC / BUD**: 疑似接続 (quasi-connectivity) と、ブロック更新検出器 (block-update-detector) の挙動です。セルライブラリはこれらを **吸収しません**。更新順序の意味論に依存する回路は、`E_NO_PORTABLE_IMPL` のコンパイルエラーになります。

## Lint と評価

- **自己修正ループ (Self-correction loop)**: コンパイル、診断、パッチ、再コンパイルを繰り返すサイクルです。1 回の生成では届かない精度を、これで引き出します ([原則 P5](/ja/spec/principles/))。診断メッセージは「何が間違っているか、ターゲットで有効な候補は何か、どう直せばよいか」の形にしなければなりません (MUST) ([Lint](/ja/spec/lint/))。
- **Fail-loud**: 未知の ID や、取りうる範囲の外の状態を、黙って置き換えたり暗黙に捨てたりすることを禁止する方針です。エラーは、有効な候補の閉じた集合と、DSL での修正案を返します。[バージョンとエディション §10.4](/ja/spec/versioning-editions/#104-fail-loud-と最小バージョン推定) を参照してください。
- **`semantic_sensitivity`**: 制約カタログのフィールドです。「ID は有効なままだが、そのバージョンで意味や挙動や見た目が変わった」ことを、`since/until` とは別に表します。1.17 での大釜の分割、1.16 での壁の接続の bool から none/low/tall への変更、1.20.5 でのアイテム形式の変更などが例です。[バージョンとエディション §10.5](/ja/spec/versioning-editions/#105-どのバージョン向けかの答えは-3-つある) を参照してください。
- **Block IoU**: ボクセル単位の IoU (積集合 ÷ 和集合) です。取り込みの自己修正ループで使い、収束の閾値は 0.985 以上です。[エコシステム連携 §12.2](/ja/spec/ecosystem-interop/#122-逆方向の変換-コンパイラは写し取りllm-がリフトする) と [評価フレームワーク §13.2](/ja/spec/evaluation/#132-逆変換の補助指標) を参照してください。
- **Zero-shot Compile Rate / Fix Convergence Rate / Token Efficiency / Edit Stability**: 仕様を改善していくときに見る 4 つの主要な指標です。[評価フレームワーク §13.1](/ja/spec/evaluation/#131-主要な指標) を参照してください。

## エディションとバージョン

- **エディション (Edition)**: `java` または `bedrock` です。コンパイル時にだけ使い、DSL の意味の層には決して現れません。[バージョンとエディション §10.7](/ja/spec/versioning-editions/#107-java--bedrock-の可搬性) を参照してください。
- **Cairn のバージョンと MC のターゲット (Cairn version vs MC target)**: **別々の軸** です。書式ではなく、フィールド、フラグ、キーワードで区別します。文章の中で紛らわしいときは、`cairn:2026.06`、`mc:1.21.4` のように書きます。[仕様書の目次](/ja/spec/) と [バージョンとエディション](/ja/spec/versioning-editions/) を参照してください。
- **変換せずに再コンパイルする (Recompile, don't transcode)**: 言語仕様は、バージョンやエディションをまたいだ NBT の可搬性を **保証しません**。新しいバージョンを対象にするときは、NBT を変換するのではなく、DSL を再コンパイルします。[バージョンとエディション §10.2](/ja/spec/versioning-editions/#102-言語の契約-変換ではなく再コンパイル) を参照してください。

## エコシステム連携

- **Raw / L1 / L2**: 取り込みで忠実に写し取るときの 3 段階です。コンパイラが担うのは L1 (空間的な圧縮。名前は付けない) までで、L2 (名前を付けて `wall`、`mat_slot`、`theme` にする) は LLM の仕事です。[エコシステム連携 §12.3](/ja/spec/ecosystem-interop/#123-写し取りの-3-段階) を参照してください。
- **`raw_fill` / `raw_block` / `raw_repeat`**: 忠実な写し取りに使うエスケープハッチのプリミティブです。取り込みで生まれたものには `origin=imported` が付き、設計用の第一級の DSL としては扱いません。
