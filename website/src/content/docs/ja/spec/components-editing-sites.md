---
title: "9. コンポーネント、編集、複数建築"
---

## 9.1 def (コンポーネント)
`def` はスロット保持型 Component を定義し、`theme` および `site` と同じ機構で統一されています。これに
より参照系が、編集・テーマ化・複数建築接続のあいだで分裂しません。

- パラメータ化 (可変サイズなど) は許可します。再帰は禁止です。
- `def` は `requires version>=X` を宣言してかまいません。コンポジットの最小バージョンはその構成要素
  の最大値です ([バージョンとエディション](versioning-editions))。

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=door  class=entry side=front at=center
  roof   id=roof  kind=gable mat_slot=roof
```

## 9.2 編集モデル
**明示 ID + 自動安定アドレスの組み合わせ**。重要なメンバは `id=` を持ち、未指定のメンバはコンパイラが
**意味ベースの安定アドレス** を自動付与します。アドレスは生成順ではなく parent / role / side / level /
offset から導かれるため、追記しても安定します。

編集はセレクタ/アドレスに対するパッチ DSL です:

```
edit window[class=vent][level=floor2] set shape=arch
edit window@front[0]                  set mat_slot=accent_glass
edit door[id=entry]                   set side=front at=center
```

「2階の窓だけアーチにする」のような概念レベルの編集が、全体を壊さずに行える必要があります。編集 diff
は `intent_state` のみを見るため ([ブロックステート](blockstate))、導出結果の変化は編集の安定
性を損ないません。

## 9.3 複数建築 (site)
AI に絶対座標の計算をさせない。トポロジカルな関係制約で配置し、絶対座標への解決はコンパイラの責務
とします。

```
site village:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  connect home1.door to home2.door path=@gravel
```

各 struct はポート (位置 / 法線 / 幅) を露出し、`connect` がそれらを繋ぎます。structure block の 48³
上限を超える村や城は、複数 struct の合成で表現します。

### 9.3.1 座標規約
- `east` は `+x` 方向、`north` は `-z` 方向に進みます。これは §5.4 の `front` が `+z` の規約と整合し
  ます — 南を向く建築の正面は `+z` 側にあり、`north_of=X` は次の配置をその背後に置きます。
- Y 軸はトポロジカルセレクタの影響を受けません。現状すべての配置は `y = 0` です。

### 9.3.2 原点セレクタ
各 `place` は `at` / `east_of` / `north_of` の **ちょうど 1 つ** を持ちます:

| セレクタ | 効果 | 注記 |
|---|---|---|
| `at=origin` | ワールド `(0, 0, 0)` に配置を固定。 | `at=` の唯一の合法値です。site 内の最初の `place` はこのアンカーを必須とします — `at=origin` の暗黙のデフォルトはありません。 |
| `east_of=ID gap=N` | 新規 origin = 直前 `(x + dims.x + N, y, z)`。 | `ID` は同じ `site` 本体で先に宣言された place 名でなければなりません。`gap` はブロック単位の辺間距離 (0 → 壁面が接触)。省略時は `0`。 |
| `north_of=ID gap=N` | 新規 origin = 直前 `(x, y, z − dims.z − N)`。 | `ID` と `gap` の規則は `east_of` と同じ。 |

セレクタの併用 (`at` + `east_of`、`east_of` + `north_of` など) は `E_INVALID_PLACE_ORIGIN` で拒否し
ます。`at=` に `origin` 以外を渡した場合も同じエラーです。

### 9.3.3 スコープ跨ぎ参照
- `use=NAME` はトップレベルの `def` を指す必要があります。未知名は `E_UNRESOLVED_PLACE_REF` で
  失敗し、編集距離キャップに収まる候補があれば近接マッチを提示します
  (`versioning-editions.md` §10.6 の規約に従う)。
- `theme=NAME` は同一ファイルで宣言された `theme` を指す必要があります。未知名は
  `E_UNRESOLVED_THEME_REF` で失敗し、同様に近接マッチを伴います。
- どの `place use=NAME` からも参照されない `def` は `W_UNUSED_DEF` (警告) として通知されます。
  `use=` 側のタイポが空ビルドを密かに生む事故を防ぐためです。
- 同一 site 内で 2 つの `place` が `id=` を共有することはできません。重複は
  `E_DUPLICATE_PLACE_ID` で報告され、診断は最初の宣言へのスパンポインタを伴います。

### 9.3.4 出力ファイル名
コンパイラは `place` ごとに 1 ファイルの `.nbt` を、`id=` 名で書き出します (例: `home1.nbt`、
`home2.nbt`)。各配置のワールド座標原点と `(site, def, theme)` の出自は `build.cairn.lock` の
`placements` セクションに記録され、下流の consumer は座標ソルバを再実行せずにレイアウトを
再構築できます。

### 9.3.5 ポートと `connect`
`connect FROM.PORT to TO.PORT path=@MATERIAL` 行は、同一 `site` 内の 2 つの placement が公開する
ポート間に幅 1 ブロックの歩道を敷設します。

**ポート**: `PLACE.PORT` は `(place, member_id)` の組に解決されます。現状ポートを公開できるのは
参照先 `def` の `door` メンバーのみで、window / stair / roof のポートは将来拡張用に予約されて
います。ポートのワールド座標は「ドアの `side=` 壁の外側 1 ブロック、地面段」とし、
`front`/`back`/`left`/`right` はそれぞれ `+z`/`-z`/`-x`/`+x` に対応します (§9.3.1)。ドアの
`at=center` という壁ローカル オフセットと placement の overhang を合成して、ポートは構造の外面
を超えた overhang リング上に着地します。

**経路**: 歩道は 2 つのポートで一致する Y で Manhattan L (先に x 軸、次に z 軸) を走ります。
階段や複数階層をまたぐ 3D 経路探索はポート面を一度に着地させるため意図的にスコープから外して
います。既存の構造床と重なるセルはスキップし、行ごとに `W_WALKWAY_BLOCKED` を 1 件だけ警告として
発します。警告には `--format json` 出力で機械可読ペイロード
(`data: { kind: "walkway_blocked", skipped: N }`) が付与され、LSP のクイックフィックスや CI
アノテーターが人間向けメッセージを再パースせずにスキップ件数を読み取れます — 詳細は
`spec/lint.md` §11.1 を参照。

**マテリアル**: `path=@TOKEN` の値はメンバーマテリアルと同じ `mat_slot=` パイプラインを通って解決
されます。`@gravel` のような canonical token はレジストリパック無しで利用でき、`@path.gravel` の
ような abstract token はパックのマテリアルカタログを要求し、ヒットしないときは
`W_ABSTRACT_TOKEN_DEFERRED` / `E_UNKNOWN_ABSTRACT_TOKEN` を発火します。

**出力**: 各 `connect` 行は site とポート名を組み合わせた 1 つの `.nbt` を書き出し
(例: `hamlet_walkway_home1_entry__home2_entry.nbt`)、`walkways:` セクションとして lockfile に
ワールド原点 / 寸法 / 解決済みパスマテリアルを記録します。

**Diagnostics**:

- `E_UNRESOLVED_PORT` — ドット右側のポート ID が参照先 def のメンバーに存在しない (近接候補が
  あれば `did you mean` ノートを付与)。
- `E_AMBIGUOUS_PORT` — def が同名 `id=` を複数のメンバーで公開している。重複を解消して参照を
  一意にする必要があります。
- `E_MISSING_PATH_MATERIAL` — `path=` が欠落しており歩道に敷くマテリアルが無い。
- `E_UNRESOLVED_PLACE_REF` — ドット左側の place ID が同一 site の先行 place を指していない。
  §9.3.3 と共有のコードです。
- `W_WALKWAY_BLOCKED` — L 字経路が既存の構造床を貫通した。衝突セルはスキップされ、残りの経路は
  そのまま敷設されます。JSON ペイロードはスキップ件数を `data.skipped` として公開するため、
  ツール側はメッセージ本文を再パースする必要がありません。
- `W_DUPLICATE_WALKWAY` — 同じ `(from, to)` ポート組が同一 site で既に敷設済み。重複行は破棄
  されます。
