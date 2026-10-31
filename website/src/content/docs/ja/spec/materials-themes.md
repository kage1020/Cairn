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

テーマが割り当てるのは、生のブロック ID ではなく **正規トークン** です。`(edition, version)` ごとに ID、ステート名、ステートの値、シリアライズの形式を解決するのはバックエンドです ([バージョンとエディション](/ja/spec/versioning-editions/))。LLM は、`pillar_axis` も、リトルエンディアンの NBT も、Bedrock の `weirdo_direction` も知る必要がありません。

現時点のバックエンドが解決する範囲は、これより狭くなっています。正規トークンは `minecraft:<token>` としてそのまま書き出し、指定したターゲットと照らし合わせるだけなので ([§7.3](#73-バージョンとエディションをまたぐ対応付け))、`@water_cauldron` はどの Bedrock のバージョンでも `E_UNKNOWN_ID` になります。また、Bedrock への書き出しでステートを変換できるのは階段の系統だけで、`@oak_log[axis=x]` は Java 向けにはコンパイルできても、Bedrock 向けには拒否されます。

トークンには 2 つの種類があります。

| 種類 | 例 | 意味 |
|---|---|---|
| **正規ブロックトークン** | `@oak_planks` / `@water_cauldron` / `@oak_log[axis=x]` | Minecraft の特定のブロックを意味します。意味が変わってしまうようなダウングレードを黙って行うことは **禁止** で、`@water_cauldron` が `cauldron` になることはありません。 |
| **抽象マテリアルトークン** | `@floor.wood.broadleaf` / `@roof.dark_wood` | 見た目の好みを表します。テーマのポリシーによってダウングレードしてもかまいません (MAY) (オーク ↔ シラカバ)。 |

```
theme cottage:
  slot floor -> @floor.wood.broadleaf   # 抽象: ターゲットとポリシーで解決する
theme exact_oak:
  slot floor -> @oak_planks             # 正規: 1 対 1 に固定する
```

抽象トークンは、レジストリパックの `materials` カタログを通じてブロックに解決します。カタログのエントリの `overrides` には、そのブロックを別の綴りで書くバージョンを挙げます ([バージョンとエディション §10.4](/ja/spec/versioning-editions/#104-fail-loud-と最小バージョン推定))。カタログにないトークンは、パックを参照する場面、つまりターゲットを指定したときと `cairn lower`、`cairn info` で `E_UNKNOWN_ABSTRACT_TOKEN` になります ([Lint](/ja/spec/lint/))。

### tier-1 のトークン

組み込みの `materials` カタログが宣言する抽象トークンが **tier-1** の語彙で、[互換性ティア](/ja/spec/compatibility/#c1-ティアの定義)はこれを Stable とします。両エディションのカタログは同じトークンを宣言するので、これらだけを割り当てるテーマは、どちらのエディションでも解決します。

| トークン | 用途 |
|---|---|
| `@floor.wood.broadleaf`、`@floor.wood.conifer`、`@floor.stone.cobble`、`@floor.stone.smooth` | 床 |
| `@wall.wood.broadleaf`、`@wall.wood.conifer`、`@wall.stone.cobble`、`@wall.stone.smooth` | 壁 |
| `@wood.dark`、`@wood.light`、`@wood.warm`、`@wood.cool` | 色味で選ぶ板材 |
| `@roof.dark_wood`、`@roof.light_wood`、`@roof.warm_wood`、`@roof.cool_wood` | 色味で選ぶ屋根の階段 |
| `@sign.oak`、`@sign.oak_wall` | 立てた看板と壁の看板 |
| `@pressure_plate.default` | `pressure_plate` が既定で使うブロック |

tier-1 のトークンが解決するブロックは **tier-2** で、Evolving です。これはターゲットに対するカタログの答えで、リリースによって変わることがあります。カタログにトークンを足しても何も壊れません。トークンの名前を変えたり廃止したりするのは Stable な面の破壊的変更なので、非推奨期間のあいだは古いトークンも `W_DEPRECATED` 付きで解決し続けます。正規ブロックトークンは Cairn ではなく Minecraft 自身の名前なので、ティアを持ちません。`@oak_planks` がターゲットに存在するかどうかは、レジストリパックが答えます ([§7.3](#73-バージョンとエディションをまたぐ対応付け))。

## 7.3 バージョンとエディションをまたぐ対応付け

正規トークンは、次の 5 つのパターンの違いを吸収します。解決に使う表の構造は[バージョンとエディション](/ja/spec/versioning-editions/)で説明します。

| パターン | 例 | 扱い |
|---|---|---|
| 1 対 1 の改名 | `@dirt_path` (旧 `grass_path`) | 自動で解決します。 |
| 1 対 N の分割 | `@cauldron[fluid=water]` → `water_cauldron` | 意味を表すトークンで区別します。 |
| N 対 1 の統合 | `@oak_slab` (旧 `wooden_slab{variant}`) | ターゲットごとに解決します。 |
| 新規追加 | `@cherry_planks` | `requires >=` で下限を指定する必要があります。 |
| 削除 | ターゲットのバージョンに存在しない | ハードエラーにして、代わりの候補を示します。 |

現時点で書いたとおりに動くのは、最後の行だけです。 正規トークンは `minecraft:<token>` としてそのまま書き出し、指定したターゲットと照らし合わせます ([バージョンとエディション §10.4](/ja/spec/versioning-editions/#104-fail-loud-と最小バージョン推定))。改名は自動では解決されず `E_UNKNOWN_ID` になり、そのターゲットでの綴りをレジストリパックの `aliases` から注記で示します (Bedrock で `@dirt_path` と書くと `minecraft:grass_path` を示します)。改名をまたいで解決できるのは抽象トークンだけで、カタログのエントリの `overrides` を通じて解決します ([§7.2](#72-正規トークン))。分割も区別しません。`@cauldron[fluid=water]` は `minecraft:cauldron[fluid=water]` のまま書き出し、`W_STATE_LITERAL_UNCHECKED` を出します。統合された id も、ほかの id と同じくターゲットごとに照らし合わせるだけです。新しいブロックから下限を推定することもありません。`@requires` のない `@pale_oak_planks` は Java 1.21.4 では問題なく通り、このブロックを持たないターゲットでは `E_UNKNOWN_ID` になります。削除された id は `E_UNKNOWN_ID` になり、近いブロックがそのターゲットにあれば、それを示します。

**吸収できるのは、ID、ステート、シリアライズの違いだけです。** 概念そのものがない場合や、ゲームの挙動の違いは吸収しません。
