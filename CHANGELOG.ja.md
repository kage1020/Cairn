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

### 破壊的変更

- *(redstone)* 2 つのネットを 1 座標だけでなく 1 歩離します。ダストは隣の座標のダストを読むので、
  隣り合う行を走る 2 本は、1 座標に乗る 2 本と同じく 2 つの信号を運ぶ 1 本のダストです。ルータは
  1 座標に乗ることは防いでいましたが、隣り合うことは何も防いでいませんでした。その結果、example
  コーパスの配線済みスコープはすべて、ショートを含むレイアウトを — 終了コード 0、診断なしで —
  記述していました。`crossbar.crn` で 3 組のネットにまたがる 6 ペア、`redstone-door.crn` で
  1 組にまたがる 2 ペア。両エディションともです。

  ネットが避ける障害物の集合は、既に敷かれたダストから、そのダストと同一平面で隣接する 4 座標
  までに広がりました。同一平面まで、それ以上は広げません — 層をまたいだ 2 本が互いを読むか
  どうかはその間に何が立っているかで決まり、疑似 2.5D のモデルはそれを持ちません。層をまたいで
  1 歩以内にある 2 本を隔てるのはルータではなく物理タイル層の義務であることを、`spec/redstone`
  §14.5 に明記しました。この変更後も example コーパスには 1 エディションあたり 9 対 — 真上に
  重なった 1 対と、1 歩ずれた 8 対 — が残ります。いずれも、避けるために登った脱出が、避けた
  相手の真上か隣に着地したものです。

  ルータだけではこの規則を担えません。それを言ったのは測定です。セル行が `z = 0` の端に立っている
  限り、`crossbar.crn` の 4 本のネットをどの順に敷いてもスコープは配線できません — 24 通りすべて、
  `void=3` の `12x8` まで測ったどの領域サイズでも、そしてセル間隔を広げてもパッド間隔を広げても
  同じです。端に接したセルには脇のレーンが 1 本しかなく、ダストが隣に届く以上 1 レーンは 1 ネット
  しか運べない一方、2 入力ゲートには 3 本のネットが接します。そこで配置パスは行を 1 行内側の
  `z = 1` に敷き、I/O パッドは `z = 0` から並びます — パッドが空けていた行に、セルが移りました。
  これはセル 1 つにつき 1 行ではなくネットリスト全体で 1 行なので、列の間隔と違ってセル数に比例
  しません。

  **破壊的変更**: `cairn synth` のダンプに現れるセル座標・`wire_length`・`delay_ticks`・
  `buffer_coords` はすべて変わります。入力パッド `i` は `(0, 0, i)`、出力パッド `k` は
  `(width - 1, 0, k)` になります。3 行 — セル行とその両脇の空き行 — に満たない `circuit` 領域は
  配置パスが拒否します。パッド行の拒否は、セル行より後ろの行数ではなく行数そのものを数えます。
  パッドはどのセルも占めない列に立つからです。

- *(redstone)* 各ネットが既に敷かれたダストを避けて配線されるようになり、2 つのネットが配線座標を
  共有しなくなりました。`spec/redstone` §14.5 は脱出を規定しています —「交差は `bridge` タイルか
  垂直層へ逃がす」— が、段階 4 がそれを行うのはバッファのリピータだけで配線には一度も行われず、
  ある座標で出会った 2 つの信号は、両方を運ぶ 1 本のダストとしてコンパイラから出てきていました。
  混雑した回路の問題ではありません。例コーパスの交差はすべて、2 入力のセルという最も単純な形から
  生まれており、領域を広げても 1 つも動きませんでした。

  修正を決めたのは 2 つの計測です。パッドとセル列の規約を変えてもコーパスの交差は 1 つも減らず、
  `crossbar.crn` では倍になりました。そして原因は配線ではなく算術でした。セル本体はブロックなので
  ネットは空いた隣接座標を通って届きます。2 入力ゲートには 3 つの異なるネットが接するので 3 つ必要
  です。そして `x = i` でパッド列に詰めて並べると、鎖の途中のセルには 2 つしかありません —
  領域の大きさによらず、です。行末のセルに至っては 1 つです。配線を bridge 層へ持ち上げても
  解決しません。持ち上げた配線も結局は面を通って到達しなければならないからです。そこで配置パスは
  行を `x = 1 + 2i` に敷き、最後のセルの先にも 1 列空けます。ルータは
  各ネットを先行するネットのダストを避けて配線し、平面に回り道が無ければ `bridge` 層へ登り、
  どこにも道が無ければそのスコープを拒否します。脱出が段階 2 で起きることが、それを計測させます。
  `wire_length` も `delay_ticks` も、この経路木から読み取られます。

  **破壊的**: セル座標が動くので、`cairn synth` のダンプにある `wire_length` / `delay_ticks` /
  `buffer_coords` はすべて変わります。`n` 個のセルに対し `2n + 1` 列 — セル 1 つずつ、その脇に
  1 列ずつ、そして行末の先に 1 列 — を確保できない `circuit` 領域は配置パスが拒否し、行が必要と
  する列数をメッセージが名指します。`W_WIRE_CROSSING` / `E_CROSSING_CONGESTION` /
  `E_BUFFER_COORD_COLLISION` は削除されました — 前 2 つはコンパイラがその欠陥を作らなくなった
  ため、3 つ目はリピータが自分のネットの経路の上に立つようになり、それを奪い合う相手がいなくなった
  ためです。`examples/crossbar.crn` は `size=5x4` から `size=6x4` へ 1 列広がります。

- *(core)* そのロールのメンバが読まない `key=` を拒否するようになりました。メンバの引数はどこでも
  検証されていませんでした。`check` は文キーワードの許可リストを持つ一方、その下のキーには何も
  持たず、`walls ... hieght=3` は無言で exit 0 になり、作者がやっと目にするのは「綴りを間違えた
  引数」ではなく「いま無い引数」を名指しする `W_DEFERRED_MEMBER` だけでした。語彙は
  `MemberRole::arguments` で、ワイルドカード無しで照合するのでロールが増えたら必ず答えることに
  なります。返り値は `Option` です — 未知のキーワードは「空の語彙」ではなく「語彙が無い」ので、
  行全体は `E_UNKNOWN_KEYWORD` のものです。`E_UNKNOWN_ARGUMENT` がエラーなのは 1 段上のキーワード
  コードと同じ理由で、キーが何も指しておらず、コンパイラがどう育っても値を読むパスは現れないから
  です。`compile` は綴り間違いを先に、それが引き起こした deferral を後に出します — 直し方の後に
  その帰結、という順序です。以前は帰結しかありませんでした。`theme` のセレクタは名指しした
  キーワードの語彙だけを広げます。`window[tags=...]` は `tags=` を window でリゾルバのセレクタ
  照合が読むキーにするので、そのキーで選択するモジュールではそれを書くことは間違いではありません。
  表を書き下ろす過程でもう 1 つ静かな穴が出ました — `window shape=` は
  `spec/components-editing-sites` §9.2 にあり、どのパスも読まず、それを使う唯一の例はそれ無しで
  建っていました。これは拒否ではなく `W_IGNORED_ARGUMENT` です (コードの意味がすでに「メンバは
  ビルドに入っており、その引数の 1 つは入っていない」)。キーは作者が書いてよいもので、欠けている
  のは実装の側だからです。**破壊的**: 綴り間違いやその他認識されない引数キーを持つソースは
  現在すべてビルドでき、これからは拒否されます。仕様が定義していてどのパスも読まないキーは
  ここに含まれず、警告のままです。`MemberRole` は `arguments` / `unread_arguments` /
  `accepted_arguments` を得ます。`examples/themed-tower.crn` は、その arrow-slit window が一度も
  使われないまま持っていた `shape=slit` を落とします。

- *(formats,core,cli)* `cairn info` が `unsupported` の数字が数えているパレットエントリを名指しする
  ようになりました。この数字は、直し方が 4 通りある 4 つの失敗をひとつの整数にまとめたものでした —
  そのエディションにブロック自体が無い、ブロックはあってもこのコンパイラがその状態の対応付けをまだ
  持たない、Java のドメイン外の状態値が state translator まで届いた、translator が読まない状態キーが
  届いた、の 4 つです。最後の 1 つだけが作者に直せるもので、エラー自身がすでにそう書いていました。
  ID もエントリも、カウンタを増やすその場所に揃っていて、どちらも捨てられていました。
  `unsupported: 1` を見た読み手は、ソースを手で二分探索するしかありませんでした。カウントは動きません
  — もともと数えていたものに名前が付いただけです。エントリは数字の下に stderr で、カウント 1 につき
  1 件、パレット順で並び、`E_UNKNOWN_ID` と同じ読み方 (単一の名前空間の中で path を比較する) の
  `did you mean` が付きます。ただし候補は固定した 1 バージョンの表ではなく、そのエディションが宣言する
  全 ID から引くので、両者が別のブロックを挙げることがあり、それぞれ自分の問いについては正しい。
  stdout の 4 行は変わりません。あれは JSON のトップレベルの text 版であり、エントリの一覧は行の形を
  していないからです。Bedrock 側はワイルドカードで全失敗を畳むのをやめ、`BedrockStateError` を variant
  ごとに照合するようになりました。4 つ目の variant が増えたとき、`_` が指していたバケツに黙って
  加わるのではなく、ここで分類しなければならなくなります。各 reason は組み上がった文ではなく自分の
  答えのフィールドを持ちます。`valid` と `handled` の一覧も translator 自身の定数から流れてくるので、
  そこにキーや値が増えれば 2 度目の編集なしにこの報告へ届きます。**破壊的**: `portability_for_java` /
  `portability_for_bedrock` は `PortabilityCounts` ではなく `PortabilityReport` (フィールドは private、
  `counts()` / `unsupported()` / `into_unsupported()`) を返します (同じ問いに別の答えを出しうる 2 組目の
  入口を増やすのではなく、エディションごとの入口を 1 つに保つため)。`BedrockStateError::UnmappableBlock`
  と `UnknownStairKey` はそれぞれ、メッセージが挙げる集合を持つフィールドを得ます。
  `EditionPortability` / `EditionReport` はそれぞれ `unsupported_entries` フィールドを得ます。
  `--format json` には `edition_portability[].unsupported_entries` が加わります。未知のキーを無視する
  消費者にとっては追加のみです。

- *(cli)* `cairn info` が、どのサポート対象ターゲットならビルドできるかを報告するようになりました。
  ポータビリティのカウンタは **エディション** に対して問うため (範囲の一部で綴りが違うブロックは
  そのエディションから欠けてはいない)、2 つのパレットエントリが互いに素なバージョン集合で宣言されて
  いても両方が「ある」と答えます。その結果、どのサポート対象ターゲットも拒否するソースに対して
  `unsupported: 0` と表示されていました。新しい `buildable targets` の行がバージョン単位で答えます。
  そのバージョンに固定した lowering がエラーを出さないバージョンを並べ、拒否するバージョンを横に
  挙げます。範囲ではなく集合なのは、2 つの id のバージョン集合が互い違いになると範囲では埋めてしまう
  穴が空くからです。導出はサポートされる各バージョンで 1 回ずつ lowering する方法 —
  `cairn compile --target` が既に行っている検査 — で行い、範囲全体のパレットの id 集合の積は取りません。
  その積は肝心な向きで不健全です。ターゲットを固定しない lowering では各マテリアルが既定の対応付けを
  取るため、`@floor.stone.smooth` (Bedrock 1.21.0 では `stonebrick` に綴り替え) を束縛したテーマの隣に
  リテラルの `@stonebrick` があると積は空になりますが、1.21.0 ではビルドできます。この行は報告する
  だけで拒否はしません。どのバージョンでもビルドできないソースでも `cairn info` は exit 0 のままで、
  これはポータビリティのカウンタが既に従っている規則と同じです。拒否するのはビルドのほうです。
  ただし拒否した各バージョンの findings はそのバージョンの下に出力します。run の中で他に見せる場所が
  無く、理由を言わない `none` は報告ではないからです。あるバージョンが buildable と数えられるのは、
  `cairn compile --target` がソースに対して課すゲートを通ったときです。すなわち、そのバージョンに
  固定した lowering がエラーを出さず、`@requires` の floor がそのバージョン以下で、ソースが宣言した
  スコープがすべて lowering されたときです。`compile` が拒否するターゲットを並べる行は、同じ欠陥を
  別の場所で作ることになります。**破壊的**: `compute_axes` はポータビリティのリストの代わりに、
  要求されたエディションごとに 1 つの `EditionReport` を取るようになりました。これにより 2 つの
  エディション単位の wire 行が、長さ・順序・どのエディションを指すかで食い違えなくなります。
  `VersionAxes` には `buildable_targets` フィールドが増え、`#[non_exhaustive]` になりました
  (次の軸が破壊的変更にならないように)。`--format json` には `buildable_targets` が加わりますが、
  未知のキーを無視する消費者にとっては additive です。

- *(core)* 何も検証しない真理値表を報告するようになりました。`assert truth` は回路を検査するための
  構文ですが、3 つの形の表が —— diff でもレビューでも通る表とまったく同じ見た目のまま —— 何も検査
  していませんでした。行が 1 つも無い表、同じ入力の組を 2 回割り当てる表、入力の組を割り当て残した
  表です。行そのものを厳しくしても、行を取り巻く表については何も言っていませんでした。行が無い表は
  後から周囲に何を書いても何ひとつ主張できないので `E_TRUTH_TABLE_EMPTY`、1 つの入力の組に異なる
  出力を割り当てる 2 行は存在し得ない回路を記述しているので後の行が `E_TRUTH_TABLE_CONFLICT` です。
  どちらもエラーであり、これまで exit 0 だったソースを拒否します。先行する行を繰り返して出力も一致
  する行 (`W_TRUTH_TABLE_DUPLICATE_ROW`) と、入力の組を割り当て残した表 (`W_TRUTH_TABLE_PARTIAL`)
  は警告です。書かれている行はいずれも実際の制約であり、入力 4 本の表は 16 行あるので書き途中の
  作成者を止めるべきではないからです。繰り返しは直前の行ではなく **最初に** そのパターンを持つ行と
  突き合わせます。こうすると 1 つの組についてのすべての findings が同じ行を参照させるので、
  `00->0; 00->1; 00->0` は衝突 1 件と重複 1 件、`00->0; 00->1; 00->1` は衝突 2 件になります。
  繰り返した行は組を埋めないため、1 つの表が重複と不足の両方で報告されることがあります。これは
  修復が 2 つあるということであって、1 つを二度言っているのではありません。食い違う 2 行のどちらが
  評価されるかはどこにも書きません。シミュレータは未実装で、いずれにせよ修復はどちらの行が誤りかを
  決めることだからです。`W_TRUTH_TABLE_PARTIAL` は書くべき組を構造化データとして持ちますが、集合
  そのものではなく先頭数件のサンプルです。入力リストに文法上の上限は無く、入力 20 本なら組は
  100 万通りあるためで、件数は算術で求めるのでその空間を歩くこともありません。コンパイラが扱える
  どの整数にも収まらない幅の表は、総数を `2^n` と表示します。この報告のために `TruthRow` が span を
  持つようになり、finding が一方の行を、その note がもう一方の行を指せるようになりました。

- *(redstone)* 値が信号を指していない信号バインディングを拒否するようになりました。従来
  バインディングは **値** で認識していたため、キーを正しく書いて値を間違えるとどの分岐にも
  入らず、`door ... opened_by=a` は何にも繋がっていないドアのまま placement に到達し、
  `pressure_plate ... -> foo.bar` は診断もスコープも出しませんでした。`-> value` の末尾は値が
  何であれ「センサのつもりだった」と言い、アクチュエータキーは値が何であれ「配線するつもり
  だった」と言う — そう読むようにして、値そのものを検査します。コードは
  `E_LOGIC_INVALID_SIGNAL` で、`logic` 行の左辺がすでに使っているものです。3 つの位置は同じ
  名前空間についての同じ規則だからです。パーサがそこに置きうる値の種類はすべて対象です — 名前の
  打ち間違いとしてありうる 5 つ (`a`、`"sig.a"`、`3`、`@tok`、`foo.bar`) と、打ち間違いではない
  が到達しうる 3 つ (`true`、`2x2`、`[a,b]`)、そして `sig.a.b` — 信号名は 2 セグメントであり、
  block-array パスは以前からそう要求していたのにフロントエンドはしていませんでした。裸の識別子には
  名前空間を補った綴りを提示します (`a` の読み方は 1 通りです)。`sig` には提示しません — これは
  逆に名前空間だけ書いて名前を落とした形だからです。ホストは値より先に問います。`walls -> a` は
  従来どおりホストの誤りだけを報告します。値をどう直しても `walls` が末尾を持てるようには
  ならないためです。`[selector]` は今回はじめて走査し、角括弧の中の組は「外に出しても直らない方の
  誤り」で答えます — `door[id=front,opened_by=sig.x]` なら角括弧そのもの、
  `door[id=front,lit_by=sig.x]` ならホスト、`door[id=front,oepend_by=sig.x]` なら未知のキー
  (`did you mean` 付き)。§14.2 はバインディングを角括弧の後ろに書き、`cairn compile` はドア
  パッチについてはすでにこの形を拒否していました。バインディングでないキーと信号参照でない値の組は
  これまでどおり誰の検出対象でもありません — 未知の引数キーが何を意味するかには、まだ答えが
  ありません。

- *(lsp)* 開いていない URI への `textDocument/didChange` は、開く代わりに無視されます。
  `didClose` 後の変更は従来ドキュメントを挿入し直して診断を publish しており、エディタ側には
  それを消すバッファがもう無い状態でマーカーだけが残っていました。一度も開かれていない URI への
  変更も、その URI で補完が効くようになってしまっていました。サーバ自身がスキーマ不一致で
  `didOpen` を落とした場合も同様で、そのドキュメントはクライアントが開き直すまで未知のままに
  なります (従来はキーストローク 1 つで復活していました)。
- *(lsp)* 文字列リテラルの内側では補完を出しません。`door id="@oa"` はマテリアルカタログ全体を、
  `door label="pick mat_slot=fl"` はテーマのスロット名を返していました。文字列は自由記述であり、
  補完モジュールが元々「候補を捏造しない」と宣言していた位置です。閉じ引用符より後ろのカーソルは
  従来どおり補完されます。
- *(lsp)* ドキュメント末尾より 1 行先の補完位置は、EOF にクランプせず `InvalidParams` で拒否
  します。クランプは各アイテムの `textEdit` を前の行に固定するため、要求された位置を含まない
  range になり、エディタはそれを破棄します — 答えとして元から使えていませんでした。
- *(core)* `window` は壁が実際にある位置にだけ開きます。矩形の各行が `walls` の 1 つの層の内側に
  なければなりません — `level y=N` 配下の `walls height=H` はワールド座標 `N+1 … N+H` 行を塗り、
  接する層どうしは 1 つに併合されます。そのため `window y=0` や、2 つの `level` 層の間の空中に
  書かれた window は `W_DEFERRED_MEMBER` になります。従来は床スラブに穴を開け、空中にガラスを
  吊り下げていました。判定が「壁の最上段」1 つの数値だったため、どちらの誤りも見えませんでした。
- *(core)* トップレベルの `walls` に開ける window について、ポートは openings パスが実際に開ける
  矩形とちょうど同じ集合で walkway を固定します。壁の最上段にぴったり収まる window は、壁には
  開けられるのにアンカーとしては拒否され、逆に地面段の window は開いてもいないカットのアンカーとして
  受理されていました。両パスが同じ述語を呼ぶようになり、`spec/components-editing-sites.md` §9.3.5 も
  コンパイラの実際の規則を述べます。`level` 配下の `walls` はこれまでどおりポート解決からは見えません
  — 2 つのパスが共有するのは述語であって、それを適用する列そのものではありません。
- *(core)* 1 層しかない屋根は自分自身を蓋するのではなく壁に着座します。短辺スパンが 1 か 2 の
  屋根は 1 層しか立ち上がらず、その層を頂部として扱っていたため全体が `half=top` になり、切妻では
  棟の全長に、寄棟では外周全体に半ブロックの隙間が走っていました。寄棟ではさらに 4 隅の `outer_*`
  と辺ごとの facing がすべて失われていました。
- *(core)* 偶数スパンの切妻の 2 段蓋は、ジェネレータ自身のコメントが元々そう述べていたとおり、棟の
  外側を向きます。`half=top` の階段はボクセルの上半分と、facing 側の下 1/4 を埋めるため、内向きの
  対では屋根の全長にわたって外面に 0.5 x 0.5 のえぐれが残っていました。奇数スパンの単段蓋は低勾配側
  の facing を保ち (`spec/compilation.md` §4.3)、変わりません。
- *(redstone)* 1 つの信号が持つドライバは 1 つです。同じ `sig.X` に束ねられた 2 つのセンサは、
  従来は先頭だけが残り 2 つ目が黙って捨てられていました (ブロックはビルドに残ったまま、電気的には
  どこにも繋がらない状態)。これが `E_LOGIC_MULTIPLE_DRIVERS` で拒否されます。それぞれ別名に発して
  `logic sig.a = sig.a1 or sig.a2` で合成してください。
- *(redstone)* `E_LOGIC_MULTIPLE_DRIVERS` は、センサか `logic` 行かにかかわらず、*後に*
  書かれた方を指します。センサより上に書かれた `logic` 行が報告されていましたが、これからは
  下にあるセンサの方が指されます。
- *(redstone)* `-> sig.X` の末尾はセンサ以外で、各アクチュエータキーは `spec/redstone` §14.2 が
  対応づけたコンポーネント以外で拒否されます (`E_LOGIC_MISPLACED_BINDING`)。
  `walls ... powered_by=sig.x` や `window ... -> sig.w` は従来そのままポートになっていました。
  `lit_by=` / `powered_by=` / `fired_by=` は `lamp` / `piston` / `dispenser` がキーワードになるまで
  正当なホストが存在しないため、どこに書かれても拒否されます。
- *(redstone)* `sig.` 名前空間の外にある `logic` の左辺は `E_LOGIC_INVALID_SIGNAL` です。
  `logic foo.bar = ...` は従来、誰も読めない信号のために実際のゲートを下ろし、配置座標を
  占有していました。
- *(redstone)* 値が `sig.` 参照でありながらキーがアクチュエータキーでない引数は
  `E_LOGIC_UNKNOWN_BINDING_KEY` になります。キーがタイプミス閾値の内側なら `did you mean` の
  note が、そうでなければ有効なキーの一覧が付きます。
  `door[id=x] oepend_by=sig.y` は従来アクチュエータを消し、`W_LOGIC_UNUSED_SIGNAL` だけを
  残していました。
- *(redstone)* どのセンサも発さずどの `logic` 行も定義しない信号を名指しする `assert` は
  `E_LOGIC_UNBOUND_SIGNAL` です。スコープの redstone 要素がその `assert` だけの場合も含みます。
  これを評価するシミュレータは未実装のままですが、存在しない名前に対するプロパティはシミュレータを
  待っていたわけではありません。また `assert` は消費側として数えられるようになったため、`assert` が
  観測している信号は `W_LOGIC_UNUSED_SIGNAL` を受けなくなります。
- *(redstone)* 信号が誰にも読まれないセンサは `W_LOGIC_UNUSED_SIGNAL` を受けます。
  `pressure_plate ... -> sig.a` だけのスコープは従来黙って合成されていました — これはビルドに
  残ったまま何にも繋がっていないプレートです。
- *(redstone)* `SynthOutput::diagnostics` が span 順にソートされるようになりました (doc が
  従来から約束していた挙動です)。`cairn synth` は渡された順に出力するため、複数の収集フェーズから
  findings が出るモジュールでは並びが変わります。

- *(redstone)* `PlacementIr::outputs` の型が `Vec<NetlistOutput>` から `Vec<PlacedOutputNode>`
  になりました。アクチュエータは配置される対象です。配置パスが割り当てたパッド座標と、セルと
  同じ `PlacementPhase` を持つため、routing / delay / crossing の各パスが同じ規則で埋めます。
  Rust API、Internal ティア。
- *(redstone)* `cairn synth --stage <placement|route|delay|crossing>` の `outputs[]` の各要素が
  広がりました。従来はどのステージでも `{name, driver}` でしたが、`{stage, name, driver, pad}`
  に加えて各ステージが埋める `wire_length` / `delay_ticks` / `buffer_coords` が付きます。`synth`
  は Evolving で、`--experimental-logic-synth` の背後にあります。
- *(redstone)* `BufferCoord::port` の型が `PortName` から `BufferSegment` になり、セルの入力
  ポートに加えてアクチュエータへ出る配線 (`"out"`) を指せるようになりました。ワイヤ形式は
  1 つのフラットな文字列のままで、`"out"` はこれまで取り得なかった値です。
- *(core)* `pressure_plate` の評価フェーズが openings から fixtures になりました。
  `spec/compilation.md` §4.1 がセンサを置いている場所です。同じセルを奪い合う plate と
  `window` は従来は後に書かれた行が勝ちましたが、これからはどちらの順でも plate が勝ちます。
- *(core)* 構造のパレットには、そのボクセルが指すブロックだけが並びます。最後の 1 ボクセルを
  後続フェーズに覆われた材質は削除され、残りのスロットは詰め直されます。該当するビルドでは
  `.nbt` のパレット、`cairn info` が報告するエントリ単位の行、`resolved_ir_hash` がいずれも
  変わります。
- *(core)* 同一フェーズの 2 つのメンバが 1 つのボクセルを別のブロックに書いた場合、
  `W_PHASE_CONFLICT` を出すようになりました。last-wins 自体は §4.1 の規定どおり変わりませんが、
  新しい警告を失敗として扱う消費者には、これまで出なかったものが出ます。
- *(redstone)* `logic` の番号付けと報告がネストをまたいでソース順になりました。`level` の中の
  binding が、その上に書かれたトップレベルの binding より小さいノード番号を取ることはなくなり
  ます。これに伴い `E_LOGIC_MULTIPLE_DRIVERS` が指す行も入れ替わります。
- *(redstone)* `cairn synth` はモジュールのスコープを「全 `struct` → 全 `def` → 全 `site`」では
  なくソース順に走査します。3 種類を混在させたモジュールでは、dump の `scopes[]` の並びも診断の
  並びも変わります。
- *(cli)* `cairn lower` / `cairn info` / `cairn compile` は、note がソース中の別の位置を指す場合に
  その note 自身の `file:line:col:` を前置するようになりました。`cairn check` と `cairn synth` は
  既にそうしていました。行頭の `  note:` で note を拾っていたスクレイパは、それらの行を取り逃がします。
- *(redstone)* `cairn_lang_redstone::DiagnosticNote` は、同じ 2 フィールドを別途宣言するのを
  やめて `cairn_lang_core::check::DiagnosticNote` の re-export になりました。パスを名前で
  参照しているだけのコードは互換です。redstone 側の型に `impl` を書いていた場合は core 側の型に
  書くことになり、両方に書いていた場合は 1 つの型に二重に書くことになります。Rust API、
  Internal ティア。
- *(cli)* `cairn check --format text` は診断を stderr に書きます。ビルド系コマンドの中でこれだけが
  stdout に出しており、`cairn check f.crn > out` はすべての指摘を飲み込んで終了コードだけを残して
  いました。`--format json` は stdout のままです — こちらは成果物であり、意図してリダイレクトされる
  ものだからです。なお**パース失敗**は従来どおり両フォーマットとも stderr に出て stdout は空のまま
  で、このリリースでは変更していません。
- *(core)* スキーマが宣言していないキーを持つロックファイルは拒否します。トップレベルだけでなく
  どの階層でもです。必須フィールドの横に `attacker_controlled: yes` を並べた文書が、従来は
  `verified: true` のまま `Ok` としてデシリアライズされていました。ロックファイルは「何がビルド
  されたか」の主張であり、読み手が黙って無視するキーを運べる文書は、意味が読み手次第になります。
- *(core)* `Lockfile` は先頭フィールドとして `lock_schema_version` を宣言します。ビルドが理解する
  バージョンより上を宣言する文書は、「フィールド名が同じままである」と仮定して読むのではなく名指しで
  拒否します。このキーを持たない文書はバージョン 1 — フィールドが存在する前の形 — として読むので、
  このコンパイラがこれまでに書いたロックファイルはすべて読めます。`spec` 10.6 のサンプルは両言語で
  このフィールドを持ち、そこで固定していたフィールド順は意図的に変更しました。
- *(nbt)* アイテムを持たないのに `TAG_End` 以外の要素型を宣言する `List` の書き出しを拒否します。
  `List` のフィールドは public なのでコンストラクタは便宜であって関門ではなく、対になる異種要素
  チェックは「空リストが一度も入らないアイテムループ」の内側にあります — ライタが全バイトの通る
  唯一の地点です。`bedrock_structure` の `block_indices` は構造体リテラルでまさにその形を作って
  いました。
- *(formats)* `pack_hash` は対象の各フィールドに長さプレフィックスを付けます。これにより全パックの
  `inputs.registry_pack_hash` が変わります。従来の単純連結ではマニフェストが最初のコンポーネント名に
  そのまま繋がり、各コンポーネントの本体も次の名前にそのまま繋がっていたため、同じバイト列を 1 バイト
  左右にずらして分けても同じダイジェストになりました — 関数の doc が「ダイジェストが防ぐ」と主張して
  いた、まさにそのリネームです。最初の名前の前に区切りを 1 つ置くだけでは半分しか塞げません。衝突は
  2 つともテストで固定しています。
- *(redstone)* 配線パスは、邪魔になる部品を貫くのではなく迂回してネットのダストを引きます。従来は
  `{source} ∪ sinks` の全域木を張って各辺を L 字で描いており、確保領域にすでに何が立っているかを
  まったく見ていませんでした — 木は遠いセルへ近いセルを **貫いて** 到達し (コンパレータが渡すのは
  自分の出力であって、それを駆動した配線ではありません)、L 字は両端の間にあるもの (感圧板を含む) を
  無視して横切っていました。いまや各シンクは葉であり、fanout は列の脇を走る幹と各セルへの分岐に
  なり、ネットの配線座標にそのネット自身の端点以外の部品が現れることはありません。
  結果は 3 つ、いずれも observable です:
  - 旧配線が何かを貫いていた箇所では `wire_length`・`delay_ticks`・バッファ repeater の座標が
    すべて動きます。`examples/redstone-door.crn` は `3` だった `wire_length` が `5` になります。
    2 つ目のセンサーのパッドが 1 つ目の後ろにあるため、そのダストは貫通ではなく迂回になりました。
  - 共有バスが、本来必要のなかった逃がし層を要求しなくなります。1 つのセンサーを 16 セルが読む形は
    従来セル #13 の本体上にリピータを要求してブリッジ層へ持ち上げられていましたが、幹が列の脇を
    走るようになったため、リピータ 1 台が空きの配線上 `(14,0,1)` に立ちます。
  - 先頭セルが 2 つ目のセンサーを読む 2 セル以上の `void=1` スコープは `E_ROUTE_CONGESTION` で
    拒否されます。セル #0 は片側にセル #1、もう片側にセンサーパッドの列がある隅にあり、サービス層が
    1 枚では 2 つ目の信号の入り口がありません。従来は感圧板を貫く配線のままビルドできていました。
    拒否は結べなかった 2 座標を名指しし、`void=` を指します。
  - `E_BUFFER_COORD_COLLISION` の原因は 3 つから 1 つになりました。バッファ repeater は経路の両端の
    内側に立ち、内側の座標をルータがブロックから外して引くので、候補がセル本体や I/O パッドになる
    ことはもうありません。残るのは「他ネットのダストがその座標を占め、その列のブリッジ層がすべて
    埋まっている」場合だけで、層を埋めるのは以前のリピータだけでなく配線でもありえます。

### 追加

- *(redstone)* `W_WIRE_CROSSING` は 1 つのダスト座標を共有する 2 つの信号を名指しします。交差の
  正規化パスはその座標をすでに計算していました — `circuit region=` の確保が 1 層しかなければ
  スコープを拒否し、それより高ければ「確保したサービス層が後段のパスに持ち上げ先を残している」
  という理由で捨てていました。持ち上げるパスは存在せず、しかも routing パスがブロックを避けるため
  にその層へ登るようになったので、層が空いているわけでもありません。確保の高さは 2 つの信号が
  混ざるかどうかを決めないため、何かを言うかどうかも決めなくなりました。層が 1 つも無い場合だけは
  `void=` を上げることが唯一答えを変えうる手段なので拒否のままとし、それ以外の交差はすべて信号の
  組と出会う座標を添えて報告します。共有座標ごとではなく組ごとに 1 件なので、並走する 2 本のネット
  は 1 件です。redstone の例は 2 つとも該当し、`redstone-door.crn` が 1 件、`crossbar.crn` が
  2 件です。後者の 2 件目は bridge 座標が起点で、ブロックを避けて登った 2 本のネットが登った先で
  出会っています — 交差は平面だけの事象ではないので、名前も平面を指しません。どちらも終了コードは
  従来どおり 0 で、報告であって修復ではないため座標は 1 つも動きません。見えるのは
  `cairn synth --stage crossing --experimental-logic-synth` からだけです — 交差パスには他の
  呼び出し元が無いため、stage 4 の診断は `cairn compile` にも `cairn check` にも届きません。

- *(cli)* `cairn compile` は置き換える前のロックファイルを読み、何が変わったかを `spec` 10.6 の
  記述どおりに報告します。別ターゲットでの再コンパイルは `W_PREVIOUSLY_VERIFIED_TARGET` を出し、
  エディションが動いたときだけエディション名を添えます (エディションごとにリリース番号の体系が違う
  ため、バージョンの対だけでは意味を成しません)。置き換えられるロックファイルが
  `member_version_sensitivity` を記録していれば `W_SEMANTIC_SENSITIVITY` がその id を挙げます。
  これは `spec` 10.6 の導入どおりターゲット変更に従属する行で、ターゲットが変わっていない
  再コンパイルでは、ロックファイルにエントリがあっても何も出しません。捏造は一切しないので、
  制約カタログの取り込みが入るまでこの行は黙ったままです。新しい Cairn が書いたロックファイルは
  「壊れている」ではなく専用の文言で報告します — 置き換えは情報の喪失であり、かつ壊れてはいない
  ためです。パースできない
  ロックファイルや、このビルドが読めないスキーマを宣言するロックファイルは、黙って捨てるのではなく
  報告してから置き換えます。いずれも警告であり終了コードは変わりません。読み戻しは明示的な
  `--lock` だけでなく既定の `<source>.lock` にも効きます。

### 修正

- *(tree-sitter)* 行末に空白があってもよくなりました。改行の前の空白と、空白だけの空行を文法が
  拒否していました — `cairn-lang-core` は受け付ける 3 つの形であり、保存時トリムのないエディタが
  そのまま書いてしまう形なので、コンパイルは通る `.crn` がハイライトだけ落ちることがありました。

  external scanner は tree-sitter が `/ +/` の extra を読み飛ばす前に呼ばれるため、空白の連なりは
  どこにあってもそのまま scanner に見えます。行頭ではそれが行のインデントであり、インデント判定は
  その長さを必要とします。行頭以外では区切りの空白にすぎません。両者は「行がどこで始まったか」で
  区別でき、それは scanner が既に記録しています — そこで連なりを一度だけ読み、**数えて**持ち回る
  ようにしました。あとから桁位置の引き算で測り直すのをやめたことで、連なりを改行処理の手前で
  消費できるようになり、改行処理の前に立ちはだかることがなくなります。

  これまで拒否できていなかった 3 つも拒否するようになりました。直前の改行が代弁できない行に置かれた、
  奇数インデント・2 段以上の飛び越し・タブです。宣言ヘッダの改行の後に来られるのはインデントだけなので、
  その次の空行は改行として消費されず scanner 自身が読み飛ばします。読み飛ばした先の行を、その場で
  測るようになりました。

- *(core)* struct が立体化される体積は、実際に塗るメンバのぶんだけ確保されるようになりました。
  メンバが配列の形を決めておきながらブロックを 1 つも置かない経路が 3 つありました。描かれない
  `roof` は `overhang=` を footprint には渡し、高さには渡していませんでした — 2 つある roof の
  走査のうち片方は「この roof は描かれるか」を問い、もう片方は「`kind=` が表にある名前か」しか
  見ていなかったためです。`5x5` の struct に `overhang=3` を書くと `11x4x11` の配列が出力され、
  壁は内側に寄り、そのまわりを空気の輪が囲み、屋根のボクセルは 1 つもありませんでした。`kind=`
  が無い場合と、`slope_to=` の無い `kind=shed` の両方が該当します。`mat_slot=` が解決しない `walls` は高さのぶん
  `Dims.y` を上げていたので、テーマが束縛されていない struct は palette が空気だけの `3x7x3` の
  配列を出力していました。壁リストの 2 つの走査は一緒に動きます — `spec/compilation.md` §4.7 は
  体積と window の切り抜きを「1 つのリストの 2 通りの読み」と定め、その一致こそが「渡された配列
  の外にメンバが描き込まない」ことを保証しているからです。§4.7 の「一般化されない」という段落は、
  それを書かせた反例とともに消え、各項ごとの規則に置き換わりました。

  何も塗らない壁に開けられた `window` / `door` は、空気に穴を開ける代わりに報告されるように
  なりました。`roof` はそれでも描かれ (壁と違って材質にフォールバックがあるため)、存在しない
  壁の上ではなく地面の高さに座ります。

  `Dims` は成果物に届きます — `.nbt` は各軸そのブロック数であり、lockfile にもその数値が記録され、
  `place` の歩道の原点もそこから導出されるので、サイト座標も範囲と一緒に動きます。警告付きで
  ビルドできていたソースは、コンパイルエラーを伴わずに以前より小さい構造物を出力するように
  なります。範囲は `>=2026.8.2, <2027.0.0` (Cargo は CalVer の `2026` をメジャーと読みます)。
  `synth` と違い、`lower` / `compile` の前に experimental のゲートはありません。

  該当する形は 2 つあり、何かを言うのはそのうち片方だけです。テーマ未束縛の struct には
  `W_NO_THEME_BOUND` がありますが、`mat_slot=` をまったく書いていない `walls` はどのパスからも
  報告されないため、そのソースは無言でサイズが変わります (この診断の欠落は別途追跡しています)。
  変わるのは範囲だけではなく形状もです — 壁が空気にフォールバックするのに対し roof は自前の材質に
  フォールバックするので、テーマ未束縛の struct に `walls height=3` と `roof kind=gable` を書くと、
  棟は「6 段の箱の上」から「基礎スラブの上」へ移り、2 つの出力の間にそれを告げるものはありません。

  それでも `修正` に置いているのは、消える範囲が元から誰も塗っていない範囲だからです — 旧来の
  配列は自分より小さい建物を収めていて、その差について何も言いませんでした。

- *(core)* `W_IGNORED_ARGUMENT` — 立体化パスが読めなかった `key=` を、それでも先へ進んだメンバに
  対して報告します。`roof kind=gable overhang=nope` は `W_DEFERRED_MEMBER` を出していましたが、
  これは「そのメンバは立体化されなかった」という意味です。実際には屋根はビルドに入っており、
  `overhang=` を書かなかった場合とまったく同じく壁面と面一で描かれていました。新しいコードは
  「値は無視された」と述べ、note がその結果を述べます — 屋根が壁面と面一で描かれたのか、それとも
  その屋根は別の理由でどのみち描かれないのか。severity は置き換え前と同じなので
  exit code は動きません。`spec/lint.md` §11.3 の字義では「値が落とされた」はエラーですが、
  昇格は未知の引数キーが待っているのと同じ判断です。

- *(redstone)* 1 本のダストには 1 台のリピータ、そしてその分だけの遅延。Steiner ツリーはシンク
  どうしがプレフィックスを共有するため、1 つのネットの 2 つのセグメントは同じバッファリピータ
  候補を計算します — 同じ信号を読む 1 セルの 2 ポート、同じ 15 ブロック地点にぶら下がる 2 セル、
  セルとアクチュエータ。そこに既に立っているリピータを認識していたのはアクチュエータ側の
  セグメントだけでした。セル側のセグメントは `void=<N>` のブリッジ層へ迂回し — 1 本しかない
  ダストの上に 2 つ目のブロックを置き — さらにブリッジへ*持ち上げられた*候補は誰からも認識
  されないため、同じネットの次のセグメントが同じ地点の上にもう 1 つ持ち上げ、2 つ合わせて予約
  高さを使い切っていました。16 セルの共有バスは `void=2` で `E_BUFFER_COORD_COLLISION` により
  拒否されていました — 必要のない層を要求したためです。今は legalize され、15 ブロック地点より
  先のすべてのセルが同じ 1 台のリピータを指します。

  セルへ入る配線を記述する 3 つのパスも、同じ信号を読む 2 ポートが 1 本のダストであることで
  一致し、どちらの数値もドライブしているネットを 1 回ずつ数えます。`logic sig.s0 = sig.a and
  sig.a` は、パッドとセルの間にある 1 ブロックのダストに対して `wire_length: 2` と報告して
  いました。リピータが必要な長さのセグメントでは `delay_ticks` がそのリピータをポートごとに
  課金しており、この 2 ポートのセルなら、1 台しか立っていないブロックの 2 倍だけ遅れる
  ことになっていました。
  `BufferCoord` の doc は、その `{port, coord}` という形が元から持っていた読みを明文化しました
  — このベクタは attribution のリストであり、「セグメント × そのセグメントが通過するリピータ」
  1 つにつき 1 エントリです。したがって 1 つのブロックが複数のセグメントを担うとき coord は
  繰り返され、ブロック数を数える利用者は coord で重複を除きます。

  これらの数値は出力されており、それを読んでいる利用者には何の合図もなく値が変わります。
  `cairn synth --stage route|delay|crossing` は `wire_length` / `delay_ticks` / `buffer_coords`
  を報告し、拒否されていたソースは exit 0 になります — Cargo は CalVer の `2026` をメジャーと
  読むため、月次のバンプで `>=2026.8.2, <2027.0.0` の範囲内に流れます。それでも `修正` に置いて
  いる理由は 2 つあります。1 つは、旧来の数値が誰にも作れない配置を記述していたこと — 1 本の
  ダストの上に 2 つのブロックが立ち、信号はそのそれぞれを 2 回以上通ったことにされていました。
  もう 1 つは、`cairn synth` が `--experimental-logic-synth` なしでは実行を拒否すること —
  この出力の形が安定互換ティアの外にあることを示すためのフラグです。

- *(core)* 属性値がリストのテーマセレクタが、そのリストを持つメンバを選択するようになりました。
  `ast::Value` は kind に加えてソーススパンも比較しており、`ValueKind::List` は `Value` を保持する
  ため、derive された等価性がその比較を再帰的に通っていました。結果として、2 行に同じつづりで
  書かれたリストはどの深さでも等しくなりませんでした。リスト値のセレクタ属性はどのメンバにも
  一致せず、作者が目にしたのは `E_THEME_SELECTOR_UNMATCHED` — 「絞り込みが狭すぎる」と読めて
  しまい、「この属性型は原理的に一致しない」とは読めませんでした。同じ比較を使う
  `E_DUPLICATE_SELECTOR` も、バイト一致する 2 つのリスト値の行を重複として認識できていません
  でしたが、これも直っています。`Value` の等価性は kind の等価性そのものになりました —
  `#[serde(transparent)]` が元から「この値は kind である」と宣言していたとおりです。`Value` を
  包む型は自分の span を持っていてそれを比較するので、2 つの span が一致する限り等価性は
  変わりません (`lower` が作るものはすべて一致します)。

  これは公開 API の挙動変更であり、コンパイルエラーを伴わずに利用者へ届きます。`ast::Value` は
  public で、ワークスペースのバージョンは CalVer なので Cargo は `2026` をメジャーと読み、月次の
  バンプは `>=2026.8.2, <2027.0.0` の範囲内で流れます。下流で AST を `assert_eq!` で比較したり
  `Vec<Value>` に `dedup` / `contains` を使ったりしているコードは、何の合図もなく新しい答えを
  受け取ります。それでも `修正` に置いているのは、旧来の答えの方が欠陥だったからです — 別の行に
  同じつづりで書かれたリストと等しくならないリストは、文書化されたどの契約にも合致せず、
  それに依存することはバグに依存することです。
- *(lsp)* `shutdown` と `exit` の間に届いたメッセージでサーバが落ちなくなりました。従来は `exit`
  以外のすべてがプロトコルエラーになり、その後ろにある `exit` を読む前にプロセスがコード 1 で
  終了していたため、エディタは言語サーバがクラッシュしたと判断して再起動していました
  — `$/cancelRequest` はいつでも届きますし、ウィンドウを閉じる際には `didClose` が飛びます。
  `shutdown` 後のリクエストは `InvalidRequest` で応答し、通知は無視し、`exit` はコード 0 で
  終了します (`shutdown` を伴わない `exit` は従来どおり非ゼロ)。
- *(docs)* `cairn-lang-wasm` の README は `wasm-pack build` が動くと書いていました。クレートには
  `wasm-bindgen` 依存がなく `cairn_version` にエクスポート属性も付いていないため、wasm-pack は
  このクレートを拒否し、素の `wasm32-unknown-unknown` ビルドは呼び出せるエクスポートを持たない
  モジュールを出します。README・クレート doc・関数の doc 行をその内容に直しました。
- *(cli, lsp)* `cairn --version`・`cairn-lsp --version`・ロックファイルの `cairn_version` が、
  実際にビルドしたリリースを報告するようになりました。この番号は `cairn-lang-core` の手書き定数で
  ワークスペースを追わなくなっており、`2026.8.2` のビルドが `2026.7` — リリースされたことのない
  番号 — と答え、しかもそれを「ロックファイルが信頼に足る情報を残すために存在する」まさにその
  フィールドに書き込んでいました。クレート自身のパッケージバージョンから読むようにし、テストは
  呼び出し側クレートに cargo が与えたバージョンと比較します — 定数がワークスペースを追わなく
  なれば、クレート境界の反対側から落ちます。この形で見えないものが 2 つあります: 実際に
  リリースされたタグから `[workspace.package] version` 自体がずれる場合 (比較の両辺が一緒に
  動くため) と、定数が「今日の番号と同じリテラル」に戻される場合 (次のバンプまで差が出ません)。
- *(tree-sitter)* npm パッケージが `tree-sitter.json` を同梱するようになりました。`package.json` の
  `files` 配列から漏れており、tree-sitter CLI 0.24 以降このファイルは `file-types` とクエリパスを
  宣言する唯一の場所なので、レジストリから `tree-sitter-cairn` を入れたエディタは `.crn` に対して
  言語をまったく解決できませんでした。`tree-sitter parse` は `src/grammar.json` で動き続けるため
  気付かれず、リポジトリの内側で走る検査では原理的に検出できません (公開されるか否かに関わらず
  ファイルはディスク上にあるため)。CI が npm の tarball を作り、チェックアウトの外で展開し、そこ経由で
  `.crn` をハイライトするようになりました。
- *(vscode)* 拡張機能のマニフェストがワークスペースのバージョンを宣言します。拡張機能自身の
  変更履歴には「拡張機能のバージョンは CLI の CalVer タグに追従する」と書かれていましたが、
  リリースパイプラインは tree-sitter のマニフェストしか揃えておらず、拡張機能は `2026.7.2` の
  まま 2 リリース分取り残されていました。整合ステップが拡張機能も書き換えるようにし、
  `release-patch` はどれか 1 つでも `[workspace.package]` と食い違う間は publish を拒否し、
  通常の pull request では CI ジョブがそれらを突き合わせます。`[workspace.dependencies]` の
  内部クレート要求も同種の写しで、release-plz が番号を決める前の推測値のまま放置されて
  いました。これも揃え、検査対象に含めています。
- *(ci)* VS Code 拡張機能とドキュメントサイトをビルドするようになりました。どちらもワークフロー
  から一度も参照されておらず、拡張機能の TypeScript エラーやレンダリングに失敗する Starlight
  ページが、どのチェックも赤にせずに統合ブランチへ到達しうる状態でした。
- *(core)* 書き出したロックファイルは改行で終わります。`serde_yml` は空のフローシーケンスを改行なしで
  閉じ、`member_version_sensitivity` は最後のフィールドなので、sensitivity エントリを持たない
  ソース — 現時点ではすべて — のロックファイルは `]` で行の途中のまま終わっていました。変更のたびに
  `git diff` が `\ No newline at end of file` を出し、1 バイト追記するだけで文書が壊れます。
- *(nbt)* `List::of_ints([])` と `List::of_compounds(vec![])` は `TAG_End` を宣言します。要素 id は
  ワイヤ上のアイテムを記述するものであり、空のリストにアイテムはありません。`tag.rs` にもそう書いて
  あり `List::empty` は `0` を書いていたので、どちらのコンストラクタを呼んだかで「空のリストが自分に
  ついて何を主張するか」が変わっていました。
- *(formats)* 両方の structure バックエンドが、パレットのスロットではないパレットインデックスを持つ
  ボクセルを拒否します。従来は `i32` として書き出され、読み手が存在しないスロットを補うしかない
  ファイルになっていました。CLI 経由では到達しません (`Palette::intern` が唯一のインデックス発生源
  です) が、`BlockArray` のフィールドも `build_*_tag` も public です。
- *(cli)* `compile --help` から「Bedrock バックエンドはステートレスパレットのみを出し、ステートを
  持つエントリはハードエラー」という記述を削除しました。同じヘルプの `--edition bedrock` の説明が
  既に逆のことを書いており、コードもそちらです: `examples/roof-hip.crn` を Bedrock でコンパイルすると
  終了コード 0 で `W_INTENT_DEGRADED` を出し、`weirdo_direction` と `upside_down_bit` を書きます。
  `lower --help` と `info --help` には、両者が元から通っていた「`Error` 重大度の診断で終了コード 1」の
  経路を明記しました。
- *(docs)* `cairn-lang-formats` の README は存在したことのない
  `BedrockStructureError::StatefulPaletteEntry` を載せ、ステートレスパレットの主張を 3 箇所で
  繰り返していました。このクレートは公開されているため、利用者が受け取ることのないバリアントに対する
  match アームを書けてしまいます。

### 追加

- `PlacementPhase` の 3 つの遷移に、失敗を値で返すミラーを追加した:
  `try_route` / `try_delay` / `try_legalize` で、いずれも
  `Result<(), PlacementPhaseTransitionError>` を返す。順序違反で
  panic するのはパイプラインの各パスにとっては正しい形である —
  フレッシュなコンパイルにおける順序違反は必ず呼び出し側のバグで、
  復帰経路が存在しない。しかし復帰経路を持つ消費者にとってはそうでは
  なくなる: 古いキャッシュ項目を「作り直す」判断に変換する検証器、
  不正な dump を診断で拒否すべき IR 取り込み、呼び出し 1 回のミスで
  長命プロセスを落とせない language server である。パイプラインは
  引き続き panic 版を呼ぶが、その panic 版は `try_*` ミラー +
  panic という形になったので、どの遷移が合法かは 1 か所でしか
  述べられず、2 つの形が食い違うことはない。エラーは variant 名だけ
  でなく違反時の phase 全体を運ぶので、消費者はその cell が実際に
  どこまで進んでいたかを見られる。同時にそれが、エラーの `Display`
  が panic 文言をバイト単位で再現できる理由でもある — ハードコード
  した写しではなく実際の panic payload と突き合わせるテストで固定
  してある。`PlacementPhaseTransitionError::with_context` は
  呼び出し側の cell 識別子を `route_at` などと同じ位置に差し込むので、
  同じ cell についての取り込み診断とパイプライン panic が同じ読み口に
  なる。拒否された遷移は phase を一切変更しない — panic する呼び出し側
  と違い、復帰する呼び出し側はその後もその phase を使うからである。
  `PlacementPhase` と新しいエラー型は crate ルートから re-export した。

### 変更

- `PlacementPhase` の 3 つの遷移が同じ cardinality を名乗るように
  した。`route` / `delay` が "must run once per …" だった一方で
  `legalize` だけが "must run **at most** once per delayed IR" で、
  これは phase enum 導入以前に crossing pass が持っていた
  release-loud な `assert!` の文言を引き継いだものだった。
  3 つとも "must run **exactly** once per …" に統一した。
  不揃い以前に "at most once" は guard の実際の検査内容を
  過小評価していた — `legalize` は `Delayed` に到達していない
  phase も拒否するので、段階飛ばしも再実行と同じくここで落ちる。
  それを伝えられるのは "exactly once" だけである。`route` /
  `delay` の doc コメントは元々 "exactly once" と書いていたので、
  panic 文面が自分の記述する契約と一致したことにもなる。
  cardinality 節は 3 か所の呼び出し側から `TRANSITION_CARDINALITY`
  という単一の const に移し、すべての遷移メッセージが違反した pass 名と
  その pass が消費する phase の間に差し込む — この 3 つの隣に追加
  される遷移は 2 つの名詞を選べるが guard の強さは選べない。
  文言が drift したのはまさにそれが可能だったからである。

- JSON dump の各 `PlacedCellNode` に、最後にその cell へ書き込んだ
  place-and-route pass を名指しする `"stage"` キーを先頭フィールド
  として追加した。値は `placement` / `route` / `delay` / `crossing`
  で、`cairn synth --stage <s>` が受け付ける語彙と同一 — dump 自身が
  それを生成したフラグ名を持つ。従来はどの optional キーが出ているか
  から stage を推論するしかなく、その推論は全域ではなかった:
  `PlacementPhase::Delayed` の cell と、crossing pass が buffer を
  0 個しか materialize しなかった `Legalized` の cell は、空の
  `buffer_coords` が serde-skip されるためまったく同じキー集合に
  シリアライズされる。つまり JSON を読む側は stage 3 の dump と
  「legalize すべきものが無かった stage 4 の dump」を区別できなかった。
  タグの導入でこれを解決しつつ、空 Vec を sentinel に昇格させることは
  していない — `buffer_coords` は空なら従来通り省略される。
  以下のエントリが述べる「stage N の dump は stage N+1 の dump の
  純粋な部分集合」という契約は、これに伴い「`stage` タグを除いて
  部分集合」へと緩和される: タグは stage ごとに新しく*現れる*のでは
  なく*値が変わる*唯一のフィールドである。タグは保存されず毎回の
  シリアライズ時に phase から導出されるため、名指しする variant と
  乖離しえない。
- `PlacementPhase::Legalized::buffer_coords` の型を
  `Vec<CellCoord>` から `Vec<BufferCoord>` に拡張した。新しい
  `BufferCoord { port: PortName, coord: CellCoord }` は crossing
  pass が materialize した各 implicit buffer repeater について、
  その buffer がどの cell driver port の segment に乗ったのかを
  座標と一緒に保持する。crossing pass は buffer 座標を選ぶ際に
  既に `cell.drivers` を走査していたが、出口で port 情報を捨てて
  いた — 下流の block-array voxel lowering は
  `drivers[i].net → source coord → floor((s - 1) /
  DUST_ATTENUATION_LIMIT)` を再計算しないと port を復元できなかった。
  attribution を座標と並べて持たせることで、lowering 側が
  driver 単位で buffer を直接グルーピングできるようになる。
  非空エントリの JSON wire 形式は
  `{"x":..,"y":..,"z":..[,"layer":..]}` から
  `{"port":"a","coord":{"x":..,"y":..,"z":..[,"layer":..]}}` に
  変わり、netlist 側の `CellPortDriver` が既に採っている
  `{port, ...}` shape と揃えた。空の `buffer_coords` は従来通り
  serde-skip されるので、delay pass が 0 buffer と数えたスコープは
  上記 `stage` タグを除けば delayed IR と byte 等価のまま。
  `PlacedCellNode::buffer_coords()` /
  `PlacementPhase::buffer_coords()` は `&[BufferCoord]` を返し、
  `PlacementPhase::legalize` は `Vec<BufferCoord>` を受け取る。
- 下記 M6-PR5 / M6-PR6 / M6-PR7 が `PlacedCellNode` に追加した 3 つの
  進化フィールド (`wire_length` / `delay_ticks` / `buffer_coords`)
  を単一の `phase: PlacementPhase` enum に集約した。`Unrouted` /
  `Routed` / `Delayed` / `Legalized` の 4 variants が
  place-and-route パイプラインの最初 4 ステージに一対一で対応し、
  「`delay_ticks` は付いているが `wire_length` が無い」「delay 前
  なのに `buffer_coords` に値がある」といった不正状態を型で表現
  不能にした。ステージ間の遷移は `PlacementPhase::route` / `delay`
  / `legalize` で表現し、各メソッドは現在の variant を pattern
  match して不正な呼び出し順で release-loud panic を起こす — これ
  まで 3 pass に散在していた `debug_assert!` / release-`assert!`
  のガードを、統一された release-loud 契約に置き換える。
  `PlacementPhase` は `#[non_exhaustive]` なので将来の Stage 5
  (`EditionLegalized`) 追加は accessor が既に隠している以外の
  downstream `match` サイトに対して additive。`PlacedCellNode` の
  `phase` フィールド自体は `pub(crate)` で、下流コンシューマからは
  旧フィールドと等価な `Option<u32>` / `&[CellCoord]` を返すフラット
  アクセッサ (`wire_length()` / `delay_ticks()` / `buffer_coords()`)
  経由のみ見える。手書き `Serialize` 実装が phase を
  `{stage, cell, drivers, coord[, wire_length][, delay_ticks][, buffer_coords]}`
  にフラット化するため、値の綴りは tagged enum object にならず以前の
  リビジョンのフラット形のまま — wire 形式への追加は上記の `stage`
  キーのみ。

### 追加

- `PlacementStage` — 上記「変更」の `"stage"` キーを支える
  `PlacementPhase` の 4 variant への射影。他の Placement IR 型と
  同様 `cairn-lang-redstone` のルートから export する。
  `PlacementPhase::stage()` / `PlacedCellNode::stage()` が返し、
  `PlacementStage::as_str` が wire 上の綴り (`placement` / `route` /
  `delay` / `crossing`) を 1 か所に固定する — layer 語彙に対して
  `RouteLayer::as_str` が果たしているのと同じ役割。`cairn synth` が
  `--edition` 欠落を拒否する際に出す `--stage <name>` 断片も、
  Placement 系 4 stage についてはリテラルの再掲をやめてこの accessor
  から導出するようになった。連鎖の 3 つ目の綴り — clap が
  `SynthStage` の variant 識別子から導出する、どの型とも結びつかない
  もの — は unit test が `ValueEnum` から読み戻して照合するので、
  variant rename で「受け付けるフラグ」と「出力されるタグ」が黙って
  ずれることはない。隣接する 3 つの
  値アクセッサと違い `stage()` は全域で、buffer 座標を 1 つも持たない
  `Legalized` を含めどの phase もちょうど 1 つの stage に属する。
  `stage()` と `Serialize` 実装はいずれも `_ =>` の catch-all では
  なく全 variant を明示列挙しているので、Stage 5 の variant 追加は
  「stage 5 の dump が黙って `crossing` と誤ラベルされる」ではなく
  それを名指しすべき 2 か所でのコンパイルエラーになる。
  `PlacementStage` は `PlacementPhase` と同じ理由で
  `#[non_exhaustive]`。
- `PlacementPhase::route_at` / `delay_at` / `legalize_at` — 3 つの
  phase 遷移メソッドに context を載せられる双子を追加し、順序違反の
  遷移 panic がそれを踏んだ cell を名指しするようにした。既存メソッド
  にも `#[track_caller]` は付いており backtrace には呼び出し側の
  `.rs:line` が出るが、**どの** cell が既に routed / delayed /
  legalized だったのかは分からず、オペレータは backtrace から IR を
  辿り直す必要があった。`_at` 形は任意の `Display` を受け取り、
  違反した phase と invariant 節の間に差し込む。これにより routing /
  delay / crossing の各 pass は例えば `PlacementPhase::legalize
  called on Legalized { .. } for cell #0 at (16,0,1) in struct
  `twice` — crossing legalization must run exactly once per delayed
  IR` のように失敗する。パンくずは pass 診断が既に使っている語彙
  （`cell #{index}` / `({x},{y},{z})` / ``{kind} `{name}` ``）で
  記述され、`PlacementIr::cells` 内での位置・placement
  coord・所属 scope の 3 点から組み立てる — `PlacedCellNode` は
  source-level name を持たないため、この 3 点が cell の唯一の安定した
  識別子である。coord の `layer` は `RouteLayer::Plane` でないときだけ
  描画する — cell coord では起こり得ない（placement pass が `Plane` を
  刻み、以降どの pass も cell body を動かさない）ので通常の描画は短い
  まま、かつ invariant を破った hand-built IR が plane coord に見える
  座標を出力することもない。context 無しの `route` /
  `delay` / `legalize` は、対応する `_at` 形と identity 節の有無だけが
  異なる — context が無い場合は空の節を描画するのではなく ` for …`
  節ごと落とすため、余分な区切りが混入しない。
- Redstone crossing legalization と `cairn synth --stage crossing
  --edition <java|bedrock>`（M6-PR7）— M6 redstone-simulates
  パイプラインの 7 枚目。`cairn-lang-redstone` に
  `compile_crossing(&ScopedPlacementIr) -> CrossingOutput`
  エントリポイントを追加し、M6-PR6 の delayed Placement IR を走査して
  routing / delay pass と同じ `NetRef → source coord` マッピングで各
  net の Manhattan Steiner tree を再描画し、2 つのタスクを実行する —
  `spec/redstone` §14.5 の 5 段パイプライン（Placement → Steiner
  routing → Delay insertion → Crossing legalization → Edition
  legalization）の第 4 段。
  タスク 1 は plane crossing 検出: cell/pad ではない wire coord を
  2 つの異なる net が占めるケースを検出し、`void=<N>` 予約に plane
  より上の y-layer が無い（`void < 2`）場合は新診断
  `E_CROSSING_CONGESTION` を発火して refuse する。v1 では wire
  crossing 自体を `Bridge` layer に持ち上げない — routed wire path
  は IR に保存されないため escape 記録の attach 先がなく、代わりに
  crossing coord set は pass 内でタスク 2 のバッファ配置を steer
  するためだけに使われる。タスク 2 は暗黙 buffer repeater の座標
  割り当て: 各 driver segment について L-shape 経路（x → z → y、
  routing と同じ軸順）を再走し、`k * DUST_ATTENUATION_LIMIT`
  （`k = 1..=buffer_count`）の点に buffer を置く。cell / pad /
  plane crossing / 既配置 buffer と衝突する候補は
  `void=<N>` 予約内の最初の空いた `RouteLayer::Bridge` y-layer
  （`y in 1..void`）にエスケープし、その `(x, z)` の全 bridge
  y-layer が塞がっている場合は新診断 `E_BUFFER_COORD_COLLISION`
  で refuse する。両診断ともユーザーへの自己修正三点セット
  （「`void` を増やす」「region を広げる」「複数の `circuit` に
  分割する」）を提示し、`CrossingCongestion` の primary は衝突する
  2 net の名前を anchor 座標と共に含めるので、ユーザーは原因の
  ソースレベル信号を特定できる。
  新しい IR 型は追加しない: crossing pass は `PlacedCellNode` の
  phase 表に沿った field write。`CellCoord` に `layer: RouteLayer`
  （`Plane` / `Bridge` / `Via`; `Via` は v1 で producer なし、
  reserved と明記）を追加、`PlacedCellNode` に
  `buffer_coords: Vec<CellCoord>` を追加し、crossing pass は
  delay pass が数えた implicit buffer repeater の具体座標を 1 個
  ずつここに埋める。両フィールドともデフォルト値では serde-skip
  され（`layer` は `Plane` のとき、`buffer_coords` は空のとき）、
  placement / routing / delay JSON dump は `stage` タグを除けば
  legalized IR dump の additive subset として扱える — legalize 対象の
  無いスコープでキーは増減せず、変わるのはタグの値だけ。
  失敗したスコープは crossing
  出力から drop され、下流の block-array voxel 落としが
  「実現不能なレイアウトに対する部分 buffer」を silent に
  materialise することはない。CLI の `cairn synth --stage` に
  `crossing` 値を追加。`--edition <java|bedrock>` フラグは
  `edition` / `placement` / `route` / `delay` と同様に必須で、
  edition-neutral な `logic` / `netlist` stage では引き続き
  exit 2 で拒否する。`--stage crossing` は upstream の fail-loud
  を継承する: routing 段で `E_ROUTE_CONGESTION`、delay 段で
  `E_ATTENUATION_LIMIT` に落ちたスコープはそれぞれの段で報告され
  exit 1 になり、crossing pass は走らない。今回のスコープ外:
  wire crossing の `Bridge` / `Via` materialisation、edition
  legalization、block-array voxel 落とし、physical-tile（3 層目）
  cell library、tick simulator、`assert truth|always|latency`
  の評価、シーケンシャルマクロ（`latch` / `pulse` / `delay` /
  `edge_*` / `counter`）、QC/BUD 拒否 (`E_NO_PORTABLE_IMPL`)。
- Redstone delay insertion と `cairn synth --stage delay
  --edition <java|bedrock>`（M6-PR6）— M6 redstone-simulates
  パイプラインの 6 枚目。`cairn-lang-redstone` に
  `compile_delay(&ScopedPlacementIr) -> DelayOutput` エントリポイントを
  追加し、M6-PR5 の routed Placement IR を走査して各セルの
  `delay_ticks` を `None` から `Some(base delay + implicit buffer
  repeater 由来 tick)` に書き換える — `spec/redstone` §14.5 の 5 段
  パイプライン（Placement → Steiner routing → Delay insertion →
  Crossing legalization → Edition legalization）の第 3 段。新しい IR
  型は追加しない: delay pass は `PlacedCellNode` の phase 表に沿った
  field write で、M6-PR5 の `wire_length` write と対称。base delay は
  `EditionCell::edition(self)` の兄弟として `const fn
  base_delay_ticks(self)` を追加し、cell library の変種テーブルの
  隣に tick 数字を置く: Java `ComparatorAnd` / `RepeaterOr` /
  `InverterTorch` と Bedrock `InverterTorch` はそれぞれ 1 tick、
  Bedrock `TorchAnd` は 2 tick（NAND→NAND の 2-torch 直列）、Bedrock
  `TorchOr` は 0 tick（bare dust merge）、`*Unpinned` variant は
  `UNPINNED_BASE_DELAY_TICKS`（3 tick — 現在 pinned な最大 2 tick より
  厳密に大きい）を返す pessimistic sentinel（将来の pinned rename が 1 行の
  match arm 書き換えで済み、delay 見積もりを silent に狂わせず、
  pinned 値と混同されない）。暗黙
  buffer repeater は dust attenuation 上限 15 blocks を跨ぐ driver
  segment に付く: 長さ `s` blocks の segment は `floor((s - 1) /
  DUST_ATTENUATION_LIMIT)` 個の buffer を実装扱いし、各 buffer は
  `BUFFER_REPEATER_TICKS`（1 tick, デフォルトの `repeater delay=1` に
  一致）を寄与する。buffer は **暗黙**扱いで座標を割り当てない — routing
  pass は自身の per-scope occupancy 集合を既に破棄しており、buffer
  座標決定は stage 4（crossing legalization）が cross-net overlap を
  `RouteLayer::Bridge` / `Via` layer にエスケープするのと合わせて
  owner になる方が自然だから。新診断 `E_ATTENUATION_LIMIT` は driver
  segment が `MAX_ATTENUATION_SEGMENT`（256 blocks — buffer 16 個
  連続分）を超えたときのみ発火する v1 sanity cap。`(DUST_ATTENUATION_LIMIT,
  MAX_ATTENUATION_SEGMENT]` の帯は正常経路で暗黙 buffer が吸収し、
  256 blocks 超えは stage-4 bridge/via 幾何が必須の非現実的な長さで
  fail-loud させる。per-driver Manhattan segment は routing pass と
  同じ `NetRef → source coord` 経路で再算出する（routing は driver 総和
  としての `wire_length` のみを保存する意図的な選択で、per-driver segment
  は再歩行が安価で JSON に二重に持たせるとむしろ膨らむ）; その共有の
  ために `input_pad` / `output_pad` / `manhattan` を `pub(crate)` に
  昇格 — pad 座標の owner は routing pass のまま維持し、将来
  `PlacementIr` の field に昇格する予定は 1 段の migration に保つ。
  Attenuation で失敗したスコープは delay 出力から drop され、下流の
  tick simulator が partial `delay_ticks` を silent に読み取ることは
  ない。CLI の `cairn synth --stage` に `delay` 値を追加。`--edition
  <java|bedrock>` フラグは `edition` / `placement` / `route` と同様に
  必須で、edition-neutral な `logic` / `netlist` stage では引き続き
  exit 2 で拒否する。`--stage delay` は upstream の fail-loud を継承
  する: routing 段で `E_ROUTE_CONGESTION` に落ちたスコープは
  routing 段で報告され exit 1 になり、delay pass は走らない。今回の
  スコープ外: crossing legalization と `RouteLayer::Bridge` / `Via`
  エスケープ、edition legalization、block-array voxel 落とし、
  physical-tile（3層目）cell library、tick simulator、`assert
  truth|always|latency` の評価、シーケンシャルマクロ（`latch` /
  `pulse` / `delay` / `edge_*` / `counter`）、QC/BUD 拒否
  (`E_NO_PORTABLE_IMPL`) — それぞれ本 PR で確定した delayed Placement
  IR shape の上に後続 PR が積む。
- Redstone Steiner routing と `cairn synth --stage route
  --edition <java|bedrock>`（M6-PR5）— M6 redstone-simulates
  パイプラインの5枚目。`cairn-lang-redstone` に
  `compile_routing(&ScopedPlacementIr) -> RoutingOutput`
  エントリポイントを追加し、M6-PR4 の Placement IR を走査して各
  スコープの `circuit region=` 予約領域の中に driver net ごとの
  Manhattan Steiner tree を敷く — `spec/redstone` §14.5 の 5 段
  パイプライン（Placement → Steiner routing → Delay insertion →
  Crossing legalization → Edition legalization）の第 2 段。新しい
  IR 型は追加しない: routing pass は `PlacedCellNode` の phase 表に
  沿った field write で、各セルの `wire_length` を `None` から
  `Some(driver source から cell への Manhattan 距離の総和)` に
  書き換える。`delay_ticks` は本段でも `None` のまま — §14.4 が
  「delay は routed wire length + 物理セル選択から決まる」と規定して
  おり、これは stage 3 の担当だから。v1 のアルゴリズムは後続 pass が
  必要とする shape を最小限で満たす構成に絞る: net 収集（NetRef
  ごとに source coord → sink coords）、Kou-Markowsky 風の
  rectilinear MST（`{source} ∪ sinks` の完全 Manhattan グラフに
  Kruskal、重み/インデックスで決定論的な tie-break を打つので
  regression story が pin される）、L-shape 描画（x → z → y の
  固定順で安定性確保）、スコープ単位の `HashSet<CellCoord>` 占有
  集合（全 cell coord と入力 / 出力 pad で seed）、そして congestion
  予算用の wire-only footprint の総和。入力 pad 座標は `(x=0, y=0,
  z=1+i)`、出力 pad 座標は `(x=width-1, y=0, z=1+k)` に置き、
  degenerate region では `depth-1` で飽和させる — これは v1 の
  convention として crate-private に閉じ、routing の外側で必要に
  なった時点で `PlacementIr` の `input_pads` / `output_pads` フィールド
  として `#[non_exhaustive]`-safe に追加する。既存の
  `E_ROUTE_CONGESTION` コードはここで再発火し、判定基準は placement
  pass の cell-only pessimistic budget ではなく実際の post-routing
  footprint（`cells.len() * CELL_FOOTPRINT + unique wire coords >
  reserved_area`）を使う。primary は `routed netlist occupies
  ~N.Mx the reserved area (void=V, region WxD)` と読み、下流の
  reader が placement 側の fail-loud と routing 側のそれとを区別
  できる。footer は §14.5 の 3 つの修正 triple をそのまま維持する。
  placement の pessimism（cells × 4）のおかげで、routing が
  `E_ROUTE_CONGESTION` に落ちるスコープはほぼ必ずセルが予約領域の
  境界きっかりまで詰まっていて、あと Manhattan で 1 段のワイヤを
  引くだけで flip するもの — 意図的なコストモデルであり、二重検出の
  見落としではない。congestion で失敗したスコープは routing 出力
  から drop されるので、下流 pass が partial routed layout を silent
  に受け取ることはない（earlier stage と同じ fail-loud cascade
  ポリシー）。CLI の `cairn synth --stage` に `route` 値を追加。
  `--edition <java|bedrock>` フラグは `edition` / `placement` と
  同様に必須で、edition-neutral な `logic` / `netlist` stage では
  引き続き exit 2 で拒否する。今回のスコープ外: delay insertion
  （リピータバッファ）、attenuation-limit 検出
  （`E_ATTENUATION_LIMIT`、dust segment 15 blocks 超）、crossing
  legalization と `RouteLayer::Bridge` / `Via` エスケープ、edition
  legalization、block-array voxel 落とし、physical-tile（3層目）cell
  library、tick simulator、`assert truth|always|latency` の評価、
  シーケンシャルマクロ (`latch` / `pulse` / `delay` / `edge_*` /
  `counter`)、QC/BUD 拒否 (`E_NO_PORTABLE_IMPL`) — それぞれ本 PR で
  確定した routed Placement IR shape の上に後続 PR が積む。
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
