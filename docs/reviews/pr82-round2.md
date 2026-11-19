# PR #82 レビュー（Opus, round 2）

対象: ci/libreoffice-check @ 11d5f035196059fc26bcc610cf473091dd182fcd

## 結論（先に）
round 1 の M1 と m1〜m5 はすべて直っている。全ケースの期待値を元の fixture と機械的に突き合わせたところ、元のファイルでも満たされてしまうケースは、意図的に元の状態へ戻す `xlsx/26-autofilter-removed`（round 1 で了承済み）だけだった。
新しい Blocker / Major は無い。converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 1）。

## 前ラウンドの解消状況
- M1: 解消。xlsx/37 は `balance!A40 = "inline rich"` と、`x:is/x:r` が 2 個あることを xml で確認する。pptx/27・28・31 は `pages` 4・8・9（元は 2・7・10）。pptx/16 は LibreOffice で再保存したファイルを python-pptx で読み、`[hyperlink]` を確認する（`check.py:261-286`）。`docs/testing/libreoffice-check.md:23-29` に「編集を省くと落ちる期待値を最低 1 つ」の規則がある。PR 本文に全 116 ケースの表と、変異を入れると落ちることの記録がある。
- m1: 解消。`check.py:244-250` で、`overrides` にあるのにキャッシュが残っていればエラーにし、そのうえで上書き値とも比べる。
- m2: 解消。`tools/lo-check/Cargo.lock` をコミットし、`cargo build --locked` でビルドする。`requirements.txt` は推移的な依存まで sha256 で固定し、`pip install --require-hashes` で入れる（workflow と文書の両方）。
- m3: 解消。untagged の export は `UNTAGGED_WRITER_CASES = {10, 14a}` だけに掛かる（`check.py:32,320`）。ほかの docx は既定の export で CI を通っている。
- m4: 解消。pptx/32 は再保存したファイルのノートで `Bold red notes, edited` と `3rd paragraph` を確認する。docx/14b は python-docx 1.2 の `Document.comments` で `EDITED Comments text` を確認する。
- m5: 解消。`docs/testing/libreoffice-check.md:56-72` に、05 のピボット、25-p2 の B2 `02/27/1900`、キャッシュの無い数式の上書き値を持つ 11 ケースが列挙されている（manifest の `formulas` を持つケース 12/13/14/15/17/18/19/21/23/33/35 と一致する）。「LibreOffice で開ける ≠ Office が修復なしで開ける」も書かれている。
- n1・n2（nit、任意）: 未対応。follow-up でよい。

## 独自の確認
バンドルを `cargo xtask manual-check` で生成し、各ケースの期待値（`text`/`pdf_text`/`resaved_text`/`notes`/`comments` の文字列、`pages`、`xml` の件数、セル値、シート名と順序）を、CHECKLIST の Source の fixture、または `xlsx/originals/` と比べた（プローブはリポジトリ外。コミットしていない）。
- docx/pptx の fixture 由来 50 ケース: どれも、元のファイルでは満たされない期待値を 1 つ以上持つ。pptx/30 は、元の slide 2 にノートが無いので `notes."2"` で区別できる。
- xlsx: シート名・セル値では区別できないケースが 8 つある（14、15、24、25、26、27、28、34-p2）。これらは、次のどれかで区別できる。
  - xml の件数: 24/25/27/28/34-p2 は、元と出力で件数が違う（たとえば 24-unmerge は 6→5、27-protected は 0→1）。22/23/37/38 も xml の件数が元と違う。
  - 数式: 14-xlookup の `Sheet1!C2` は、元にはキャッシュ `Dianne Pugh` があり、出力では消えている。m1 の修正で、キャッシュが残れば落ちる。15 の `Label!C50` は元に存在しない数式セル。
  - 26-autofilter-removed は、元に戻す編集なので区別できない（PR 本文に明記済み。round 1 で了承済みなので再指摘しない）。
- synthetic の docx と、元のファイルが無い p2 系の xlsx は、生成した値そのものが編集結果。p2 の書式上限ケースは、xml で `xf[2]` の `fillId=299` などを確認している。

## Findings

### n1（nit）: 「Excel recalculation is checked separately」の参照先が書かれていない
- 根拠: `docs/testing/libreoffice-check.md:71`。自動の Excel 検査は無く、実際には manual-check の CHECKLIST（XL 7.2-x の項目）で owner が確かめる。
- 修正案: 「Excel recalculation is covered by the bundled manual checklists」と書き換える。

## その他の確認
- 順序の変更: (b) は (d) の再保存より後に動く。再保存に失敗しても、`resaved_text` などを持たないケースの (b) は従来どおり評価される。持つケースは「resaved document unavailable」で落ちる。どちらも正しい。
- `check_xml` は、入力が `preflight`（DTD の拒否）を通った後にだけ動く（`check.py:307`）。
- 干渉: #85（docx/29）に加えて、#86（`crates/ooxml-xlsx/examples/manual_check.rs` に触れる）も manual-check のケースを増やしうる。後からマージする PR は `expected.json` に項目を足す必要がある（足さないと coverage 検査で落ちる。これは正しい挙動）。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 1）
