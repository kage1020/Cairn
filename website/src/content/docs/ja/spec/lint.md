---
title: "11. Lint と制約検証"
---

コンパイラは行番号付きの警告とエラーを返します。すべてのメッセージは自己修正の三つ組、すなわち
**何が間違っているか / ターゲットで有効な候補 / 推奨される修正** を持たなければなりません。
[評価フレームワーク](evaluation) のループに乗せるためです。

## 11.1 診断コード

### 重複

| コード | 意味 |
|---|---|
| `E_DUPLICATE_SIZE` | ヘッダに `size=` が 2 つ以上ある。 |
| `E_DUPLICATE_SLOT` | `theme` 本体が同じスロットを 2 回宣言している。 |
| `E_DUPLICATE_ARG` | 1 つの引数リストで `key=` が繰り返されている。 |
| `E_DUPLICATE_ID` | 同一ボディスコープ内の 2 つのメンバが `id=` を共有している。 |
| `E_DUPLICATE_SELECTOR` | 1 つの `theme` 内の 2 つのセレクタ行が、同じメンバを選び同じキーを束縛している。 |
| `E_DUPLICATE_ITEM` | 同じ種別のトップレベル項目 2 つが名前を共有している。 |
| `E_DUPLICATE_HEADER` | 単一値の `@directive` が 2 回以上宣言されている。 |

`E_DUPLICATE_SELECTOR` はセレクタを字面ではなく意味で比較します。属性の順序は関係なく、`class=` /
`id=` / `mat_slot=` はラベルテキストとして比較されるので `small` と `"small"` は同じ値です。
*違う* キーを束縛する行は合成されるので報告されません。属性が部分的に重なるだけの行も同様です
([マテリアルとテーマ §7.1](materials-themes#71-依存性注入としてのスロット))。

`E_DUPLICATE_ITEM` は `theme` / `def` / `struct` / `site` を 4 つの別々の名前空間として扱うので、 1
つの名前は各種別に 1 回ずつ現れられます。前 3 つでは最初の宣言が解決し、残りは何も束縛しません。同名
の `site` ブロック 2 つは代わりに `site::NAME::PLACE_ID` の名前空間を共有してマージされます。 `id=`
が異なる place はすべてビルドされ、衝突するのは `id=` の重複だけですが、`east_of=` はブロックをまた
いで届きません。

`E_DUPLICATE_HEADER` の対象は `@cairn` と `@intended_targets` です。`@requires` は除外されます。その
下限はすべての行で最も厳しいものに畳まれるので、2 つ目は制約を追加します ([§5.3](/ja/spec/syntax#53-ヘッダ))。

### 構文と構造

| コード | 意味 |
|---|---|
| `E_UNKNOWN_KEYWORD` | 文キーワードが既知キーワード表にない。 |
| `E_UNKNOWN_ARGUMENT` | そのメンバのキーワードの語彙に無い `key=`。 |
| `E_MISPLACED_MEMBER` | キーワードは既知だが、囲んでいるボディに読み手がいない。 |
| `E_UNEXPECTED_POSITIONAL` | 位置引数を読まない行に裸の値がある ([§5.1](/ja/spec/syntax#51-字句))。 |
| `E_UNSUPPORTED_NESTING` | メンバが、誰も読まないインデントされたボディを持っている。 |
| `E_TYPE_MISMATCH_LABEL` | ラベル型キーの値が識別子でも文字列でもない。 |
| `E_TYPE_MISMATCH_SIZE` | `size=` の値が `WxH` リテラルでない。 |
| `E_CONNECT_ARITY` | `connect` 行の形が `FROM.PORT to TO.PORT` でない。 |
| `E_INVALID_REQUIRES` | `@requires` の式がバージョン下限になっていない ([§5.3](/ja/spec/syntax#53-ヘッダ))。 |

`E_MISPLACED_MEMBER` は `struct` / `def` 内の `place` / `connect`、あるいは `site` の行に混ざった
ジオメトリキーワードで発火します。該当行で 1 回だけ報告され、その下にインデントされたものも一緒に
落ちます。

`E_UNSUPPORTED_NESTING`: メンバをグループ化するのは `struct` / `def` 内の `level y=N` だけです。
`site` のボディはフラットなリストです。落ちたサブツリーごとに、その根で 1 回報告されます。

`E_TYPE_MISMATCH_LABEL`: ラベル型キーは `id=` / `class=` / `mat_slot=` / `use=` / `theme=` です。
`use=` と `theme=` では、型の合わない値はリゾルバから見るとキーが無いのと区別できません。このコード
は「キーは行にあるが使えない」、`E_INCOMPLETE_PLACE` は「キーが無い」を意味します。

`E_CONNECT_ARITY`: `connect FROM.PORT to TO.PORT` が位置引数を読む唯一の形です。片側の欠落、`to`
キーワードの欠落や別トークンへの置き換え、末尾の余分な位置引数、ドット 1 つの `PLACE.PORT` 参照で
ない端点を対象にします。2 つの端点は独立した修正箇所なので別々に報告されます。

`E_INVALID_REQUIRES`: 受け付ける形は `version`、`>=`、ドット区切り 10 進バージョンの 3 つで、
空白は任意です。それ以外の演算子、バージョンの欠落、10 進数でないか `u32` に収まらない構成要素、
バージョン後の余分なテキストを対象にします。

### マテリアルとターゲット

| コード | 意味 |
|---|---|
| `E_UNKNOWN_ID` | 固定されたターゲットが宣言していない解決済みブロック ID。 |
| `E_INCOMPATIBLE_MATERIAL` | ブロックステートを付けるジオメトリを持つメンバが、それを保持できないマテリアルに束縛されている。 |
| `E_THEME_VARIANT_MISSING` | 固定されたエディションが、テーマのどのエディション別バリアントも束縛できない。 |
| `E_INCOMPLETE_PLACE` | `place` 行が `id=` / `use=` / `theme=` のいずれかを欠いている ([§9.3](/ja/spec/components-editing-sites#93-site-による複数建築))。 |

`E_UNKNOWN_ID` と `E_INCOMPATIBLE_MATERIAL` は block-array lowering 段で発生するので、報告するのは
`cairn compile` (と `cairn lower`) だけです。`cairn check` は lowering を走らせません。さらに
`E_UNKNOWN_ID` は固定されたターゲットを必要とするので、実際に出せるのは `cairn compile --target` だ
けです ([バージョンとエディション §10.4](versioning-editions#104-fail-loud-と最小バージョン推定))。

`E_INCOMPATIBLE_MATERIAL` は現時点では、階段ファミリ外に束縛された傾斜屋根または軒の `stair` を
意味します ([コンパイルモデル §4.3](compilation#43-切妻屋根のボクセル規則))。

`E_THEME_VARIANT_MISSING` は `--edition` 指定時のみ発火し、いくつのスコープが読んでいても
**論理テーマごとに 1 回** 報告されます。修正すべきは同じ `theme` ブロックの同じ 1 箇所だからです。
そのテーマを名指しする placement はすべて拒否されます。テーマを宣言しつつ `mat_slot=` を 1 つも
読まないモジュールは報告されません。ピンの有無でビルド結果が 1 バイトも変わらないからです。

`E_INCOMPLETE_PLACE` は欠けているキーをすべて列挙し、その行はビルドから落とされます。

### 真理値表

| コード | 意味 |
|---|---|
| `E_TRUTH_TABLE_EMPTY` | 行が 1 つも無い `assert truth(...)`。 |
| `E_TRUTH_TABLE_CONFLICT` | 2 つの行が同じ入力の組に違う出力を割り当てている。 |
| `W_TRUTH_TABLE_DUPLICATE_ROW` | 行が先行する行を繰り返し、かつ一致している。 |
| `W_TRUTH_TABLE_PARTIAL` | 行が割り当てていない入力の組がある。 |

`E_TRUTH_TABLE_CONFLICT` は後ろの行で報告され、同じパターンを持つ最初の行に note が付きます。評価器
がどちらを読むかは規定しません。修復はどちらの行が誤りかを決めることです。

2 つの警告が警告なのは、書かれている行はどれも本物の制約だからです。1 つの表が両方を得ることも
あります。繰り返し行は組を 1 つも埋めないためです。

### 意味カテゴリ

上のコードに加えて、lint は次を見ます。

| カテゴリ | 検査内容 |
|---|---|
| **ジオメトリ** | AABB 展開。壁の外の窓、空中に浮くドア。 |
| **attachment** | 額縁・絵画・看板・ボタン・レバー・松明が有効な取り付け面にあるか。 |
| **entity_aabb** | エンティティが壁や通路にめり込まないか、ドアの開閉弧を塞がないか、過密でないか。 |
| **support** | 吊りランタン、松明、キャンプファイア、砂利などの重力ブロックの支持条件。 |
| **fluid** | 水源 / 流れ / `waterlogged` の整合性。 |
| **version_caps / parity** | 状態やエンティティのスキーマがターゲットで使えるか ([バージョンとエディション](versioning-editions))。 |
| **edit_stability** | `intent_state` の変更が無関係なメンバの `resolved_state` に波及しないか。 |
| **redstone** | 宣言された真理値表と時相アサーションに対する tick 単位のシミュレーション。タイミング衝突、QC 依存、配線輻輳 ([レッドストーン](redstone))。 |
| **AABB 干渉** | 重なった場合は優先マージか拒否。境界ブロックステートの再解決は IR 層の責務。 |

### did you mean

クローズドな語彙に対して識別子を拒絶する診断には、``did you mean `X`?`` の note が付きます。未知の
キーワード、未知の `mat_slot=` 名、未知の `--target` バージョンが対象です。

候補を出す条件は、入力長でスケールする Damerau-Levenshtein 距離の閾値内にあることです。閾値は 1〜3
文字なら 1 編集以下、4〜6 文字なら 2、それ以上は 3 です。候補列挙 (`expected one of: ...`) は常に併
せて出力されます。

## 11.2 機械可読ペイロード

`--format json` は所見ごとに 1 オブジェクトを返します。

| フィールド | 型 | 備考 |
|---|---|---|
| `code` | string | 安定した `E_*` / `W_*` 識別子。gcc スタイル出力と同じ文字列。 |
| `severity` | string | `"error"` または `"warning"`。 |
| `line` | integer | プライマリスパン先頭バイトの 1-based 行番号。 |
| `col` | integer | 同先頭バイトの 1-based カラム (Unicode スカラー値)。 |
| `end_line` | integer | スパン終端 (排他) の 1-based 行番号。 |
| `end_col` | integer | スパン終端 (排他) の 1-based カラム。 |
| `primary` | string | 人間向けメッセージ。 |
| `notes` | array | `[{line?, col?, message}]`。空のときは省略。 |
| `data` | object | コード固有のペイロード。無いときは省略。 |

`data` は `kind` で判別する開かれたオブジェクトです。`primary` を解析せず `(code, data.kind)` で
照合してください。追加は厳密に additive なので、未知の `kind` は失敗にせず無視します。下表に無い
コードは `data` をまるごと省略します (JSON でも `null` ではなくキーごと存在しません)。

| コード | `data` ペイロード |
|---|---|
| `W_WALKWAY_BLOCKED` | `{ "kind": "walkway_blocked", "skipped": <u64> }`。フォールバックの L 字経路で既存構造と衝突してスキップされたセル数。 |
| `E_DUPLICATE_SELECTOR` | `{ "kind": "duplicate_selector", "rebound": ["frame"] }`。この行が先行する行から奪う束縛キー。末尾の `=` は含みません。常に非空。 |
| `E_UNKNOWN_ID` | `{ "kind": "unknown_id", "id", "registry", "origin", "token"?, "suggestion"? }`。後述。 |
| `E_INCOMPATIBLE_MATERIAL` | `{ "kind": "incompatible_material", "id", "required", "slot"?, "token"? }`。束縛されたマテリアル、ジオメトリが必要とするファミリ、束縛の出どころ。 |
| `E_INCOMPLETE_PLACE` | `{ "kind": "incomplete_place", "missing": ["id", "use", "theme"] }`。行が宣言していないキー。常に非空。 |
| `E_INVALID_REQUIRES` | `{ "kind": "invalid_requires", "reason", "found" }`。`reason` は `not_a_version_requirement` / `unsupported_operator` / `empty_version` / `component_not_a_number` / `component_too_large` / `trailing_tokens` のいずれか。失敗が断片を名指ししないとき `found` は空。 |
| `W_TRUTH_TABLE_PARTIAL` | `{ "kind": "truth_table_partial", "inputs": 2, "covered": 1, "missing": ["01","10","11"] }`。後述。 |

**`E_UNKNOWN_ID.origin`** は誰がその ID を選んだかを示します。修復先が違うからです。

| `origin` | 意味 | 修正箇所 |
|---|---|---|
| `authored` | ソースが ID を直接名指しした。 | 作者の行。 |
| `catalog` | レジストリパックがトークンを対応付けた。 | パックの対応付け。 |
| `builtin` | メンバのデフォルト用の行をパックが持たず、コンパイラ組み込みの ID が使われた。 | 行を追加すべきパック。 |

`token` は `catalog` と `builtin` に付随し、`authored` では省略されます。`suggestion` はタイポ閾値内
の宣言済み ID が無いときは省略され、リネームでは常に省略されます。

`E_INCOMPATIBLE_MATERIAL` も同じ考えです。`slot` はメンバが読んだ `mat_slot=` 名で、束縛が無ければ省
略されます。ドット付きの `token` (`roof.dark_wood`) なら、修正すべきはソース行ではなくパックの対応付
けです。`required` を暗黙にせず名前で持つのは、将来別のファミリが加わったときにコードを増やさずここ
の値で表せるようにするためです。

`W_TRUTH_TABLE_PARTIAL.missing` は集合そのものではなく **サンプル** です。入力 20 本なら組は 100 万
通りあります。件数は `missing.len()` ではなく `2^inputs - covered` から求めてください。総数ではなく
`inputs` を持つのは、入力リストに文法上の上限が無く `2^130` を収める整数が無いためです。

## 11.3 エラーと警告の区分

- **エラー** は放置すると意図しない結果になるもの、すなわち概念の不在、未知 ID、ドメイン外の状態です。
  サイレント置換と暗黙の削除は禁止です。
- **警告** はバージョン/エディション間の意味ドリフト、レッドストーン挙動の非保証、そして
  block-array パスが報告する部分ビルドの劣化です。不完全なのがソースではなくコンパイラ側の場合です。

`E_` / `W_` の接頭辞は severity ではありません。`W_` は部分ビルドの劣化を表し、`E_` 接頭辞の 2 つは
名前ではなく上のルールで判定されています。

- `E_UNKNOWN_SLOT_TARGET` は **エラー** です。マテリアルでない値に束縛されたスロットは、参照する
  メンバをすべて空気に落とすからです。
- `E_THEME_SELECTOR_UNMATCHED` は **警告** です。何にもマッチしないルールは何も上書きしません。

`E_UNKNOWN_ARGUMENT` は **エラー** です。理由は 1 段上の `E_UNKNOWN_KEYWORD` と同じで、キーワードの
語彙に無いキーは何も指しておらず、コンパイラがどう育っても値を読むパスは現れず、メンバは求められた
ものを持たずに建つからです。既定値を持つ引数の綴り間違いが最悪で、ビルドは既定値のまま成功し、何も
言いません。

各キーワードの語彙は閉じており、`theme` のセレクタが名指ししたキーワードの語彙だけを広げます。
theme に `window[tags=...]` と書けば `tags=` は window で何かが読むキーになり、それ以外では
なりません。逆方向は `E_THEME_SELECTOR_UNMATCHED` です。セレクタが作れるのは新しい語であって、その
キーワードがすでに持つ語から 1 編集の距離にあるものは造語ではなく「2 度書かれた綴り間違い」なので、
候補付きで拒否されます。

`W_IGNORED_ARGUMENT` は **警告** で、2 つのものを覆います。語彙にはあるが値を読めなかった `key=` は
捨てられ代わりに既定値が入ります。そして本仕様が定義していてまだどのパスも読まない `key=`
(今日それに当たるのは `window shape=` / `anchor=` と `roof footprint=` / `bounds=` です) は IR まで
運ばれて一度も参照されません。境界はキーワードです。コンパイラが知るキーワード上の仕様定義キーは
この形で報告され、知らない**キーワード**は `E_UNKNOWN_KEYWORD` で、その引数は判定されません。どちらも
ビルドをソースと食い違わせます。ルールが禁じているのは *サイレント* な置換であり、どちらも告知
されます。後者で欠けているのはソースではなくコンパイラの側で、だから拒否ではありません。autofix を
提供するかは実装で定義します。

## 11.4 制約カタログ

ゲーム内制約 (重力ブロック、取り付け条件、流体挙動、許容されない組み合わせ) はカタログ化し、
バージョンごとに管理します ([バージョンとエディション](versioning-editions))。「額縁はガラスに
掛けられない」のような制約はここに入ります。
