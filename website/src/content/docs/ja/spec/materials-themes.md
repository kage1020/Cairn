---
title: "7. マテリアルとテーマ"
---

## 7.1 依存性注入としてのスロット

**変更の余地あり。** セレクタのバインディング、つまりセレクタ行の矢印の右にある `key=value` は予約されたままで、まだ組み上げられません ([互換性ティア](/ja/spec/compatibility/#変更の余地あり))。

構造の側には、具体的なブロック名を書きません。構造が持つのは `mat_slot` という注入ポイントだけで、そこにスロットやセレクタを通じて値を割り当てるのが `theme` です。CSS や依存性注入と同じ考え方です。これにより、構造 (壁がどこにあるか) とスタイル (どのブロックを使い、どう飾るか) が分かれます。

```
def cottage class=house size=9x7:
  floor  id=floor  mat_slot=floor
  walls  id=walls  class=outer mat_slot=wall height=4
  roof   id=roof   kind=gable  mat_slot=roof
  window id=front_windows class=small side=front y=2 offset=1 size=2x2 repeat=2 step=5 mat_slot=glass

theme medieval:
  slot floor -> @spruce_planks
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  slot glass -> @glass_pane
  walls[class=outer]  -> trim=@spruce_log     # セレクタによる部位の装飾 (予約)
  window[class=small] -> frame=@spruce_wood
```

**カスケード。** メンバーは、自分にマッチするすべてのセレクタ行の割り当てを、ソース上の順番に集めるので、2 つの行が同じキーに値を割り当てていれば、メンバーには後の値が残ります。CSS で同じ詳細度の 2 つのルールに適用される規則と同じです。ただし割り当ては予約中 (後述) なので、今のところカスケードが決めるのはメンバーが何を集めるかだけで、作られるものには影響しません。

属性が部分的に重なる行は、この性質を利用します。`window[class=small,side=front]` は、自分が選ぶメンバーについて `window[class=small]` の値を上書きします。広いほうの行だけが選ぶメンバーには、広いほうの行の割り当てが残ります。

一方、*同じ* メンバーを選ぶ 2 つの行は事情が違います。キーワードも属性も同じなら、2 つの行はメンバー単位で完全に一致するので、両方の行が割り当てるキーについて、前の行の値が残るメンバーは 1 つもありません。割り当てが今後どんな意味を持つことになっても、その値は効かない記述です。これが `E_DUPLICATE_SELECTOR` です ([Lint §11.1](/ja/spec/lint/#111-診断コード))。割り当てが予約中の今もエラーにするのは、1 つの行の中で 2 回書いたキーを `E_DUPLICATE_ARG` が拒否するのと同じ理由です ([Lint §11.3](/ja/spec/lint/#113-エラーと警告の区分))。同じかどうかは意味で判断します。属性の順番は問わず、`class=` / `id=` / `mat_slot=` の値はラベルの文字列として比べるので、`window[class=small]` と `window[class="small"]` は同じセレクタです。セレクタが同じでも、別々のキーを割り当てる行は報告しません。それらは合成されるので、長い割り当ての並びを 2 行に分けて書いてもかまいません。

**セレクタによる割り当ては予約中です。** 行がキーワードごとにどのキーを割り当てられるか、それぞれのキーが何を塗るかは、まだ定めていません。割り当てを読む lowering もありません。上の例の 2 つのセレクタ行があってもなくても、作られる cottage は同じで、どのメンバーのブロックもこれらの行に左右されません。

これが定まるまで、コンパイラはキーワードを知っている行のすべての割り当てを、まだ使われないキーとして `W_IGNORED_ARGUMENT` で報告します ([Lint](/ja/spec/lint/))。キーや値が何であっても、行がマッチしたかどうか、テーマが適用されたかどうかにかかわらず報告します。キーワードを知らない行は、代わりに `E_UNKNOWN_KEYWORD` になります。キーが何を塗るのかが決まっていないので、割り当ての値をブロックとして解決することもしません。`frame=@no_such_block` も `frame=@spruce_wood` や `frame=42` とまったく同じように報告され、`--target` の有無も関係ありません。割り当てが原因で `check`、`compile`、`info` がビルドを拒否することはありません。

ただし、同じメンバーに同じキーを 2 回割り当てると拒否します。割り当てが今後どんな意味を持つことになっても、前の値は効かない記述だからです。1 つの行の中なら `E_DUPLICATE_ARG`、同じメンバーを選ぶ 2 つの行にまたがるなら `E_DUPLICATE_SELECTOR` です。行の割り当て以外の部分は検査します。`E_THEME_SELECTOR_UNMATCHED` は、その診断の項目に書かれたとおりに適用されます。

`def`、`theme`、`site` は、どれも同じ「スロットを持つコンポーネント」の仕組みで統一されています ([コンポーネント・編集・複数建築](/ja/spec/components-editing-sites/))。

## 7.2 正規トークン

テーマが割り当てるのは、生のブロック ID ではなく **正規トークン** です。`(edition, version)` ごとに ID、ステート名、ステートの値、シリアライズの形式を解決するのはバックエンドです ([バージョンとエディション](/ja/spec/versioning-editions/))。LLM は、`pillar_axis` も、リトルエンディアンの NBT も、Bedrock の `weirdo_direction` も知る必要がありません。正規の語彙は、ステート名とステートの値を Java と同じに綴ります。`@oak_stairs[facing=east]` は Java の `facing` を書き、Bedrock のバックエンドがそれを自分の `weirdo_direction` に写します。

正規トークンは、固定したターゲットがその ID を宣言していれば `minecraft:<token>` として、宣言していなければそのターゲットの表を通して書き出すので ([§7.3](#73-バージョンとエディションをまたぐ対応付け))、`@water_cauldron` は Bedrock では水の入った `cauldron` として書き出します。ステートはそのトークンの Java ブロックが取るもので、Bedrock のライターはターゲットのバージョンに対するパックのステートマップでそれを写します。`@oak_log[axis=x]` はどの Bedrock のバージョンでも `pillar_axis=x` として書き出しますが、`@cherry_wood[axis=x]` は、マップにその形がない 1.21.40 では `E_STATE_UNMAPPED` になります。

トークンには 2 つの種類があります。

| 種類 | 例 | 意味 |
|---|---|---|
| **正規ブロックトークン** | `@oak_planks` / `@water_cauldron` / `@oak_log[axis=x]` | Minecraft の特定のブロックを意味します。意味が変わってしまうようなダウングレードを黙って行うことは **禁止** で、`@water_cauldron` が空の `cauldron` になることはありません。 |
| **抽象マテリアルトークン** | `@floor.wood.broadleaf` / `@roof.dark_wood` | 見た目の好みを表します。テーマのポリシーによってダウングレードしてもかまいません (MAY) (オーク ↔ シラカバ)。 |

```
theme cottage:
  slot floor -> @floor.wood.broadleaf   # 抽象: ターゲットとポリシーで解決する
theme exact_oak:
  slot floor -> @oak_planks             # 正規: 1 対 1 に固定する
```

抽象トークンは、レジストリパックの `materials` カタログを通じてブロックに解決します。カタログのエントリの `overrides` には、そのブロックを別の綴りで書くバージョンを挙げます ([バージョンとエディション §10.4](/ja/spec/versioning-editions/#104-fail-loud-と最小バージョン推定))。カタログにないトークンは、パックを参照する場面、つまりターゲットを指定したときと `cairn lower`、`cairn info` で `E_UNKNOWN_ABSTRACT_TOKEN` になります ([Lint](/ja/spec/lint/))。

### tier-1 のトークン

組み込みの `materials` カタログが宣言する抽象トークンが **tier-1** の語彙で、[互換性ティア](/ja/spec/compatibility/#c1-ティアの定義)で Stable と定めています。両エディションのカタログは同じトークンを宣言するので、これらだけを割り当てるテーマは、どちらのエディションでも解決します。

| トークン | 用途 |
|---|---|
| `@floor.wood.broadleaf`、`@floor.wood.conifer`、`@floor.stone.cobble`、`@floor.stone.smooth` | 床 |
| `@wall.wood.broadleaf`、`@wall.wood.conifer`、`@wall.stone.cobble`、`@wall.stone.smooth` | 壁 |
| `@wood.dark`、`@wood.light`、`@wood.warm`、`@wood.cool` | 色味で選ぶ板材 |
| `@roof.dark_wood`、`@roof.light_wood`、`@roof.warm_wood`、`@roof.cool_wood` | 色味で選ぶ屋根の階段 |
| `@sign.oak`、`@sign.oak_wall` | 立てた看板と壁の看板 |
| `@door.default` | `mat_slot=` で割り当てていない `door` が置くドアのブロック |
| `@chest.default` | `mat_slot=` で割り当てていない `chest` が置くチェストのブロック |
| `@pressure_plate.default` | `mat_slot=` で割り当てていない `pressure_plate` が置くブロック |
| `@lever.default` | `lever` が壁に掛けるレバー |
| `@button.default` | `mat_slot=` で割り当てていない `button` が壁に掛けるボタン |
| `@daylight.default` | `daylight` が置く日照センサー |
| `@observer.default` | `observer` が置くオブザーバー |
| `@lamp.default` | `lamp` が置くレッドストーンランプ |
| `@piston.default` | `piston` が置くピストン |
| `@piston.sticky` | `sticky=true` の `piston` が置く粘着ピストン |
| `@dispenser.default` | `dispenser` が置くディスペンサー |
| `@repeater.default` | `repeater` が置くリピーター |
| `@comparator.default` | `comparator` が置くコンパレーター |
| `@redstone_torch.default`、`@redstone_torch.wall` | `redstone_torch` が置く、立てたトーチと壁付きトーチ |
| `@redstone_dust.default` | `redstone_dust` が敷くダスト |
| `@circuit.block` | struct の回路が床下でダスト、トーチ、リピーター、コンパレーターを載せるブロック |

tier-1 のトークンが解決するブロックは **tier-2** で、Evolving です。ターゲットごとにカタログで決まり、リリースによって変わることがあります。カタログにトークンを足しても何も壊れません。トークンの名前を変えたり廃止したりするのは Stable な面の破壊的変更なので、非推奨期間のあいだは古いトークンも `W_DEPRECATED` 付きで解決し続けます。正規ブロックトークンは tier-1 でも tier-2 でもありません。[§7.3](#73-バージョンとエディションをまたぐ対応付け) の対応付けを通じてブロックを指す名前で、ターゲットが受け付ける名前はそのレジストリパックで決まり、[互換性ティア](/ja/spec/compatibility/#evolving)で Evolving と定めています。

## 7.3 バージョンとエディションをまたぐ対応付け

正規トークンは、次の 5 つのパターンの違いを吸収します。解決に使う表の構造は[バージョンとエディション](/ja/spec/versioning-editions/)で説明します。

| パターン | 例 | 扱い |
|---|---|---|
| 1 対 1 の改名 | `@chain` (Java 26.3 での `iron_chain`) | ターゲットごとに自動で解決します。 |
| 1 対 N の分割 | `@cauldron[fluid=water]` → `water_cauldron` | 意味を表すトークンで区別します。 |
| N 対 1 の統合 | `@stone_bricks` (Bedrock 1.21.0 での `stonebrick[stone_brick_type=default]`) | ターゲットごとに解決します。 |
| 新規追加 | `@pale_oak_planks` | それを宣言する最初のターゲットより下では拒否します。 |
| 削除 | ターゲットのバージョンに存在しない | ハードエラーにして、代わりの候補を示します。 |

正規トークンは `minecraft:<token>` として書き出し、固定したターゲットと照らし合わせます ([バージョンとエディション §10.4](/ja/spec/versioning-editions/#104-fail-loud-と最小バージョン推定))。その前に、ターゲットのレジストリパックの 3 つの表が、次の順にトークンに答えます。

1. **意味トークン**。分割に対するものです。パックの `meanings` 表は、ブロックが複数に分かれたトークンごとに、その意味トークン (ブロックが持たないプロパティで、その値が分かれた先の 1 つを選ぶもの) と、各値が指すブロックを挙げます。`@cauldron[fluid=water]` は `water_cauldron` になり、`fluid` を落として、ほかのステートはすべて残します。行が挙げない値は `E_STATE_DOMAIN` になり、行が挙げる値を示します。
2. **改名**。固定したターゲットがトークンの ID を宣言していない場合です。エディションのどこかのバージョンがそれを宣言し、その `aliases` のグループが、エディションの範囲の中で改名された 1 つのブロックで、どのバージョンもその綴りを 2 つは宣言しないなら、トークンはターゲットが宣言するただ 1 つの綴りで書き出します。Java 26.3 は `@chain` を `iron_chain` として、Java 1.21.4 は `@iron_chain` を `chain` として書き出します。
3. **ステートマップ**。Bedrock で、固定したバージョンがその ID を宣言していない場合です。そのバージョンのステートマップが Bedrock のブロックとして書き出す Java のブロックは、そこに写るほかの Java のブロックが同じ形を 1 つも取らなければ、そのブロックとして書き出します。こうすれば、書き出したものからどのブロックだったかがわかります。`@oak_sign` は `standing_sign` として、1.21.0 での `@stone_bricks` は `stonebrick[stone_brick_type=default]` として書き出し、後者はターゲットごとに解決する統合です。パレットには Java のブロックとそのステートを残し、ライターはほかのブロックと同じようにそれを写します。ベッドはこのようには書き出しません。Bedrock はベッドの色も植木鉢の植物もブロックエンティティに持つので、そのブロックでは区別できず、`@black_bed` はそこでは `E_UNKNOWN_ID` のままです。

どの表も答えないトークンは `E_UNKNOWN_ID` になり、パックの `aliases` にそのターゲットでの綴りがあれば注記で示します。Bedrock 1.21.60 での `@stonebrick` には、1.21.40 がそれを分割した 4 つのブロックを返します。1 つを選べば [バージョンとエディション §10.4](/ja/spec/versioning-editions/#104-fail-loud-と最小バージョン推定) が禁じるサイレント置換になるからです。ターゲットを固定しない場合、ターゲットにできるいずれかのバージョンが別の綴りで書き出すトークンは、そのまま通します。そのため `cairn check` と `cairn info` の範囲全体のパスは、固定したビルドが書き出すものを拒否しません。生のブロックは自分が書き出す ID を名指し、これらの表を通しては読みません ([§7.4](#74-生のブロック))。抽象トークンは、カタログのエントリとそのエントリの `overrides` を通じて解決します ([§7.2](#72-正規トークン))。`cairn info` は、あるバージョンが別の ID で書き出す各トークンを、書き出す ID とともに示します ([バージョンとエディション](/ja/spec/versioning-editions/#respelled-の行))。

新しいブロックから下限を推定することはありません。`@requires` のない `@pale_oak_planks` は Java 1.21.4 では問題なく通り、このブロックを持たないターゲットでは `E_UNKNOWN_ID` になって、それを宣言する最初のターゲットとして 1.21.4 を示します。`cairn info` は Java の推定範囲をそこから始めます ([バージョンとエディション](/ja/spec/versioning-editions/#inferred-range-の行))。削除された id は `E_UNKNOWN_ID` になり、それを宣言する最後のターゲットを示します。推定範囲もそこで終わるので、そのターゲットより上の下限は `E_REQUIRES_CONFLICT` になります。どのバージョンも宣言していない id なら、近いブロックがそのターゲットにあれば、それを示します。

**吸収できるのは、ID、ステート、シリアライズの違いだけです。** 概念そのものがない場合や、ゲームの挙動の違いは吸収しません。

## 7.4 生のブロック

テーマは正規トークンを割り当て、メンバーはスロットの解決先から作られます。それで足りないところでは、2 つのメンバーがブロックそのものを名指します。これが **エスケープハッチ** で、`raw` フェーズで組み上げます ([コンパイルモデル §4.1](/ja/spec/compilation/#41-フェーズ評価))。

```
raw_block mat=minecraft:light[level=15] at=4,3,2
raw_fill  mat=minecraft:tinted_glass from=1,2,0 to=3,3,0
```

`mat=` は **ブロック ID** で、`namespace:path` の形をとり、接して続くステートリテラルを付けられます ([構文 §5.1.1](/ja/spec/syntax/#511-リテラルと区切り))。`raw_block` は、`at=` が指す 1 つのセルにそのブロックを置きます。`raw_fill` は、`from=` と `to=` のあいだの箱のすべてのセルに置きます。`fill` と同じく、2 つは向かい合う角で、どちらを先に書いてもかまいません ([ブロックステート §6.3](/ja/spec/blockstate/#63-waterlogged))。どれも座標の組で、外形の低い側の角から、`level y=N` の下ではその `level` から測ります。

生のブロックは、テーマとスロットを経由しません。`mat_slot=` が割り当てることはないので、行に書いた `mat_slot=` は `W_IGNORED_ARGUMENT` になります。抽象トークンやカタログの既定値が代わりに入ることもなく、テーマが割り当てられていない struct でも組み上げます。ターゲットは経由します。バージョンを固定したビルドは、ほかのブロックと同じように ID とステートを照らし合わせます。ターゲットが宣言していない ID は `E_UNKNOWN_ID` に、ブロックのステートにないステートは `E_STATE_DOMAIN` になります ([バージョンとエディション §10.4](/ja/spec/versioning-editions/#104-fail-loud-と最小バージョン推定))。Bedrock では、ステートは正規の語彙で、正規トークンと同じくステートマップで書き出すので ([§7.2](#72-正規トークン))、マップに形がないステートは `E_STATE_UNMAPPED` になります。

ブロックは書いたとおりに書き出します。`raw` フェーズはほかのどのフェーズよりも後に走るので、生のブロックは、ほかのメンバーがそのセルに置いたもの (空気も含む) を置き換えます。どのステートも導出しません。先に水を注いでいても水浸しにはならず、隣とつなぐことも、隣のチェストと連結することもありません。ただし、隣のブロックは、ほかのブロックと同じように生のブロックへつながります。そのとき読むのは、生のブロックが書いたつながりのステートと、書いていないものについてのブロックの既定値です。辺を書いていないガラス板は、下の壁から見ると単独の柱です ([ブロックステート §6.1](/ja/spec/blockstate/#つながり))。`door` の片方の半分に生のブロックを置くと、その半分が置き換わり、ゲームがもう片方を取り除きます。その行がそのブロックを求めているので、警告は出しません。2 つの生のメンバーが 1 つのセルに書くと、同じフェーズで競合し、後の行が `W_PHASE_CONFLICT` とともに勝ちます。生のメンバーは体積を決めません。`fill` と同じく、外形の外にあるセルや、書かれたとおりの struct が届くどの段よりも上にあるセルは `E_INVALID_MEMBER` に、struct が届くが建たない段にあるセルは `W_DEFERRED_MEMBER` になります。

`mat=` がないか、ブロック ID でなければ `E_INVALID_MEMBER` です。スロットが割り当てる正規トークンの `@stone` もこれにあたります。`at=`、`from=`、`to=` がないか座標の組でない場合も同じです。`mat=` はセルが建つかどうかにかかわらず判定するので、段のために延期された生のメンバーや、struct の外にあるセルのために拒否された生のメンバーも、`mat=` に応じた指摘を受けます。

生のメンバーは `@edition` のガードの外に書いてかまいません。生の ID はもともとエディション固有で、ガードのないものも、固定したターゲットと照らし合わせるので誤りは見逃されません。`minecraft:light` は Java では組み上がり、このブロックを `light_block`、1.21.40 からは `light_block_0` … `light_block_15` と綴る Bedrock では `E_UNKNOWN_ID` になります。両方のエディション向けにビルドするファイルは、両方が宣言する ID を書くか、`@edition` の下にエディションごとの分岐を 1 つずつ書きます ([バージョンとエディション §10.7](/ja/spec/versioning-editions/#代替を書くとき))。

`raw_repeat` と、取り込みが書き出したものに付く `origin=imported` は、取り込みの側のものです ([エコシステム連携 §12.3](/ja/spec/ecosystem-interop/#123-写し取りの-3-段階))。取り込みはまだありません。それまでは、`raw_repeat` は `E_UNKNOWN_KEYWORD`、`origin=` は `E_UNKNOWN_ARGUMENT` です。
