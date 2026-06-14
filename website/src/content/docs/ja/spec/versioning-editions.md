---
title: "10. バージョニングとエディション戦略"
---

## 10.1 ターゲットはコンパイル時パラメータ
ターゲットは二軸 `(edition, version)` です。バージョン/エディションは DSL ソースに書きません
([コンパイルモデル](compilation))。バージョン/エディションを知るのはバックエンドのみです。

**バージョン文字列は opaque なラベルとして扱います**。Minecraft のバージョンは旧来の semver 風
(`1.21.4`) かもしれませんし、最新リリース以降は **日付ベース** かもしれません。Cairn はバージョン文字列
を解析・比較しません。**DataVersion (Mojang が割り当てる単調増加整数) を正規順序キー** として使います。
これにより `since/until`、Vmin/Vmax、`@requires`、`semantic_sensitivity` 境界の順序/範囲ロジックが、
semver→日付ベース移行を跨いで壊れません。バックエンドは「バージョン文字列 ↔ DataVersion」テーブル
を持つので、`--target` には semver でも日付ベースでもよく、同じ DataVersion に解決されます。
(Bedrock も同様に、バージョン文字列を内部の単調キーに解決します。)

## 10.2 言語の契約: recompile であり transcode ではない

> 言語仕様は、バージョン/エディションを跨ぐ **NBT の可搬性を保証しません**。保証するのは「同じ DSL を
> あるターゲットにコンパイルした結果」だけです。

- DSL = 設計図 / NBT = ターゲット固定のビルド成果物 (バイナリ相当)。新しいバージョンや別エディション
  で使うには **NBT を変換するのではなく DSL を再コンパイル** します。
- DataFixerUpper (DFU) は前方一方向、ロスあり、不完全 (アイテム、看板、絵画、ブロックエンティティで
  しばしばロス) です。**救済ツールとして扱い、言語意味論から外します**。
- 解けない残差は明示します: バージョン間の意味変化 (cauldron 分割、アイテム `tag`→`components`)、
  データテーブルにないゲーム挙動 (流体/重力/取り付け/レッドストーン)、視覚整合 (色温度ドリフト)、
  物理ルール変更 (1.21 の風チャージが古い罠を壊す)。「幾何的に正しい NBT は吐くが、ゲームプレイ
  体験は保証しない」。

## 10.3 バックエンド = データテーブル (機械抽出 + 手書きカタログ)
- **機械抽出 (ゲームの `--reports` / レジストリダンプ) = 構文とドメインの真実**: ブロック/エンティティ
  ID (存在チェック)、ブロックステートのプロパティ/ドメイン (`north=none/low/tall` の検証)、アイテム/
  コンポーネントスキーマ、DataVersion、タグ。「ゲームそのもの」を真実の源にすることで、新バージョンの
  知識ギャップを構造的に解決します (我々や LLM の記憶ではなく)。
- **データに無いもの = 手書きの、バージョンタグ付き制約カタログ** (§5.4 の制約に繋がる): 取り付け
  (額縁はガラスに掛けられない)、重力/支持 (砂利、吊りランタン)、流体挙動、エンティティ AABB、
  レッドストーン (原則モデル外)。新バージョンごとに 1 度定義すれば全ユーザが恩恵を受けます。

```yaml
constraints:
  minecraft:item_frame:
    type: entity_attachment
    since: "1.13"
    targets: { solid_full_face: true, glass_pane: false }
    error: "item_frame requires a solid attachable face"
  minecraft:lantern:
    type: support
    states:
      hanging=true:  { requires_above: solid_or_chain }
      hanging=false: { requires_below: solid_top }
```

### `(edition, version)` 行列の畳み込み
- 正規トークンを主キーに、トークンごとにエディション別マッピング (id + state_map) を持つ。
- バージョンは `inherits + diffs` で畳む。**Java をベース、Bedrock を上書き差分** として定義。
- 機械抽出された事実と手書きの意味カタログを分離。手書き部は差分のある点だけを記録。

```yaml
"@oak_stairs":
  base: { states: { half: [bottom,top], shape: [straight,inner_left,inner_right,outer_left,outer_right] } }
  mappings:
    java:    { id: minecraft:oak_stairs, base: "1.13" }
    bedrock: { id: minecraft:oak_stairs, state_map: { half=top: {upside_down_bit: true} }, dropped_states: [shape] }
  sensitivity:
    - { edition: bedrock, kind: missing_state, state: shape, reason: "no inner/outer stair shape" }
```

## 10.4 Fail-loud + 最小バージョン推定
未知 ID、ドメイン外の状態、パリティギャップは **ハードエラー** です。サイレント置換や暗黙の削除は
**禁止** です。エラーは **ターゲットで有効な候補の閉集合** + 最小バージョン + 修正案 DSL を返し、モデル
を記憶ではなくレジストリ由来の候補に引き戻します。これが自己修正ループを駆動します
([評価フレームワーク](evaluation))。

```text
E_UNKNOWN_ID line 12: "minecraft:pale_oak_planks" not in 1.21.4 registry.
  Similar valid: minecraft:oak_planks, minecraft:dark_oak_planks, minecraft:cherry_planks

E_VERSION_CAP line 7: minecraft:cherry_planks introduced in 1.20 (target 1.19.4).
  Fix: --target >=1.20, or  slot decor -> @oak_planks

E_STATE_DOMAIN line 18: wall north=true invalid for 1.21.4. Valid: none, low, tall (changed from boolean in 1.16).
  Suggested DSL: wall_segment id=yard_wall connect_north=low

E_PARITY_UNSUPPORTED line 8: text_display is Java-only (since 1.19.4); Bedrock has no display entity.
  Suggested: sign side=front text="Inn", or slot+theme fallback, or @edition java guard
```

`def` / `theme` は `requires version>=X` を宣言してかまいません。コンポジットの最小バージョンはその
構成要素の最大値です。

## 10.5 「どのバージョン用か?」 = 三軸
単一の「対応バージョン」は存在しません。`cairn info` は三軸を返します:

1. **レジストリ互換範囲 [Vmin, Vmax]**: 使用しているトークン/状態の `since/until` の交差をコンパイラ
   が導出。
2. **意味敏感メンバ (最重要)**: ID は有効なまま意味/挙動/見た目が変化する **semantic drift**。
   `since/until` は「ID が有効か」しか見ません。挙動の変化は ID の消滅より遥かに頻繁なので、Vmax を
   レジストリだけで決めるのは危険です。制約カタログは `since/until` とは別に `semantic_sensitivity`
   (境界バージョン + 理由) を持ち、コンパイルがそれを跨ぐと警告を発します (例: cauldron 分割@1.17、
   wall 接続 bool→none/low/tall@1.16、アイテムフォーマット@1.20.5)。
3. **検証済みロックターゲット** (10.6)。

```text
$ cairn info build.crn --editions java,bedrock
registry compatibility:  Java: 1.20.0 .. latest   Bedrock: 1.21.30 .. latest
edition portability:     portable: 42  degraded: 3  unsupported: 1
semantic-sensitive:      yard_water(cauldron split@1.17), fence(wall conn@1.16)
recommended test targets: Java min 1.20.0 / latest 1.21.4
```

## 10.6 Provenance とロック (再現性)
- `.crn` は `@intended_targets` (希望/ヒント) のみを持ちます。**`verified:true` + DataVersion +
  ハッシュはコンパイラが成功ビルド時にロックに書き込み** ます (ユーザ/LLM が手書きするものではありま
  せん)。
- ロックの最小十分集合: `source_hash` / `cairn_version` (Cairn リリースの CalVer。
  [README](README) 参照) / `target(mc_version + data_version)` / `registry_pack_hash` /
  `constraint_catalog_hash` / **`resolved_ir_hash`** (再現性の核心: マクロ展開、デフォルト充填、自動
  アドレス付与後の IR を固定)。

```yaml
# build.cairn.lock (コンパイラ生成)
source_hash: sha256:...
cairn_version: 2026.06        # Cairn リリースの日付バージョン (CalVer)
target: { edition: java, mc_version: 1.20.4, data_version: 3700 }
inputs: { registry_pack_hash: sha256:..., constraint_catalog_hash: sha256:... }
resolved_ir_hash: sha256:...
verified: true
member_version_sensitivity: [ { id: yard_water, reason: "cauldron split at 1.17" } ]
```

別ターゲットへの再コンパイルは、検証済みからの差分を強い警告として表示します:

```text
$ cairn compile build.crn --target 1.21.4 --lock build.cairn.lock
W_PREVIOUSLY_VERIFIED_TARGET: verified for 1.20.4/DataVersion 3700, now 1.21.4/3955.
W_SEMANTIC_SENSITIVITY: 2 members may resolve differently: yard_water, fence
```

## 10.7 Java / Bedrock の可搬性
- 導出ルールはエディション固有です: **intent_state は中立、resolved_state はエディション別**。
  契約は「同じ意図から、エディションごとに最も近い合法表現に解決する」であり、「同じ結果を保証する」
  ことではありません。

```yaml
intent_state: { primitive: stairs, corner: inner_left, facing: east }   # エディション中立
resolved_state:
  java:    { facing: east, half: bottom, shape: inner_left }
  bedrock: { weirdo_direction: 1, upside_down_bit: false }              # shape が無いので内角は繋がらない
```

解決結果の差が視覚/機能差になる場合、lint が通知します:

```text
W_INTENT_DEGRADED line 12 id=roof_corner:
  shape=inner_left cannot be resolved in Bedrock (stairs have no shape state).
  Bedrock stairs render straight; visual gaps at corners.
```

- 正規語彙は ID/状態/シリアライゼーション差のみ吸収可能です。**概念の不在やゲーム挙動差は吸収しません**。
  代表例: display エンティティ (Bedrock 無し)、stairs shape (Bedrock 状態無し)、armor_stand ポーズ、
  レッドストーン伝搬、アイテム components↔Bedrock アイテム NBT、light ブロックの内部挙動。

- **意味層での `@edition` 条件分岐は禁止** です。代替が必要な場合は次の階層を使います:
  1. 閉じた意味プリミティブ (中立) → 表現できなければ fail-loud (`E_PARITY_UNSUPPORTED`)。
  2. **スロット + エディション別テーマでのフォールバック** (`floating_text` スロットを Java では
     `text_display`、Bedrock では glowing 看板に解決)。
  3. エスケープハッチ層 (raw ID/nbt は本来エディション固有) でのみ `@edition` ガードを使用。

```
hologram id=shop_sign text="Weapon" mat_slot=floating_text   # 意味層は常に中立
theme shop_java:    slot floating_text -> text_display scale=2.0
theme shop_bedrock: slot floating_text -> sign glowing=true   # Bedrock フォールバック

@edition java    { raw_block mat=minecraft:light[level=15] at=4,3,2 }
@edition bedrock { raw_block mat=minecraft:light_block["block_light_level"=15] at=4,3,2 }
```

- バージョン間適用は非対称です。**ダウングレード (新バージョン NBT → 旧バージョンワールド) =
  ハードエラー** (未知 components がクラッシュ/破損を引き起こす)。**アップグレード (旧バージョン NBT
  → 新バージョンワールド) = 強い警告 + DataVersion スタンプ + DFU 依存** (明示的な
  `--allow-cross-version` のみ)。すべてのビルドがエディション可搬である必要はありません。コンパイラは
  何が可搬性を壊しているかを述べます。
