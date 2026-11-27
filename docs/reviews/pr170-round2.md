# PR #170 レビュー（Opus, round 2）

対象: impl/xlsx-js-compat @ c7d93be6aba40ed55e0dec30c23deab32b5acacb

## 結論（先に）
round 1 の M1/m1/m2/n1 はすべて解消した。M1 は手書きの変異 3 種で、追加テストが落ちることを確かめた。round 1 で積み残した JS/wasm 側は、wasm を build して node で SheetJS/ExcelJS アダプタを約 30 件プローブした。no-op で出力がバイト一致すること、編集の局所性、拒否のあと出力が変わらないこと、失敗した load のあと前の状態が残ること、古い handle の拒否、範囲外への拡張の拒否を確かめ、Blocker/Major は見つからなかった。干渉 PR #169 は未マージだが、このブランチとは衝突なくマージでき、合わせた tree の ooxml-xlsx lib テスト 336 件が通った。converged: yes（ただし、レビュー時点で CI は全ジョブ pending。マージ前に全緑を確認すること）。

## 前ラウンドの解消状況
- M1: 解消。phase4.rs の 5 テストは、Err のとき try_on が assert_unchanged で出力のバイト一致を確かめる。変異と結果:
  - replaced_groups の `&= covered` → `|= covered`: partial_member_write_is_refused と single_cell_write_hands_off_to_next_member の 2 件が落ちる。
  - `or_insert(true)` → `or_insert(false)`: replaces_whole_group_without_handoff が落ちる。
  - `si.checked_add(1)` → `Some(si)`（si 割り当て）: writes_group_with_caches_and_next_si と replaces_whole_group の 2 件が落ちる。

  変異はすべて元に戻した。
- m1: 解消。ADR-0017 の note が実装と一致する。round 1 の P8/P19 で、範囲内の値セルを残したまま Ok を返すことを確かめている。
- m2: 解消。post-acceptance note の記述が、round 1 の P5/P13 の挙動と一致する。
- n1: 解消。コメントの記述は正確（journal/op.rs::plan_call はこの 3 つの op を作らない）。

## Findings
### n1（nit）: 「make build.sh executable」と書いたコミット bbc11f9 で、build.sh の mode が 100644 のまま
- 根拠: `git ls-tree HEAD packages/xlsx/build.sh` の結果が 100644。`./build.sh` は Permission denied で失敗する。README は `bash packages/xlsx/build.sh` と書いているので、手順としては動く。
- 修正案: `git update-index --chmod=+x` を当てるか、コミットメッセージの主張を取り下げる。

### 確認した JS/wasm の挙動（指摘なし。ADR-0021 の受入条件 3/4/5 との対応）
- SheetJS:
  - no-op の write は入力とバイト一致する。
  - A1 の編集では sheet1 の当該セルが変わり、依存する数式のキャッシュ（B1、A2:B2）が消える（ADR-0003）。sheet2 と sst は変わらない。
  - 範囲外 D9 への代入（拡張）、NaN、lone surrogate、セル削除、shared master と merge 領域への代入、日付書式の無いセルへの日付は拒否される。拒否のあとに直して書き直すと、ベースラインとバイト一致する。
  - `_x0041_ <&>` は往復で元に戻る。-0 は 0 に正規化される。セルの .w だけを変えた場合は no-op になる。
  - 単一セルの dimension（`A1`）は README のとおりプレースホルダとして扱い、データの範囲（A1:D4）で投影する。
- ExcelJS:
  - no-op はバイト一致する。setter の拒否は、編集前と編集後のどちらでも、保存結果を直前のバイトから変えない。
  - 失敗した load のあと、前の状態が残る。reload したあとの古い handle は拒否される。
  - shared formula の投影は参照版の形（formula/ref/shareType、sharedFormula）になる。
  - 数式と結果の書き込みで dimension が広がる。merge の従セルへの代入は master に書かれる（ExcelJS の参照挙動と同じ）。
- mutation モード（run-properties.mjs）は、候補を serialization の前に公開する変異を入れ、`serialization failure changed commit` で落ちることを確かめる。受入条件 5 の前半（failure isolation）に対応する。後半の「untouched data の欠落」は check-parts.py の局所性検査が受け持つ。受入条件 5 の文言を厳密に読めば、後半にも専用の変異があるとよい（follow-up）。
- 干渉:
  - #169（B1 fix、OPEN）: `git merge-tree` で衝突なし。合わせた tree で `cargo test -p ooxml-xlsx --lib` が 336 passed。どちらを先にマージしてもよい。
  - #144（pptx、Draft）: 衝突なし。触るファイルが重ならない。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 1）。マージ前に、c7d93be の CI（現在 pending）が全緑であることを merge-pr 手順で確認すること。Firefox と実ブラウザでのスイートは、今回のレビューでは再実行していない（node での adapter プローブのみ）。
