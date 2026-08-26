---
title: "13. 評価フレームワーク"
---

仕様の品質は嗜好ではなく、Minecraft 本体に依存しないヘッドレスなジオメトリシミュレータで定量的に反復
します。語彙や構文の議論は好みに流れるので、まず評価ベンチを固定します。

```text
テストプロンプト集 (~50)
  → zero-shot 生成
  → ヘッドレス lint (構文 + AABB ジオメトリ: 「壁外の窓」「空中のドア」など)
  → 行番号付きエラーを返し、最大 3 ターン自己修正する
```

## 13.1 主要メトリクス

| メトリクス | 何を測るか |
|---|---|
| **Zero-shot compile rate** | 初回でエラー無しにコンパイルできる割合。仕様の直感性。 |
| **Fix convergence rate** | 3 ターン以内にエラー無しへ収束する割合。エラーメッセージの表現力。 |
| **Token efficiency** | 展開ブロック数 ÷ DSL トークン数。 |
| **Edit stability** | 「2 階の窓だけアーチに」のような追加編集後の AST 差分 / NBT 差分の小ささ。 |

## 13.2 逆方向変換の補助メトリクス

逆方向変換の品質 ([エコシステム連携](ecosystem-interop)) は主要評価から外し、補助メトリクスとして扱
います。lossy なアプローチと整合させるためです。測るのは「形を再現したか」ではなく **「編集可能な
DSL になったか」** です。

- `block_iou`、`state_accuracy` (facing / shape / waterlogged の一致)、`entity_accuracy`
  (frame / sign / villager / display の保持)。
- `residual_ratio` — リフト後に残った raw 体積。`compression_ratio` — ボクセル数 ÷ トークン数。
- `editability_score` — 名前付きメンバ数、slot 化率、安定アドレス率。
- `theme_extraction_score` — 具体ブロックがインライン化されず slot と theme に分離されたか。
- `symmetry_score` — `repeat` / `mirror` / `def` に畳まれた割合。`version_portability` — 正準トーク
  ン率。

## 13.3 運用ルール

語彙の追加と構文の変更は、これらのメトリクス、とりわけ fix convergence rate と edit stability を改善
する方向にだけ採用します。「モデルに仕様だけを与えて生成させ、どこでエラーが出るかを観察する」実験を
回せば、構文と語彙の論争のほとんどは実データで決着します。

逆方向の評価ハーネスは、コミュニティの schematic コーパスから `def` / `theme` 標準ライブラリを育てる
エンジンも兼ねます。

```text
コーパス → 取り込み → 正規化(エディション/バージョン) → L1 圧縮 → クラスタリング(形/マテリアル)
  → LLM リフト候補 → コンパイル/diff → 人手レビュー → def/theme ライブラリ
```

## 13.4 レッドストーン検証

ヘッドレスのジオメトリシミュレータは、tick 単位のレッドストーン論理シミュレータに拡張されます。ター
ゲットエディションごとに合成回路をシミュレートし、宣言された真理値表と時相アサーションに突き合わせま
す (synth → sim → diff → patch)。[レッドストーン](redstone) を参照してください。
