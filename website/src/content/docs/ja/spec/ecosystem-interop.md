---
title: "12. エコシステム連携と逆変換"
---

## 12.1 順方向の変換

block-array IR をシリアライズすると、`.nbt` / `.litematic` / `.schem` / `.mcstructure` が出力されます ([アーキテクチャ](/ja/spec/architecture/))。各形式はシリアライザで、既存の形式は、ピボットの周りに追加するバックエンドという位置付けです。現在は 4 つすべてのシリアライザがあります。

`cairn compile --edition bedrock` は `.mcstructure` を書き出します。`--edition java` は `.nbt` を書き出すか、`--structure-format` が指定した形式で書き出します。

| `--structure-format` | 書き出すもの | `site` の書き出し方 |
|---|---|---|
| `nbt` (既定) | struct、配置、通路ごとに 1 つのバニラ構造ファイル | 配置ごと、通路ごとに 1 ファイル ([§9.3.4](/ja/spec/components-editing-sites/#934-出力ファイル名)) |
| `litematic` | struct ごと、site ごとに 1 つの Litematica schematic ([§12.1.1](#1211-litematica)) | 1 ファイルにまとめ、配置ごと、通路ごとに 1 つのリージョン |
| `schem` | struct、配置、通路ごとに 1 つの Sponge Schematic ([§12.1.2](#1212-sponge-schematic)) | 配置ごと、通路ごとに 1 ファイル。それぞれ site の中の原点だけずらす |

形式で変わるのはファイルだけです。block array は同じで、成果物は同じ規則で名付け、同じ衝突で拒否し、ロックファイルはどの形式で書き出してもバイト単位で同じです。Bedrock の構造形式は 1 つだけなので、`--edition bedrock` と一緒に `--structure-format` を指定すると使い方の誤り (終了コード 2) になり、ソースを読む前に拒否します。

### 12.1.1 Litematica

`.litematic` は、Litematica が保存して読み込む、gzip で包んだ Java の NBT です。struct はそれだけで 1 ファイルになり、struct の名前を付けて (`cottage.litematic`)、原点に 1 つのリージョンを置きます。site は site の名前を付けた 1 ファイル (`hamlet.litematic`) になり、配置ごと、通路ごとに 1 つのリージョンを、ソルバーが決めた原点に置きます。この原点はロックファイルに記録するものです。リージョンには、その配列を `.nbt` に書き出すときのファイル名を付けます (`home1`、`hamlet_walkway_home1_entry__home2_entry`)。これは取り込みが来歴として残す形です ([§12.4](#124-取り込み時の記録と落とし穴))。site のリージョンは、読み戻すとその配置になります。同じ名前の site と struct は 1 つのファイルに書き出すことになるので、`.nbt` のファイル名が重なる 2 つを拒否するのと同じく、何も書き出す前に拒否します。

ルートには次のものを入れます。

- `MinecraftDataVersion` はターゲットの DataVersion、`Version` はそのリリース向けの Litematica が読める schematic のバージョンで、1.20.5 の DataVersion (3837) より前なら 6、それ以降なら 7 です。`SubVersion` は 1 です。
- `Metadata`: ファイルの名前にあたる `Name`、ソースには作者を書かないので `Cairn` とする `Author`、空の `Description`、`RegionCount`、リージョンのボクセル数を合計した `TotalVolume`、そのうち air、cave air、void air 以外の数を数えた `TotalBlocks`、すべてのリージョンを囲む箱の大きさの `EnclosingSize` です。作成時刻、更新時刻、プレビュー画像は入れません。そのため、同じソースからは常に同じバイト列が書き出されます。
- `Regions`: 各リージョンには、最小の角にあたる `Position` と、どの軸も正の `Size` を入れます。`BlockStatePalette` には、リージョンのブロック状態を air を先頭にして並べ、どの DataVersion でも `Name` と `Properties` のキーで書きます。構造ファイルが `id` と `properties` に切り替わる版でも、Litematica はこのキーのままだからです。`BlockStates` には、各ボクセルのパレット番号を `(y, z, x)` の順に、最大の番号に必要なビット数 (2 ビット以上) で、各 long の下位から詰めます。1 つの long の残りに収まらない番号は、次の long にまたがります。`TileEntities` には、各ブロックエンティティを `.nbt` の書き出しが `nbt` に書くのと同じ形で入れ、リージョンの角からの `x`、`y`、`z` を加えます。`Entities`、`PendingBlockTicks`、`PendingFluidTicks` は空です。

### 12.1.2 Sponge Schematic

`.schem` は、WorldEdit が保存して読み込む、Sponge Schematic Specification の gzip で包んだ Java の NBT です。Cairn はバージョン 3 で書き出します。struct、配置、通路はそれぞれ 1 ファイルになり、`.nbt` と同じ名前を付けます (`cottage.schem`、`home1.schem`)。そのため site は、`.nbt` で書き出すときと同じファイルの組になります。各ファイルの `Offset` には、その配列が site の中で置かれる位置、つまりロックファイルが記録する原点を入れ、struct では `[0, 0, 0]` とします。site のファイルをすべて同じ地点に貼り付けると、site が組み上がります。

バージョン 2 ではなくバージョン 3 で書き出す理由は次のとおりです。

- WorldEdit は 7.3.0 からバージョン 3 を読み書きでき、7.3.0 以降の WorldEdit は、Cairn が出荷する Java のターゲット (1.20.4 から) のどれでも動きます。
- バージョン 3 のブロックエンティティは、フィールドを `Id` と `Pos` と並ぶ `Data` コンパウンドに入れます。block array がブロックエンティティの id とフィールドを分けて持つのと同じ形です。バージョン 2 はフィールドを `Id` と `Pos` と同じ階層に並べるので、どちらかと同じ名前のフィールドがあると衝突します。
- バージョン 3 は、Minecraft 自身のデータファイルと同じく、schematic を名前のないルートの中に入れます。

`.schem` の読み込みはどちらのバージョンも受け付けるので、バージョン 2 しか書き出さないツールのファイルも読めます。

ルートには `Schematic` コンパウンドを 1 つ入れ、その中に次のものを入れます。

- `Version` は 3、`DataVersion` はターゲットのものです。
- `Metadata`: ファイルの名前にあたる `Name` と、`Cairn` とする `Author` です。保存した日時は入れません。そのため、同じソースからは常に同じバイト列が書き出されます。
- `Width`、`Height`、`Length` は、配列の x、y、z 方向の大きさです。どれも符号なし short で保存するので、いずれかの軸で 65535 を超える配列は拒否します。
- `Offset` は前述のとおりです。
- `Blocks`: `Palette` は、各ブロック状態をプロパティを名前順に並べた `namespace:id[name=value,...]` の綴りで書き、block array のパレット (air が先頭) での番号に対応付けます。プロパティの値にカンマを含むものなど、綴りを読み戻すと別の状態になってしまうものは拒否します。`Data` には、各ボクセルの番号を `(y, z, x)` の順に varint で入れます。`BlockEntities` には、各ブロックエンティティを `Pos`、`Id`、そして `.nbt` の書き出しが `nbt` に書くフィールドを入れた `Data` として入れます。

バイオームとエンティティは書き出しません。

## 12.2 逆方向の変換: コンパイラは写し取り、LLM がリフトする

コンパイラは、ボクセルを見て「これは壁だ」と判定するようなコンピュータビジョンは作りません。保守できなくなるからです。コンパイラが実装するのは、堅牢で忠実な写し取り、検証、ボクセル単位の差分です。意味を付けて引き上げる作業 (リフト) は、raw を中心にした DSL を LLM がリファクタリングする形で行います。言語を自分で使って確かめることにもなり、P5 の自己修正ループとも一致します。

```text
cairn import house.litematic --mode raw    → house.raw.crn     # fill/raw_block 中心
(LLM が house.raw.crn を意味 DSL にリファクタ) → house.lifted.crn
cairn compile house.lifted.crn --edition java --target 1.21.4
cairn diff-blocks house.litematic house.lifted.crn             # voxel XOR → 自己修正へ
```

逆方向の変換はまだ実装していません。 `cairn import` も `cairn diff-blocks` もまだないので、この節とそれに続く 2 つの節の内容は、現在はどれも動きません。

compile → diff → patch のループは、次のように報告します。

```text
E_DECOMPILE_DIFF: block IoU = 0.962 < threshold 0.985
  missing bbox=(12,4,3)..(18,6,3) mat=glass_pane → likely window repeat too small
  Suggested patch: edit window[id=front_windows] set repeat=4
```

`E_DECOMPILE_DIFF` は未実装です。また、修正案が使っている `edit` 文もまだパースできません ([§9.2](/ja/spec/components-editing-sites/#92-編集モデル))。

収束の閾値は、block IoU ≥ 0.985、`state_accuracy` ≥ 0.995、残った raw ≤ 5% です。完全な一致は求めません。残差は `raw_fill id=residual_* origin=imported` として明示的に残します。

## 12.3 写し取りの 3 段階

写し取りとリフトの境界は、名前を付けるかどうかです。

| 段階 | 内容 | 上限 |
|---|---|---|
| **L0: raw セル** | 1 行に 1 ボクセル。LLM の文脈には大きすぎるので、中間表現としてだけ使う。 | — |
| **L1: 空間的な圧縮** | fill への集約、AABB によるパレット圧縮、`resolved_state` から `intent_state` への逆変換 (`stair facing=east half=top`)、対称性と周期性の `raw_repeat` への畳み込み。**名前は付けない。** | コンパイラにできる上限。 |
| **L2: 意味のリフト** | fill → `wall`、repeat → `def` / `use`、具体的なブロック → `mat_slot` + `theme`。 | LLM にできる上限。 |

```
# L1: 名前を付けない。決定的
raw_repeat id=r03 count=5 step=3,0,0: raw_fill mat=minecraft:glass_pane from=0,2,0 to=1,3,0
# L2: LLM が名前を付け、意味を与える
window id=front_windows side=front mat_slot=glass repeat=5 ...
```

## 12.4 取り込み時の記録と落とし穴

取り込むときは、`(edition, version)` と来歴 (provenance) を block-array IR に記録します (`.litematic` → java、`.mcstructure` → bedrock、`.schem` → java)。これによって、取り込んだものも再現できるようになります ([バージョンとエディション](/ja/spec/versioning-editions/))。

**取り込みを「作者の意図の復元」として見せてはいけません。** これが最大の落とし穴です。復元できるのはボクセルと一部の規則性だけで、そのことは CLI の `W_SEMANTIC_LOSS` に示されます。ただし取り込みはまだないので、何も記録されず、`W_SEMANTIC_LOSS` も未実装です。

ほかの規則:

- 取り込みに由来する `raw_fill` は `origin=imported` / `residual` で区別し、設計用の第一級の DSL としては扱いません。
- Litematica の複数のリージョンとサブリージョンのオフセットは、平坦化せずに来歴として残し、リージョンは `site` または複数の struct に対応付けます。
- エンティティを含む schematic では、block IoU だけで成功と判定してはいけません。エンティティ用の指標を別に持ち、第一級のエンティティ ([エンティティ](/ja/spec/entities/)) だけを取り出します。チェストの中身とコマンドブロックは捨てます。
- 巨大な schematic (48³ を超えるものや村全体) を一度にリフトすると、LLM の文脈に収まりません。ストリーミングでパースしながら、チャンクへの分割 → チャンクごとの L1 → 部分ごとのリフト → `site` での結合、という流れをまとめて制御する必要があります。
- 1.13 のフラット化より前の、数値 ID の `.schematic` は v1 では扱いません ([目的とスコープ](/ja/spec/overview/))。
