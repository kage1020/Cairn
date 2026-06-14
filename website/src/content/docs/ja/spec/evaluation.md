---
title: "13. 評価フレームワーク"
---

仕様の品質は嗜好ではなく、**ヘッドレスなジオメトリシミュレータ** (Minecraft 本体に依存しない) で
定量的に反復します。語彙/構文の議論は好みに流れるので、まず評価ベンチを固定します。

```text
テストプロンプト集 (~50)
  → zero-shot 生成
  → ヘッドレス lint (構文 + AABB ジオメトリ展開で「壁外の窓」「空中のドア」などを検出)
  → 行番号付きエラーを返し、最大 3 ターン自己修正する
```

## 13.1 主要メトリクス
- **Zero-shot Compile Rate**: 初回でエラー無しコンパイルできる割合 (= 仕様の直感性)。
- **Fix Convergence Rate**: 3 ターン以内にエラー無しに収束する割合 (= エラーメッセージの表現力)。
- **Token Efficiency**: 展開ブロック数 / DSL トークン数。
- **Edit Stability**: 「2 階の窓だけアーチに」のような追加編集後の AST diff / NBT diff の小ささ。

## 13.2 逆方向の補助メトリクス (decompile_quality)
逆方向 ([エコシステム連携](ecosystem-interop)) の品質は主要評価から外し、補助メトリク
スとして扱います (lossy アプローチと整合)。核心は「形を再現したか」ではなく **「編集可能な DSL になっ
たか」** を測ることです。

- `block_iou`、`state_accuracy` (facing/shape/waterlogged 一致)、`entity_accuracy`
  (frame/sign/villager/display の保持)
- `residual_ratio` (リフト後に残った raw 体積)、`compression_ratio` (ボクセル数 / トークン数)
- `editability_score` (名前付きメンバ数、slot 化率、安定アドレス率)
- `theme_extraction_score` (具体ブロックがインライン化されず slot/theme に分離されたか)
- `symmetry_score` (repeat/mirror/def に畳まれた割合)、`version_portability` (正規トークン率)

## 13.3 運用ルール
語彙の追加 / 構文変更は、これらのメトリクス (特に Fix Convergence Rate と Edit Stability) を改善する
方向のみ採用します。「モデルに仕様だけを与えて生成させ、どこでエラーが出るか観察する」実験を回せば、
構文/語彙の論争のほとんどは実データで決着します。

逆方向の評価ハーネスは、コミュニティの schematic コーパスから `def` / `theme` 標準ライブラリを成長
させるエンジンも兼ねます:

```text
コーパス → 取り込み → 正規化(エディション/バージョン) → L1 圧縮 → クラスタリング(形/マテリアル)
  → LLM リフト候補 → コンパイル/diff → 人手レビュー → def/theme ライブラリ
```

## 13.4 レッドストーン検証
ヘッドレスジオメトリ sim は **tick 単位のレッドストーン論理 simulator** に拡張されます。ターゲット
エディションごとに合成回路をシミュレートし、宣言された真理値表/時相アサーションと突き合わせます
(synth→sim→diff→patch)。[レッドストーン](redstone) 参照。
