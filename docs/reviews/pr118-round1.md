# PR #118 レビュー（Opus, round 1）

対象: fix/pptx-manual-check-fixtures @ f9a802df6de9cc7e93fa61f0b4b5ab56ed28807f

## 結論（先に）
17-M1 の修正自体は正しい: 生成した `17-M1-text-box-shape-styles.pptx` の master は `p:txStyles` に titleStyle / bodyStyle / otherStyle が 1 つずつ、この順で並び、整形式。
ただし 08 を消したのに `tools/lo-check/expected.json` の `pptx/08-modern-comment-shape-anchor.pptx` が残っていて、LibreOffice CI がマニフェスト一致チェックで落ちている。
converged: no（Major 1）。

## 前ラウンドの解消状況
（round 1 のため無し）

## Findings
### M1（Major）: `tools/lo-check/expected.json` に 08 のエントリが残り、LibreOffice CI が赤
- 根拠: `tools/lo-check/expected.json:117` `"pptx/08-modern-comment-shape-anchor.pptx": {"text": ["Hello, edited"]}`。`check.py:366` はバンドルのファイル集合とマニフェストのキー集合が完全一致することを要求する。この PR の CI run 44863829840（job `libreoffice`）は `ValueError: manifest coverage: missing=[], extra=['pptx/08-modern-comment-shape-anchor.pptx']` で失敗している。
- 修正案: expected.json から 08 の行を消す（同じコミットで）。

### m1（Minor）: CHECKLIST 見出しの「Phase 0 / 0.1 (1–13)」が 08 の欠番を示さない
- 根拠: `manual_check.rs` の見出し文字列は変えていない。末尾に「No 08」の注記はあるので実害は小さいが、見出しだけ見る owner には 08 が抜けた理由がすぐ分からない。
- 修正案: 見出しを「(1–7, 9–13)」にするか、そのままでよいと判断するならこのままでよい。

### n1（nit）: #102 と同じ箇所に触る
- 根拠: #102 も `crates/ooxml-pptx/examples/manual_check.rs` の CHECKLIST 見出し行と `tools/lo-check/expected.json` を変える。後からマージする側で衝突解消が要る（どちらも機械的）。

## 確認したこと（問題なし）
- `sample_with` で master の `p:txStyles` を触るのは 17 だけ。他の M ケース（18 slide の `p:ph`、19/21/22 layout）は対象パートに txStyles が無いので重複チェックで誤って落ちない。バンドル生成（`cargo run -p ooxml-pptx --example pptx_manual_check`）は 38 ファイルを正常に書き出す。
- 重複チェックの `<p:titleStyle` / `<p:bodyStyle` / `<p:otherStyle` は前方一致だが、PML にこれらを前方部分に持つ別の要素名は無く、誤検知しない。`p` 以外のプレフィックスを使う master は見逃すが、sample.pptx の固定入力に対する安全柵としては十分。
- 置換元を `</p:bodyStyle><p:otherStyle/>` にしたため、sample.pptx が変わって空の otherStyle が無くなった場合は `not found` の `Err` で止まる（黙って二重にしない）。
- 19-M3 の layout transition は `p:clrMapOvr` の後・`</p:sldLayout>` の直前で、CT_SlideLayout の順序（cSld, clrMapOvr, transition, timing, hf, extLst）に合う。
- `modern-comments.pptx` を使うテスト（deckread / structure / refusals / replace / oracle cases）は fixture 自体を消していないので影響なし。xtask は個別のファイル名を参照しない。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 1 / nit 1）
