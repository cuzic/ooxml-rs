# PR #117 レビュー（Opus, round 1）

対象: fix/pptx-full-adjust-list @ 1f35bb087973878a5df8f27970369fa58f17f164

## 結論（先に）
converged: yes。`complete_adjustments` の挿入位置・no-op・`Err ⇒ byte-identical` をプローブで確かめ、
壊れたファイルや黙った誤りは見つからなかった。Blocker/Major は 0。
残りは、カーソル更新の分岐を消してもテストが落ちないこと（m1）と、「プリセット順」が生成側では
名前順で保証されていること（n1）、`hf`/`vf` を PowerPoint 実機で確かめていないこと（n2）。

## 前ラウンドの解消状況
（round 1 のため無し）

## プローブ（スクラッチの一時テスト。コミットしていない）
`set_adjustment` を 1 回呼び、Err なら入力とバイト一致、Ok なら同じ呼び出しをもう一度（dirty tree と再読込の両方）してバイト一致を確かめた。全件通過。

| 入力 `a:prstGeom` | 呼び出し | 結果 |
| --- | --- | --- |
| upArrow `<a:avLst/>`（自己終了） | adj2=1 | `adj1 50000, adj2 1`。冪等 |
| upArrow、avLst 無し | adj1=1 | `adj1 1, adj2 50000`。冪等 |
| upArrow、整形済み（空白テキストノードあり）、adj2 だけ | adj2=7 | adj1 が先頭（空白の前）に入る。既存の空白はそのまま。冪等 |
| upArrow、整形済み、adj1 だけ | adj1=8 | adj2 が adj1 の直後（後ろの空白の前）に入る |
| upArrow、完全なリストで同じ値 | adj1=50000 | **無変更**（バイト一致） |
| upArrow、`adj2, adj1` の逆順 | adj1=3 | 並べ替えず値だけ変わる |
| borderCallout2、`adj4, adj1` | adj1=3 | `adj4, adj1, adj2, adj3, adj5, adj6`。既存を動かさず、欠けた分を補う |
| upArrow、adj1 が重複 | adj1=3 | 先頭の adj1 を更新し、その直後に adj2 を入れる。2 つ目の adj1 は残る（従来どおり） |
| upArrow、未知 guide adj3 | adj3 | `UnknownAdjustment`、バイト一致 |
| rect `<a:avLst/>` | adj | `UnknownAdjustment`、バイト一致 |
| pentagon（adj 無し、`hf`/`vf` だけ） | hf=100000 | `hf 100000, vf 110557` |
| star5 | adj=20000 | `adj, hf 105146, vf 110557` |
| upArrow、adj2 が `val` でない式 | adj1=3 | adj2 の式は保たれる |
| upArrow、avLst 内に別名前空間の要素 | adj1=3 | その要素は残る |

- 生成表の順序: ベンダーした `presetShapeDefinitions.xml` の 187 プリセットすべてで、`avLst` の guide 順は名前順と同じ
  （guide 名は `adj`, `adj1`〜`adj8`, `hf`, `vf` だけ）。`hf`/`vf` を持つのは decagon・heptagon・hexagon・pentagon・star5/6/7/10 の 8 つ。
- `mc:AlternateContent` の中の shape は `shape_mut_by_id` が先に `InsideAlternateContent` で拒否するので、この PR とは関係しない。
- `cargo test -p ooxml-pptx --test shape_properties`（17 件）通過。`cargo xtask codegen --check` 通過。origin/main との merge は衝突なし。
  ADR-0016 に触る open PR は #102 だけで、別の節を変えている。

## Findings
### m1（Minor）: 既存 guide の後ろへの挿入（`cursor = cursor.max(index + 1)`）がテストされていない
- 根拠: `shape_properties.rs` の `Some(index) => cursor = cursor.max(index + 1),` を `Some(_index) => {}` に変えても、
  `--test shape_properties` は 17 件すべて通る。新しいテストは、どれも「空から全部入れる」か「欠けた guide が先頭」のケースで、
  既存の guide の後ろに入れる経路を通らない。この変異があると、upArrow の `[adj1]` に adj1 を書いたとき `[adj2, adj1]` になる
  （プリセット順の主張が崩れる）。
- 修正案: `set_adjustment_completes_a_partial_list_in_order` に、adj1 だけのリスト（できれば整形済みの空白入り）へ書いて
  `adj1, adj2` の順になるケースを足す。

### n1（nit）: 「プリセット順」は生成側では名前順（`BTreeMap`）で決まっている
- 根拠: `xtask/src/preset_geometry.rs` は guide を `BTreeMap<String, i64>` に集めるので、`PRESET_ADJUSTMENTS` は名前順になる。
  上記のとおり現行の 187 プリセットでは ECMA の順と一致するので挙動は正しい。ただし doc コメントと ADR note の
  「in the preset's order」を保証しているのは、この偶然の一致である。
- 修正案: 生成器で元の順序を `Vec` で保つか、パース時に「元の順 == 名前順」を検査して食い違えば codegen を失敗させる。

### n2（nit）: `hf`/`vf` を既定値で書くことを PowerPoint の出力で裏付けていない
- 根拠: `reference/fixtures/pptx` のうち PowerPoint で作られたファイルで、空でない `avLst` は `roundRect`（adj）と、
  PowerPoint 7.0 の chevron/homePlate（adj）だけ。`hf`/`vf` を持つプリセットの例は無い。値は ECMA の既定値なので、
  書いても形状は変わらない（省略したときと同じ）。PowerPoint は hexagon で `adj` + `vf 115470` を書くと記憶しているが、
  リポジトリの中では確かめられない。
- 修正案: マージ後の manual-check バンドル（p3b）に hexagon と star5 の `set_adjustment` を足し、実機で開けることを確かめる。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 2）
