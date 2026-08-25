---
title: "10. バージョンとエディション戦略"
---

## 10.1 ターゲットはコンパイル時パラメータ

ターゲットは `(edition, version)` の 2 軸で、どちらもソースには書きません。知っているのはバックエンド
だけです ([コンパイルモデル](compilation))。

**バージョン文字列は不透明なラベルとして扱います。** Minecraft のバージョンは従来の semver 風
(`1.21.4`) の場合も、最新リリース以降の日付ベースの場合もあります。Cairn はバージョン文字列を比較
せず、Mojang が付ける単調増加の整数 **DataVersion** を正準の順序キーにします。これにより
`since`/`until`、Vmin/Vmax、`@requires`、`semantic_sensitivity` の境界が semver → 日付ベースの移行を
またいでも壊れません。

バックエンドは「バージョン文字列 ↔ DataVersion」表を持つので、`--target` には同じバージョンのどちらの
綴りを渡してもかまいません。Bedrock も同様に、バージョン文字列を内部の単調キーに解決します。

## 10.2 言語の契約: recompile であり transcode ではない

> 仕様はバージョンやエディションをまたぐ NBT の可搬性を **保証しません**。保証するのは「同じソースを
> あるターゲットにコンパイルした結果」だけです。

ソースが設計図、`.nbt` はターゲットに固定されたビルド成果物 (コンパイル済みバイナリ相当) です。新しい
バージョンや別のエディションで使うには、ソースを再コンパイルします。

DataFixerUpper は前方向のみ、lossy、かつ不完全です (アイテム、看板、絵画、ブロックエンティティで欠落が
頻発します)。救済ツールであり、言語の意味論には入れません。

解けない残りは隠さず明示します。

- バージョン間の意味変化 (cauldron の分割、アイテムの `tag` → `components`)。
- データテーブルに無いゲーム挙動 (流体、重力、取り付け、レッドストーン)。
- 見た目の一貫性 (色温度のドリフト)。
- 物理規則の変更 (1.21 の wind charge が旧来のトラップを壊す)。

幾何的に正しい NBT は出ますが、ゲーム体験は保証されません。

## 10.3 バックエンド = データテーブル

バックエンドには 2 つの供給源があり、両者は分離されています。

**機械抽出** — ゲームの `--reports` / レジストリダンプから。構文とドメインの真実です。ブロック/
エンティティ ID、ブロックステートのプロパティとドメイン、アイテム/コンポーネントのスキーマ、
DataVersion、タグ。誰かの記憶ではなくゲーム自体を真実の源にすることで、新バージョンに対する知識の
ギャップを構造的に解消します。

**手書きのバージョンタグ付き制約カタログ** — データに無いもの用です。取り付け (額縁はガラスに掛け
られない)、重力と支持 (砂利、吊りランタン)、流体挙動、エンティティ AABB、レッドストーン。新バージョン
ごとに 1 回定義すれば、全ユーザが恩恵を受けます。

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

正準トークンを主キーにし、各トークンがエディション別の対応 (id + state_map) を持ちます。バージョンは
`inherits + diffs` で畳み込み、Java を基底、Bedrock を上書き差分とします。手書きの意味カタログは
差異のある点だけを記録します。

```yaml
"@oak_stairs":
  base: { states: { half: [bottom,top], shape: [straight,inner_left,inner_right,outer_left,outer_right] } }
  mappings:
    java:    { id: minecraft:oak_stairs, base: "1.13" }
    bedrock: { id: minecraft:oak_stairs, state_map: { half=top: {upside_down_bit: true} }, dropped_states: [shape] }
  sensitivity:
    - { edition: bedrock, kind: missing_state, state: shape, reason: "no inner/outer stair shape" }
```

## 10.4 Fail-loud と最小バージョン推定

未知 ID、ドメイン外の状態、パリティのギャップはハードエラーです。サイレント置換と暗黙の削除は禁止です。
エラーはターゲットで有効な候補の閉集合、最小バージョン、推奨修正を返します。モデルを自身の記憶ではなく
レジストリ由来の候補へ引き戻すためです。

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

### 未知 ID は「どのレジストリ」に対して未知なのか

ID はエディション全体ではなく、コンパイルが固定した 1 つの `(edition, version)` のブロック表に対して
照合されます。Bedrock 1.21.0 は石レンガを `stonebrick`、1.21.40 は `stone_bricks` と綴るので、
エディション単位で答えるとどちらもどこでも通り、どちらの誤りも捕まえられません。表はレジストリパックの
`blocks` コンポーネントに入り、§10.3 の `inherits + diffs` で畳み込まれます。

したがってこの検査が走るのは `cairn compile --target` だけです。`cairn info` と `cairn lower` は
lowering はしますがバージョンを固定しません (`info` は意図的に範囲全体を報告します)。作者に代わって
バージョンを選ぶことはせず、比較を飛ばします。`cairn check` は block-array lowering 自体を走らせないので、
`E_UNKNOWN_ABSTRACT_TOKEN` を含む lowering 段の診断はそもそも届きません。

推奨修正は同じ表に対するタイポ検索です (`oak_plank` には `oak_planks` を返します)。**リネーム** は
タイポではなく (Bedrock が Java の `light` を `light_block` と呼ぶのは 6 編集離れています)、無関係な
最近傍ブロックを提示する代わりに「候補は無い」と明言します。これを埋めるにはエディション別のエイリアス
表が要りますが、パックはまだ持っていません。

同じバージョン単位のスコープは、パック自身のマテリアル対応にも適用されます。エントリは綴りが異なる
バージョンを名指す `overrides` を持てます。これが 1 つの `@floor.stone.smooth` をリネームをまたぐ
範囲で解決させています。`since` の側はまだ保留です。表が記録するのはあるバージョンがどの ID を
*持つか* であって、どのバージョンで導入されたかではないので、上の `E_VERSION_CAP` の例はレジストリ推定
ではなく `@requires` の下限です。

`def` と `theme` は `requires version>=X` を宣言できます。合成物の最小バージョンは構成要素の最大値です。
モジュールレベルの `@requires` は実装済みで、メンバレベルの形はまだパースされません。

### 宣言された下限は強制する

モジュールの `@requires` の下限は、その `@requires` 行のうち最も厳しいものです。それを下回る
`cairn compile --target` は `E_VERSION_CAP` で、成果物を用意する前に報告されるので、拒否されたビルドは
構造ファイルもロックも残しません。この順序が肝心です。ロックは検証済みの内容を記録するもので、ソース
自身が排除しているターゲットに対して `verified: true` と言ってはならないからです。

`E_REQUIRES_CONFLICT` は **予約** です。宣言された下限がレジストリから *推定された* 範囲と矛盾する
ことと定義されていますが、推定範囲はまだ導出されません (パックが `since` / `until` を持たないため)。
2 つの `@requires` 行の衝突ではありません。下限は最も厳しいものへ畳まれるので、積が空になることは
ありません。`version<1.20` のような上限を要する制約は言語が受け付けない形で、`E_INVALID_REQUIRES`
になります。

### 順序付けと、その限界

§10.1 は `DataVersion` を正準の順序キーとしています。現在のバージョン比較は代わりに **ドット区切り
10 進の要素ごとの比較** で、これは第 2 の規約ではなく既知の不足です。パックは `DataVersion` 表を同梱
していますが、障害はその不在ではなく網羅性です。表が名指すのはパックがビルドされた対象バージョンだけ
なのに対し、下限は任意のバージョンを名指せます。

任意のラベルに答えられるようになるまでは、Cairn が順序付けられないバージョンは後段で誤って並べるのでは
なくディレクティブの時点で拒否します。プレリリース、スナップショット、日付ベースのラベル
(`1.21.4-rc1`、`24w14a`) は `@requires` では受け付けません。

受け付ける範囲の中でも、この規約は 2 つを取り違えます。

- **日付ベースのラベルと semver のラベルの比較。** §10.1 が存在する理由そのものの移行であり、ドット
  区切り 10 進の比較は生き残れません。
- **2 つのエディションの採番を 1 つのものとして比較すること。** Java は `1.20.4 / 1.21 / 1.21.4`、
  Bedrock は `1.21.0 / 1.21.40 / 1.21.60` と進みます。下限はエディションを持たないので
  `@requires version>=1.21.4` は Bedrock `1.21.40` で満たされたと読まれ (`40 > 4`)、Java の採番では
  下限未満のバージョンに対してビルドが認証されます。`@requires` がそもそもエディション中立でよいのかは
  未決の言語課題です。

## 10.5 「どのバージョン用か?」には 3 つの答えがある

単一の「対応バージョン」はありません。`cairn info` は 3 つの軸を報告します。

1. **レジストリ互換範囲 `[Vmin, Vmax]`** — 使用トークン/状態にわたる `since`/`until` の積。
2. **意味的に敏感なメンバ** — ID は有効なまま、意味・挙動・見た目が変わる箇所。範囲より重要です。
   挙動の変化は ID の消滅よりはるかに頻繁なので、レジストリだけから Vmax を決めるのは危険です。制約
   カタログは `since`/`until` とは別に `semantic_sensitivity` (境界バージョン + 理由) を持ち、それを
   またぐコンパイルで警告を出します。例: 1.17 の cauldron 分割、1.16 の壁接続の bool →
   `none/low/tall`、1.20.5 のアイテムフォーマット。
3. **検証済みのロックターゲット** (§10.6)。

```text
$ cairn info build.crn --editions java,bedrock
registry compatibility:  1.21.40 .. latest
edition portability:     Java: portable: 42  degraded: 0  unsupported: 0   Bedrock: portable: 38  degraded: 3  unsupported: 1
buildable targets:       Java: none (1.20.4, 1.21, 1.21.4 all refuse)   Bedrock: 1.21.40, 1.21.60 (1.21.0 refuses)
semantic-sensitive:      yard_water(cauldron split@1.17), fence(wall conn@1.16)
```

名前が出るバージョンはすべて同梱パックが宣言しているものです。この出力の元のファイルは
`@requires version>=1.21.40` を持ち、それが Java の全ターゲットと Bedrock 1.21.0 を下限未満にしています。

4 行は stdout、各数字の内訳は stderr の `note:` 行へ出ます。行を読むパイプラインは、`cairn info` が
完走するかぎり毎回同じ 4 行を見ます。完走しない場合は別です。所見が行を 1 つも計算する前にコマンドを
拒否するので、stdout は行が足りないのではなく空になります。

### `edition portability` の行

この行はパレットのエントリを数えます。エントリが `unsupported` になる理由は 4 通りです。

| 理由 | 修復 |
|---|---|
| そのエディションにそのブロックが無い。 | マテリアルを変えるか、パックの対応付けを変える。 |
| ブロックはあるが、intent が持つステートに対する対応を Cairn が持たない (§10.7)。 | まだ無い — 対応を追加するのは Cairn 側。 |
| Java のドメイン外のステート値がステート変換器に届いた。 | 無し — 本来パックが拒否すべきだが、値ドメインを表せるパックスキーマが今は無い。 |
| 変換器が読まないステートキーが届いた。 | ソースのブロックステートからそのキーを消す。 |

最初の 1 つは ID の話、残り 3 つはステートの話です。`degraded` になりうるのは 2 番目だけです。存在
しないブロックには失う詳細がなく、変換器が拒否するステートは部分的な欠落ではありません。3 番目と
4 番目はそもそも可搬性の事実ではなく、上流が通してはいけないブロックステートを通した結果です。

1 つの数字の裏に 4 通りの修復が隠れるので、数えられた各エントリは理由とともに stderr で名指しされ、
ID のケースには `E_UNKNOWN_ID` と同じ読み方の `did you mean` が付きます。`--format json` では
`edition_portability[].unsupported_entries` として、数の 1 単位につき 1 要素、パレット順で出ます。

どちらの問いもバージョンではなく *エディション* に対して発せられます。この行が互換範囲全体にわたって
報告するものだからです。範囲の一部でしか有効でない ID (Bedrock が 1.21.40 で `stonebrick` を
`stone_bricks` にリネームした件) は `unsupported` にはなりません。実際にビルドされるバージョンが
それを持つかは `cairn compile --target` が `E_UNKNOWN_ID` として答える問いです (§10.4)。

### `buildable targets` の行

カウンタでは言えないことがあります。2 つのエントリがそれぞれ *互いに素な* バージョン集合で宣言されて
いても、どちらも「エディションは持っている」と答えるので、どのバージョンも両方を宣言していないのに行は
きれいなままです。

`buildable targets` はバージョン単位の答えです。要求されたエディションごとに、固定 lowering がエラーを
出さないサポート対象バージョンを並べ、拒否したバージョンをその横に名指しします。`[Vmin, Vmax]` の範囲
ではなく集合なのは、バージョン集合が交錯する 2 つの ID が、範囲では埋めてしまう隙間を残すからです。

導出はサポート対象バージョンごとに 1 回 lowering する方法で、`cairn compile --target` と同じ検査です。
範囲全体のパレットの ID 集合を積集合するのは **不健全** なので採りません。ターゲットを固定しないと
すべてのマテリアルが *既定* の対応を取るため、ターゲットが綴り替えるトークンが誤った ID として比較されます。

カウンタと同じく、この行は報告するだけで拒否しません。どのサポート対象バージョンでもビルドできない
ソースに対しても `cairn info` は 0 で終了します。拒否するのはビルドの仕事です。拒否した各バージョンの
所見はそのバージョンの下に印字されるので、`E_UNKNOWN_ID` がそれを出したターゲット抜きで置き去りになる
ことはありません。

5 行目の `recommended test targets` はこの軸に属し、また別の問い (どのバージョンをテストする価値が
あるか) に答えます。まだどのコードパスも出力しません。

## 10.6 Provenance とロック

`.crn` が持つのはヒントである `@intended_targets` だけです。`verified: true`、DataVersion、各種
ハッシュはロックにのみ存在し、ビルド成功時にコンパイラが書きます。手書きすることはありません。

```yaml
# build.cairn.lock (コンパイラ生成)
lock_schema_version: 1        # この文書自身のスキーマ改訂
source_hash: sha256:...
cairn_version: 2026.06        # Cairn リリースの日付バージョン (CalVer)
target: { edition: java, mc_version: 1.20.4, data_version: 3700 }
inputs: { registry_pack_hash: sha256:..., constraint_catalog_hash: sha256:... }
resolved_ir_hash: sha256:...
verified: true
member_version_sensitivity: [ { id: yard_water, reason: "cauldron split at 1.17" } ]
```

`resolved_ir_hash` が再現性の核です。マクロ展開、デフォルト補完、自動アドレス割り当ての後の IR を
固定します。

`lock_schema_version` を先頭に置くのは、読み手が残りをパースする前に理解できるか判断できるようにする
ためです。バージョン `1` は上の形で、キーを省いた文書はバージョン `1` です。より高いバージョンを宣言する
文書は、フィールド名の意味が同じままだと仮定して読むのではなく拒否します。スキーマが宣言していない
キーは、どこに現れても拒否されます。

別のターゲットで再コンパイルすると、検証済みのものとの差が大きく報告されます。

```text
$ cairn compile build.crn --target 1.21.4 --lock build.cairn.lock
W_PREVIOUSLY_VERIFIED_TARGET: verified for 1.20.4/DataVersion 3700, now 1.21.4/4189.
W_SEMANTIC_SENSITIVITY: 2 members may resolve differently: yard_water, fence
```

## 10.7 Java / Bedrock の可搬性

導出規則はエディション固有です。**`intent_state` は中立、`resolved_state` はエディション別。** 契約は
「同じ intent から、エディションごとに最も近い合法表現へ解決する」ことであり、「同じ結果を保証する」
ことではありません。

```yaml
intent_state: { primitive: stairs, corner: inner_left, facing: east }   # エディション中立
resolved_state:
  java:    { facing: east, half: bottom, shape: inner_left }
  bedrock: { weirdo_direction: 1, upside_down_bit: false }              # shape が無く角がつながらない
```

解決結果の差が見た目や機能の差になるとき、lint が知らせます。

```text
W_INTENT_DEGRADED line 12 id=roof_corner:
  shape=inner_left cannot be resolved in Bedrock (stairs have no shape state).
  Bedrock stairs render straight; visual gaps at corners.
```

正準語彙が吸収できるのは ID / ステート / シリアライズの差だけです。**概念の不在とゲーム挙動の差は吸収
しません。** 吸収できない代表例: display エンティティ (Bedrock に無い)、階段の `shape` (Bedrock には
ステートが無い)、armor_stand のポーズ、レッドストーンの伝播、アイテムコンポーネント ↔ Bedrock の
アイテム NBT、light ブロックの内部挙動。

### 代替を書くとき

意味層での `@edition` 条件分岐は禁止です。代替が必要なときは、この階層を上から順に使います。

1. クローズドな意味プリミティブ (中立) を使う。表現できなければ `E_PARITY_UNSUPPORTED` で fail-loud。
2. **スロット + エディション別テーマ** でフォールバックする (`floating_text` スロットを Java では
   `text_display`、Bedrock では光る看板に解決)。
3. エスケープハッチ層でのみ `@edition` でガードする。生の ID や NBT は本質的にエディション固有です。

```
hologram id=shop_sign text="Weapon" mat_slot=floating_text   # 意味層は常に中立
theme shop_java:    slot floating_text -> text_display scale=2.0
theme shop_bedrock: slot floating_text -> sign glowing=true   # Bedrock フォールバック

@edition java    { raw_block mat=minecraft:light[level=15] at=4,3,2 }
@edition bedrock { raw_block mat=minecraft:light_block["block_light_level"=15] at=4,3,2 }
```

### バリアントを選ぶのはビルドであってソースではない

`theme NAME_java` と `theme NAME_bedrock` は論理テーマ `NAME` の 2 つのバリアントを宣言します。
`--edition` のピンはそのエディションのバリアントを束縛し、無ければ接尾辞の無い `NAME` にフォール
バックし、そこで止まります。*もう一方の* エディションのバリアントを束縛すると、そのスロット値がこの
エディションの出力に流れ込みます。§10.4 が禁じるサイレント置換です。どちらも無ければ、要求された体積を
空気で建てるのではなく `E_THEME_VARIANT_MISSING` でコンパイルを止めます。

`place ... theme=NAME` は **論理** テーマを名指し、まさにこの規則に従います。1 つの site が、ビルドが
必要とするバリアントで同じ def を配置できます。そこでバリアントを名指しても (`theme=shop_bedrock`)
解決はします。ピンが選んだバリアントを束縛し、`W_THEME_VARIANT_REBOUND` が代わりに束縛されたものを
告げます。ただし意味層が担うべきなのは中立な綴りです。

`--edition` のピンが無ければ、作者が名指したバリアントを選び直すことはありません。宣言された名前は
そのまま束縛されます。接尾辞 *無し* で書かれた名前は、モジュールレベルの選択と同じピン無しの順序で解決
されます。モジュールが宣言していない接尾辞 *付き* の名前は `E_UNRESOLVED_THEME_REF` です。

### バージョンをまたぐ適用

非対称で、意図的にそうしています。

- **ダウングレード** (新バージョンの NBT → 旧バージョンのワールド) はハードエラーです。未知の
  コンポーネントはクラッシュや破損を招きます。
- **アップグレード** (旧バージョンの NBT → 新バージョンのワールド) は大きな警告 + DataVersion スタンプ
  で、DFU 依存です。明示的な `--allow-cross-version` を要します。

すべてのビルドがエディション可搬である必要はありません。コンパイラの仕事は、何が可搬性を壊すかを述べる
ことです。

## 10.8 Cairn 自身の面の互換性ティア

上の `(edition, version)` 軸が扱うのは Cairn が *出力するもの* です。直交する軸は、Cairn が自身の進化に
ついて約束することです。`.crn` 構文、ロックファイル、CLI フラグ、Rust API。CalVer にはそれを読み取れる
「メジャー」軸が無いので、約束は [互換性ティア](compatibility) に明記します。`Stable` な面は
`W_DEPRECATED` で 1 リリース分の猶予があり、`Evolving` な面は任意の月次マイナーで変わり、`Internal` は
何も約束しません。
