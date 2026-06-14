---
title: "Cairn チュートリアル"
---

[サンプル](../examples) ディレクトリの実例を順に辿るガイド付きツアー。各セクションは、例の各行を
背後にある仕様章にマッピングします。チュートリアルがそのまま注釈付き読み順表になります。

このチュートリアルは [目的とスコープ](../spec/overview) と
[設計原則](../spec/principles) を先に読んでいる前提です。用語で詰まったら
[用語集](../spec/glossary) が最速のジャンプテーブルです。

> リファレンスコンパイラはまだ実装されていません。`cairn compile` の呼び出しは将来形ですが、仕様
> 通りなので、CLI の感覚を掴むには読むだけで十分です。

## 1. 最小限の実用ビルド — [`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn)

Cairn の Hello, world。ドア、窓、切妻屋根のあるコテージ。

```
@cairn 2026.06
@requires version>=1.20

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  slot glass -> @glass_pane
  window[class=small] -> frame=@spruce_wood

struct cottage size=9x7
  floor  mat_slot=floor
  walls  class=outer mat_slot=wall height=4
  door   side=front at=center
  window class=small side=front offset=2 y=2 size=2x2 sym=true mat_slot=glass
  roof   kind=gable mat_slot=roof overhang=1
```

注目点:

1. **ヘッダはオプションだが安い**。`@cairn` はファイルが書かれた Cairn 言語のバージョンを記録し、
   `@requires` は Minecraft ターゲットへの capability の下限です。Minecraft のバージョンそのものは
   **絶対にソースに書きません** — コンパイル時の `--target` でだけ与えます。
   ([syntax §5.3](../spec/syntax))
2. **`theme` は「何で」と「どこ」を分離する**。構造体は `mat_slot` 注入ポイントを持ち、テーマがそれを
   正規ブロックトークン (`@oak_planks` など) に束ねます。テーマを切り替えても構造体には触れません。
   ([materials-themes §7.1](../spec/materials-themes))
3. **1 行 1 コマンド、key=value**。先頭のキーワード (`floor`、`walls`、`door` など) だけが位置トークン
   で、それ以外は `key=value` です。これは意図的な設計です。すべてのパラメータに attention アンカー
   を与え、LLM の生成を安定させます。
   ([principles P3](../spec/principles)、[syntax §5.1](../spec/syntax))
4. **セレクタは意味的で、座標ではない**。`side=front`、`offset=2`、`y=2`、`at=center` は壁の位置と
   壁に沿ったオフセットを指します。作者面に絶対座標は出てきません。
   ([principles P4](../spec/principles)、[syntax §5.4](../spec/syntax))
5. **フェーズ順、ソース順ではない**。`window` は `roof` より後に書かれていますが、それでも壁の開口
   として切られます。コンパイラは評価前にコマンドを固定フェーズパイプラインに振り分けるからです。
   ([compilation §4.1](../spec/compilation)、[principles P2](../spec/principles))
6. **ブロックステートはデフォルトで導出**。ドアの `facing=south` も、壁の `north=tall` も、ガラス
   ペインの `connected` 状態も、誰も書きません。コンパイラが位置と隣接から導出します。
   ([blockstate §6.1](../spec/blockstate))

コンパイル (将来形。リファレンスコンパイラはまだスケルトンです):

```sh
cairn compile examples/cottage.crn --edition java    --target 1.21.4
cairn compile examples/cottage.crn --edition bedrock --target 1.21.40
```

## 2. テーマ、抽象トークン、上書きによる昇格 — [`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn)

2 階建ての石造り keep。新しい考え方が 3 つ入ります: **抽象マテリアルトークン**、**レベル**、**上書き
による昇格**。

```
theme keep_dark:
  slot floor -> @floor.wood.broadleaf   # 抽象トークン
  slot wall  -> @wall.stone.cobble
  slot trim  -> @wood.dark
  slot roof  -> @roof.dark_wood

struct keep size=11x9
  ...
  level id=floor2 y=5
    walls  id=upper class=outer mat_slot=wall height=4
    window class=arrow_slit side=front repeat=3 step=2 y=2 size=1x2 shape=slit
    stair  id=eave kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left
```

注目点:

1. **正規トークンの 2 段** ([materials-themes §7.2](../spec/materials-themes))。
   - `@oak_planks` は *canonical block token*: 意味そのもの。サイレントなダウングレードは禁止。
   - `@floor.wood.broadleaf` は *abstract material token*: テーマポリシーがターゲットに応じて
     ダウングレードしてよい美的選択 (オーク ↔ シラカバ)。
2. **`level`** はフロアごとのローカル `y=0` を与えます。2 階の窓はワールド床ではなく自分のフロアから
   `y=2` のまま書けます ([open-issues §15.2](../spec/open-issues) でこの面は調整余地が予約され
   ていますが、現状の構文は十分に教えられる安定度です)。
3. **上書きによる昇格**。`stair id=eave` の行は `half=top facing=out shape=outer_left` を明示的に
   書きました — これらは導出値ではなく *意図* になります。ブロックステートモデルは
   「デフォルトは導出。意図になり得るブロックステートはすべて上書き可能」です。
   [blockstate §6.1](../spec/blockstate) の上書き可能ケース一覧を読んでください。
4. **`shape=slit` 窓プリミティブ**。すべてのプリミティブは IR に `anchor`、宣言 bbox、ホスト面を持つ
   ので、非矩形のアロースリットでも周囲の壁ブロックステートと綺麗に合成されます
   ([entities §8.2](../spec/entities))。

## 3. 論理的なレッドストーン — [`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn)

レッドストーン面は Cairn の中で最も仕様寄りです。dust や repeater を置く代わりに、*信号グラフ* を
宣言すると、コンパイラが回路を合成・配置・配線します。

```
pressure_plate id=plate at=front.outside offset=0 y=0 -> sig.step
pressure_plate id=inner at=inside.front  offset=0 y=0 -> sig.exit

logic sig.open = sig.step or sig.exit
door[id=front] opened_by=sig.open

circuit region=floor void=2

assert truth(sig.step, sig.exit -> sig.open) { 00->0; 01->1; 10->1; 11->1 }
assert always(sig.step -> eventually sig.open within 2)
```

注目点:

1. **信号グラフが IR**。`sig.*` はデータフローノードに名前を付けます。センサが信号を発し、アクチュ
   エータが消費し、`logic` がそれらの依存を書きます。
   ([redstone §14.2–14.3](../spec/redstone))
2. **tick 演算なし**。論理式に時間はありません。アサーションの `within 2` だけが「数値 = tick」の
   現れる唯一の場所です。ディレイは Placement IR で初めて決まります。
   ([redstone §14.4、§14.8](../spec/redstone))
3. **`circuit region=…`** が place-and-route 用のスペースを確保します。配線の輻輳が確保面積を超え
   たら `E_ROUTE_CONGESTION` エラーが推奨修正付きで出ます。コンパイラはサイレントにオーバーフロー
   しません。
4. **3 種のアサーション**。`truth(…)` が組み合わせ、`latency(in → out) <= N` が有界ディレイ、
   `always(in -> eventually out within N)` が有界時相。設計上、完全な LTL は提供しません — tick
   simulator が安価に決定できるものだけを扱います。
   ([redstone §14.7](../spec/redstone))
5. **エディション差はセルライブラリで、言語側ではない**。同じ論理が Java では `ComparatorAND` セル
   に、Bedrock では `TorchAND` セルにコンパイルされます。QC/BUD 依存の回路はサイレントな罠ではなく
   コンパイルエラーになります。 ([redstone §14.6](../spec/redstone))

## 4. 複数建築 — [`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn)

1 軒のコテージが動いたら、サイト上で再利用します。サイト側で絶対座標を計算する必要はありません。

```
def cottage class=house size=9x7:
  ...

site hamlet:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  place id=home3 use=cottage theme=medieval north_of=home1 gap=5

  connect home1.entry to home2.entry path=@gravel
  connect home1.entry to home3.entry path=@gravel
```

注目点:

1. **`def` はスロット保持型コンポーネント**。`theme` および `site` と同じ機構で、参照系が編集・
   テーマ化・複数建築接続の間で分裂しません。
   ([components-editing-sites §9.1](../spec/components-editing-sites))
2. **トポロジカルな配置**。`east_of=home1 gap=4` は制約で、絶対座標はコンパイラの責務。LLM の算術
   ミスの最悪クラスを回避します。
   ([principles P4](../spec/principles)、
   [components-editing-sites §9.3](../spec/components-editing-sites))
3. **各 struct はポートを露出**。`home1.entry` は `def` 内の door メンバを指し、`connect` がパス
   スロット経由で 2 つのポートを繋ぎます。
4. **structure block の 48³ 上限が溶ける**。1 つの structure block に収まらない村や城は、複数 `def`
   の `site` 上の合成として表現します。

## 次のステップ

- **編集**。パッチ DSL (`edit window[class=vent] set shape=arch`) は
  [components-editing-sites §9.2](../spec/components-editing-sites) を参照。編集 diff は
  `intent_state` だけを見るので、編集後に解決状態を再導出しても安全です。
- **ターゲティング**。`cairn info` がファイルのレジストリ互換範囲と意味敏感メンバを返します。
  [versioning-editions §10.5](../spec/versioning-editions) 参照。
- **インポート**。`cairn import` ワークフロー (忠実写し取り → LLM による意味リフト → voxel diff で
  ループを駆動) は [ecosystem-interop §12](../spec/ecosystem-interop) を参照。
- **評価メトリクス**。仕様に押し戻したい場合、Cairn が論じる言語は
  [evaluation §13.1](../spec/evaluation) の 4 メトリクスです。
