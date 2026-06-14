---
title: "14. レッドストーン (論理回路)"
---

Cairn はレッドストーンを **論理レベル** で記述できます。作者は **信号グラフ (データフロー)** を宣言し、
コンパイラが実際の dust/repeater/torch/comparator を **合成 → 配置 → 配線 (place-and-route)** して
ボクセル化します。P1 (意図を宣言し、物理はコンパイラが解決) が最も効くのがこのアプリケーションです。
信号減衰、クロストーク、ディレイ計算 — AI がボクセル建築よりさらに苦手な物理 — を、論理記述から決定論
的に導出できます。

**設計の核**: 論理層のファーストクラスのオブジェクトは「挙動」ではなく **信号依存グラフ (IR 化可能な
データフロー)** です。時間は言語コアに持ちません (14.4)。これが P1/P3/P5 と最もよく整合します。

## 14.1 二層モデルと v1 境界 (旧「非目標」を置換)
- **Tier 0 物理配置**: `repeater facing=north delay=2` など。作者が部品を置き、ブロックステートは
  導出。挙動はモデル化しない ([ブロックステート](blockstate))。
- **Tier 1 論理 (本章)**: 信号グラフを宣言し、合成 → 配置 → 配線でコンパイラがボクセル化。

新規キーワードは **`logic` / `circuit` / `assert`** のみです。論理プリミティブは **組み込み `def`
ライブラリ** として提供され、小さく閉じた語彙 (P3) を保ちます。

v1 スコープ (Verilog で言えば `assign` 相当のみ。クロックド代入はなし):
- **○ 組み合わせ**: `and` / `or` / `not` / `xor` / `nand` / `nor` / `mux`
- **○ 厳選された順序マクロ**: `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter`
- **× スコープ外 (→ Tier 0 / raw)**: `always` / `process` / `state` / `case` / FSM / クロックド代入 /
  CPU など一般的な順序合成。

## 14.2 信号バインディング (センサ → 信号グラフ → アクチュエータ)
センサが信号を発し、アクチュエータが消費します。両方とも物理メンバで、先のフェーズで配置されます
([コンポーネント・編集・複数建築](components-editing-sites))。
```
# センサ → 信号
lever      id=sw   side=front offset=2 y=1 -> sig.power
button     id=bt   side=front               -> sig.ring
daylight   id=dl   at=..                     -> sig.day
observer   id=ob   at=.. facing=down         -> sig.tick

# アクチュエータ ← 信号
lamp       id=l1   at=..  lit_by=sig.lamps
piston     id=p1   at=..  powered_by=sig.mem facing=up sticky=true
door       id=d1   ..     opened_by=sig.power
dispenser  id=ds   at=..  fired_by=sig.pulse facing=south
```

## 14.3 論理層 = 信号依存グラフ (DAG)
作者は信号間の依存 (ブール組み合わせ + マクロ適用) を書きます。これは純粋で時間を持たないデータフロー
で、コンパイラ内部で Logic IR (DAG) になります。
```
logic sig.lamps = sig.power and not sig.day
logic sig.mem   = latch(set=sig.a, reset=sig.b)   # RS latch (マクロ)
logic sig.pulse = pulse(sig.ring, 4)              # 単安定: 4 段 (→ 内部で repeater 段に展開)
logic sig.fire  = edge_rising(sig.tick)
logic sig.sel   = mux(sel=sig.s, a=sig.x, b=sig.y)
```
- 論理式自身に時間演算はありません (14.4)。`pulse(sig.ring, 4)` の `4` は **段数** であり、tick 値では
  ありません。

## 14.4 時間モデル: 言語コアには持たない
- v1 では **マクロ (`delay`/`pulse`/`edge`/`latch`/`counter`) のみが時間を持ちます**。`delay(3)` は内部で
  `Repeater×3` に降りるセルマクロです。**tick 演算子を書く DSL ではありません**。
- **ディレイは Logic IR にも Netlist IR にも持ちません。Placement IR で初めて決まります** (14.8)。
  `and` は論理的にはゼロディレイですが、tick 数はセル選択 (`and → ComparatorAND(Java)`) と配置後の
  実ワイヤ長が分かって初めて決まります。
- 時間 (tick) として数値が現れるのは **検証アサーションだけ** (14.7) です。作者は論理式の中で tick 演算
  をしません。

## 14.5 Place-and-route: DSL は 2D、内部は擬似 2.5D
ユーザに見せるメンタルモデルは 2D ですが、純 2D のフロアプランでは詰まるので、**内部実装は擬似 2.5D
にし、交差・fanout・ワイヤ長を扱います**。内部に `plane` / `via` / `bridge` の 3 概念を持ちます (DSL には
出しません)。
- 純 2D モデルでは扱えない回路クラス: **fanout / bus / 交差 / comparator フィードバック / observer
  チェーン**。
- 内部アルゴリズムは 5 段階: **Placement → Steiner routing → Delay insertion → Crossing legalization →
  Edition legalization**。
  - placement: トポロジ順、左→右。
  - routing: マンハッタン。交差は `bridge tile` か垂直層に逃がす。fanout は木を作る。
  - delay insertion: 信号減衰の 15 を超えるセグメントにのみバッファとして repeater を挿入。
- 配線は `circuit` 領域に閉じます。収まらなければ fail-loud (congestion = 領域不足を報告)。
```
circuit region=basement void=3       # 高さ 3 のサービス層を確保し、合成回路をここに配線
```
```text
E_ROUTE_CONGESTION line 21 circuit=basement:
  synthesized netlist needs ~3.2x the reserved area (void=3, region 9x7).
  Fix: increase `void`, enlarge region, or split into multiple `circuit` blocks.
```

## 14.6 エディション差: セルライブラリで吸収。QC/BUD は合成しない
セルライブラリは 3 段で、**エディション差はライブラリだけに閉じ込めます**:
```
Logical Cell → Edition Cell → Physical Tile
  AND        → Java:    ComparatorAND → block array
             → Bedrock: TorchAND      → block array
```
- **吸収 (○)**: repeater / observer / comparator / 向き (セル実装差)。
- **吸収しない (×)**: QC (quasi-connectivity) / BUD / 更新順序 / quasi-connectivity。これらはブロック
  更新順の暗黙意味論に依存し、可搬実装が存在しません。
- 論理が更新順意味論を要求する場合は **コンパイルエラー (合成不能)** とします。これは
  「recompile であり transcode ではない」と整合します (P1 / [バージョンとエディション](versioning-editions))。
```text
E_NO_PORTABLE_IMPL line 15:
  this circuit requires update-order (quasi-connectivity / BUD) semantics.
  No portable redstone implementation exists for the target edition.
  Fix: redesign the logic to be order-independent, or drop to Tier 0 with an @edition guard.
```
- 論理はエディション中立、合成回路はエディション固有です。手置きのレッドストーンはエディションを
  跨ぐと壊れますが、論理記述ならコンパイラがエディション正確な回路を吐けます — 論理記述の最大の
  動機です。

## 14.7 検証: tick simulator に対して 3 種のアサーションを検査 ([評価フレームワーク](evaluation) を拡張)
意図を宣言した上で、**合成回路を tick 単位でシミュレート (ヘッドレス)** し検査します。アサーションは
3 種です:
```
# 組み合わせ: 真理値表
assert truth(sig.a, sig.b -> sig.out) { 00->0; 01->1; 10->1; 11->0 }

# レイテンシ (P&R がディレイを変えるので重要)
assert latency(sig.in -> sig.out) <= 4

# 時相 (完全な LTL ではない — 有界 eventually のみ)
assert always(sig.button -> eventually sig.door_open within 8)
```
- 自己修正ループ (P5) は **synth → sim → diff → patch**。検証は **ターゲットエディションごと** に
  走ります。
- **パッチが書き換えるのは P&R / 配置ヒント、repeater、バッファのみ。論理 (Logic IR) は決して書き換え
  ません** (論理を自動修正する自己修正は危険)。
```text
E_SIM_ASSERTION_FAILED edition=bedrock:
  assert latency(sig.in -> sig.out) <= 4, but measured 6 (extra repeaters from crossing legalization).
  Patch target: placement hint / route. (logic is never auto-modified)
  Suggested: relax to <=6, enlarge circuit void to shorten routes, or pin cell placement.
```

## 14.8 IR とフェーズへの接続
論理記述では、Intent と block-array の間に **3 つの IR 層** が入ります
([アーキテクチャ](architecture))。役割が違うので分離されています (HDL では標準):
```
Intent IR        (logic 宣言 / circuit 領域 / 信号バインディング)
   ↓ logic_synth
Logic IR         (論理式 / 依存 DAG。エディション中立、ゼロディレイ)
   ↓
Netlist IR       (セル/ネット。Logical Cell 選択。まだディレイなし)
   ↓ logic_place
Placement IR     (セル座標 + 実ワイヤ長 → ディレイ/tick がここで初めて決まる)
   ↓ logic_route
block-array IR   (dust/repeater/torch/comparator のボクセル化)
```
フェーズモデル ([コンパイルモデル](compilation)) は `fixtures` の直後を 3 段に分けます:
```
massing → envelope → openings → fixtures → logic_synth → logic_place → logic_route → raw
```
`fixtures` (センサ/アクチュエータ) が 3D に配置されて初めて I/O ポートの絶対座標が確定し、配置と
配線が可能になります。**ディレイは Logic IR / Netlist IR では持たず、Placement IR で初めて決まります**
(14.4)。

## 14.9 逆方向変換
v1 では、schematic から取り込んだ手置きレッドストーン ([エコシステム連携](ecosystem-interop))
は **Tier 0 raw** として保持します。大量の dust から論理を逆合成するのは v1 のスコープ外です
(generation-first, lossy のアプローチと整合)。
