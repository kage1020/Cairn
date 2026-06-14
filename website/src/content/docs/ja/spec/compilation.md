---
title: "4. コンパイルモデル"
---

## 4.1 フェーズ評価
ソースは行指向、フラット、順不同で書いてかまいません。コンパイラは各コマンドを暗黙のフェーズに
振り分け、**固定順** で評価します:

```
massing (shell: floor/walls/volume)
  → envelope (roof/exterior)
  → openings (door/window)
  → fixtures (装飾物: sign/painting/frame/bed/sensors & actuators)
  → logic_synth (レッドストーン合成: Logic IR → Netlist IR)
  → logic_place (セル配置)
  → logic_route (配線 → Placement IR、ディレイ決定)
  → raw (エスケープハッチ)
```

- ソース内で `roof` の後に書かれた `window` も、壁の開口として適用されます (順序事故が消えます)。
- **last-wins は同一フェーズ内のローカル上書きにのみ適用されます**。`raw` (fill など) は危険ゾーン
  であり、常に最後に適用されます。
- レッドストーン論理 ([redstone.ja.md](redstone)) は `fixtures` の直後を 3 フェーズに分割します。
  センサ/アクチュエータが 3D に配置されて初めて I/O ポートの座標が確定し、配置と配線が可能になり
  ます。

```
struct keep size=11x9
floor  id=base   mat_slot=floor
walls  id=shell  mat_slot=wall height=5
roof   id=roof   kind=gable mat_slot=roof overhang=1
window id=front_windows side=front y=2 offset=2 size=2x2 mat_slot=glass   # roof の後でも開口を切る
door   id=entry  side=front at=center
```

## 4.2 ターゲット軸
ターゲットは **二軸 `(edition, version)`** です。バージョンとエディションは **DSL ソースには書きません**。
バージョン/エディションを知るのはバックエンドのみです ([versioning-editions.ja.md](versioning-editions))。

```sh
cairn compile build.crn --edition java    --target 1.21.4
cairn compile build.crn --edition bedrock --target 1.21.40
```

- `--target` 単独は **禁止** です。`--edition` は **必須** です。
- 同じ「1.21」も Java と Bedrock では異なる意味を持ち、Java の DataVersion は Bedrock の block_version
  とは無関係です。
