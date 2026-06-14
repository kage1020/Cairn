---
title: "用語集"
---

仕様書全体で使われる定義語。詳細は該当章を参照してください。

このページは **単独では非規範的** です (該当章が source of truth)。実装と作者はこれと同じ綴りを
使うことが推奨されます。語彙は意図的に閉じられており ([原則 P3](principles))、並行する用語を
発明すると lint ループが機能しなくなります。

## アーキテクチャと IR

- **Block-array IR**。三層 IR の最下層にある普遍ピボット。ボクセル格子 + パレット + block entities +
  entities で、フォーマット・エディション・バージョンに中立。すべてのフォーマットのフロントエンド/
  バックエンドと、diff/IoU/シリアライゼーションがここで出会う。
  [architecture §3.1](architecture) 参照。
- **Intent IR**。最上層。`id` / `class` / `role` / `mat_slot` / `intent_state` / `resolved_state` を
  持つ名前付きメンバ。不変条件を持つ独立した型で、schematic 取り込みから直接は生成されない。
  [architecture §3.2](architecture) 参照。
- **Semantic / Component-Theme IR**。テーマ、コンポーネント (`def`)、複数建築 (`site`) を Intent IR に
  解決する中間層。[components-editing-sites](components-editing-sites)、
  [materials-themes](materials-themes)。
- **Logic IR / Netlist IR / Placement IR**。Intent IR と block-array IR の間にあるレッドストーンの
  サブ層。ディレイは Logic/Netlist には **持たず**、Placement IR のみが持つ。
  [redstone §14.8](redstone)、[architecture §3.3](architecture)。
- **`semantic_level`**。取り込み成果物の進捗ラベル: `raw` (1 行 1 ボクセル) → `grouped` (L1 空間圧縮) →
  `lifted` (L2 意味命名)。コンパイラは L1 まで到達し、L2 は LLM の仕事。
  [ecosystem-interop §12.3](ecosystem-interop)。

## メンバとブロックステート

- **メンバ**。Intent IR の名前付き要素。型: `block`、`block_entity`、`entity`。IR は型を持つが、作者は
  全要素に対して同じセレクタ文法を書く ([原則 P4](principles))。
- **`id` / `class` / `role`**。メンバの同一性・グループ・建築機能タグ。セレクタと安定アドレスに使われる。
- **`mat_slot`**。メンバ上のマテリアル注入ポイント。`theme` が束ねる。構造はスロットを持ち、テーマが
  ブロックバインディングを持つ。[materials-themes §7.1](materials-themes)。
- **`intent_state`**。作者のブロックステート意図。編集 diff は **ここだけ** を見る。
  [blockstate §6.2](blockstate)。
- **`resolved_state`**。コンパイラ導出のブロックステート (向き、接続、waterlogged)。手書きしない。
- **上書きによる昇格 (Override-promotion)**。意図になり得るブロックステートを書くと、`resolved_state`
  から `intent_state` に昇格する。仕様ルールは「デフォルトは導出。意図になり得るブロックステートは
  すべて上書き可能」。[blockstate §6.1](blockstate)。
- **Anchor / bbox**。プリミティブは `anchor` (基準点)、宣言 bbox、実 bbox、ホスト面を IR に持つ。絵画、
  額縁、アーチ窓のように宣言サイズと占有 AABB が異なるプリミティブのために必要。
  [entities §8.2](entities)。

## マテリアルとテーマ

- **正規トークン (Canonical token)**。スロットやテーマセレクタにバインドされる値: 生のブロック ID
  ではなく **意味** のトークン。バックエンドが `(edition, version)` ごとの ID と状態名を解決する。
  [materials-themes §7.2](materials-themes)。
- **Canonical block token**。具体的なブロック意味: `@oak_planks`、`@water_cauldron`、
  `@oak_log[axis=x]`。サイレントな意味破壊的ダウングレード (`@water_cauldron` → `cauldron`) は禁止。
- **Abstract material token**。美的選択: `@floor.wood.broadleaf`、`@roof.dark_wood`。テーマポリシーが
  ダウングレードしてよい (オーク↔シラカバ)。
- **`theme`**。CSS 的な、スロット/セレクタ値から正規トークンへのバインディング。**構造** (壁がどこに
  あるか) と **スタイル** (どのブロックか) を分離する。
- **`def`**。スロット保持型 Component 定義 (再利用可能な struct)。再帰は禁止、パラメータ化は許可。
  コンポジットの最小バージョンは構成要素の最大値。
  [components-editing-sites §9.1](components-editing-sites)。
- **`site`**。`def` 由来の構造体を絶対座標ではなくトポロジカルな関係 (`east_of`、`gap=`、`connect`) で
  配置する複数建築コンテナ。[components-editing-sites §9.3](components-editing-sites)。

## コンパイル

- **フェーズ**。コンパイラが各コマンドを振り分ける固定評価スロット:
  `massing → envelope → openings → fixtures → logic_synth → logic_place → logic_route → raw`。
  ソース順は無関係で、フェーズが意味論を強制する ([compilation §4.1](compilation))。
- **Last-wins (ローカル)**。同一フェーズ内で、後のコマンドが先のコマンドを上書きする。旧来のプログラム
  全体 last-wins (ペイントモデル) は破棄。[原則 P2](principles)。
- **ターゲット軸**。`(edition, version)`: バックエンドだけがこれを知る。DSL ソースは決してこれを
  名指ししない。`--edition` 必須、`--target` 単独は禁止。[compilation §4.2](compilation)、
  [versioning-editions](versioning-editions)。
- **DataVersion**。Java バージョンに対する Mojang の単調増加整数キー。Cairn は正規順序キーとして使い、
  semver→日付ベース移行で `since/until` や `@requires` が壊れないようにする。
  [versioning-editions §10.1](versioning-editions)。

## ヘッダと provenance

- **`@cairn`**。ファイルが書かれた **Cairn 言語のバージョン** を宣言するヘッダ (CalVer
  `YYYY.0M[.PATCH]`)。provenance のみ、オプション。
- **`@requires`**。Minecraft ターゲットへの capability の下限 (例: `version>=1.20`)。推定値との衝突で
  ハードエラー。
- **`@intended_targets`**。どの Minecraft バージョン向けに設計したかのヒント。検証記録ではない (検証
  はロックに記録)。
- **ロック (`*.cairn.lock`)**。コンパイラ生成の再現性記録。`source_hash`、`cairn_version`、
  `target(mc_version + data_version)`、`registry_pack_hash`、`constraint_catalog_hash`、
  `resolved_ir_hash`、`verified: true` を持つ。
  [versioning-editions §10.6](versioning-editions)。
- **Provenance スタンプ**。取り込み時に block-array IR に記録される `(edition, version)`。フォーマット
  自体からマップされる (`.litematic` → java、`.mcstructure` → bedrock、`.schem` → java)。
  [ecosystem-interop §12.4](ecosystem-interop)。

## レッドストーン

- **Logical cell / edition cell / physical tile**。3 段のセルライブラリ。Logic IR が logical cell を
  選択し、エディション別セルライブラリが physical tile に降ろす。Java/Bedrock 差をライブラリに閉じ
  込める。[redstone §14.6](redstone)。
- **組み合わせ vs 順序**。v1 は閉集合の組み合わせゲートと、厳選された順序マクロ (`latch` / `pulse` /
  `delay` / `edge_rising` / `edge_falling` / `counter`) を提供。任意の FSM / CPU は v1 ではスコープ外。
- **真理 / レイテンシ / 時相アサーション**。3 種の検証。[redstone §14.7](redstone)。時相は
  有界 `eventually within N` のみで、完全な LTL ではない。
- **QC / BUD**。Quasi-connectivity / block-update-detector 挙動。セルライブラリで **吸収しない**。
  更新順意味論に依存する回路は `E_NO_PORTABLE_IMPL` コンパイルエラー。

## Lint と評価

- **自己修正ループ**。コンパイル → 診断 → パッチ → 再コンパイルのサイクルで、ワンショット生成を超えた
  精度を獲得する ([原則 P5](principles))。診断メッセージは「何が間違っているか / ターゲットで
  有効な候補 / 推奨される修正」の形でなければならない ([lint](lint))。
- **Fail-loud**。未知 ID / ドメイン外状態のサイレント置換と暗黙の削除は禁止。エラーは有効な候補の
  閉集合と修正案 DSL を返す。[versioning-editions §10.4](versioning-editions)。
- **`semantic_sensitivity`**。「ID は有効なまま意味/挙動/見た目が変わった」を `since/until` と区別する
  制約カタログのフィールド。cauldron 分割@1.17、壁接続 bool→none/low/tall@1.16、アイテムフォーマット
  @1.20.5 が例。[versioning-editions §10.5](versioning-editions)。
- **Block IoU**。取り込み自己修正ループに使うボクセル積集合/和集合比。収束閾値 ≥ 0.985。
  [ecosystem-interop §12.2](ecosystem-interop)、[evaluation §13.2](evaluation)。
- **Zero-shot Compile Rate / Fix Convergence Rate / Token Efficiency / Edit Stability**。仕様反復の
  4 主要メトリクス。[evaluation §13.1](evaluation)。

## エディションとバージョン

- **エディション**。`java` または `bedrock`。コンパイル時のみ、DSL の意味層には決して現れない。
  [versioning-editions §10.7](versioning-editions)。
- **Cairn バージョン vs MC ターゲット**。**別軸** の 2 つ。フィールド/フラグ/キーワードで区別し、
  フォーマットでは区別しない。曖昧化を避ける散文では `cairn:2026.06` vs `mc:1.21.4`。
  [spec/README.md](README)、[versioning-editions](versioning-editions)。
- **Recompile、transcode しない**。言語仕様はバージョン/エディションを跨ぐ NBT 可搬性を保証しない。
  新バージョンを狙うには DSL を再コンパイルする。NBT を変換しない。
  [versioning-editions §10.2](versioning-editions)。

## エコシステム連携

- **Raw / L1 / L2**。取り込み忠実写し取りの 3 段階。コンパイラは L1 (空間圧縮、命名なし) まで到達し、
  L2 は LLM の仕事 (命名 → `wall`、`mat_slot`、`theme`)。
  [ecosystem-interop §12.3](ecosystem-interop)。
- **`raw_fill` / `raw_block` / `raw_repeat`**。忠実写し取りで使われるエスケープハッチプリミティブ。
  取り込み起源のインスタンスは `origin=imported` を持ち、ファーストクラスの設計 DSL として扱わない。
