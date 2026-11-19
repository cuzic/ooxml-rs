# PR #82 レビュー（Opus, round 1）

対象: ci/libreoffice-check @ 58fee22477b639e162332249f22aa44344ce21bb

## 結論（先に）
仕組みはおおむね正しく失敗する。soffice の終了コードと読み込みエラー文言を見て、出力が空なら落とす。CSV に無いセルは `""` として扱うので、空でない期待値とは一致しない。数値の比較は相対 `1e-12` と厳しい。マニフェストは出力から生成しない静的な JSON で、カバレッジは `files == manifest` で両方向に確かめている（`check.py:294`）。
ただし、一部のケースでは (b) の期待値が編集前の fixture にもある文字列やセルしか見ていないので、編集が失われても通る（M1）。
converged: no（Major 1）。

## 前ラウンドの解消状況
round 1 のため無し。

## Findings

### M1（Major）: 編集を観測しない (b) 期待値があり、編集が失われても通る
- 根拠: バンドルを手元で生成し、期待値を `xlsx/originals/` と CHECKLIST の Source に書かれた fixture と突き合わせた。
  - `tools/lo-check/expected.json:204` `xlsx/37-p2-rich-strings-inline.xlsx` は `balance!B11 = ""`, `R11 = ""` だけを見ている（07 のコピーとみられる）。編集は A40 のインライン rich string（`inline ` + `rich`）で、A40 は検査されていない。B11 は元のファイルでも空の `inlineStr` で、R11 はセル自体が無い。
  - `:45` `pptx/16-replace-in-hyperlink.pptx` の `"Subject is here"` は元の `poi/with_japanese.pptx` にある文字列。編集（`hyperlink` → `[hyperlink]`）は検査されない。PR 本文によると、PDF でクリップされるので外したとのこと。
  - `:56` / `:57` / `:60` の 27・28・31 は、スライドの追加・複製・削除そのものが編集だが、needle は元からある文字列（`Lorem ipsum dolor sit amet`、`Triple Set Numbers`、`Centered Title`）だけ。枚数は 2→4、7→8、10→9 と変わるのに、既にある `pages` を使っていない。
- 修正案: 37 は `balance!A40 = "inline rich"` にする。27/28/31 には `"pages": 4 / 8 / 9` を足す。16 は PDF に頼らず、(d) で再保存したファイルを python-pptx で読んで `[hyperlink]` を確かめる。たとえば `resaved_text` のような期待値を追加する。今後のケースのために、`docs/testing/libreoffice-check.md:21-32` に「元の fixture に無い値（編集結果）を最低 1 つ入れる」と書く。

### m1（Minor）: セルにキャッシュがあると `formulas` の上書きが黙って無視される
- 根拠: `tools/lo-check/check.py:219-223`。キャッシュがある数式セルでは `overrides` を読まないが、`seen` には入る。このため `:229` の「余分なキー」検査も通る。キャッシュを消すはずの編集が消さなかった場合、マニフェストの値は使われず、そのことも報告されない。
- 修正案: `cached is not None and key in overrides` をエラーにする（または両方と比較する）。

### m2（Minor）: 依存の固定が o3 より弱い
- 根拠: `.github/workflows/libreoffice-check.yml:63` の `cargo build` は `--locked` 無しで、`tools/lo-check/` に `Cargo.lock` も無い。calamine は `=0.38.0` に固定されているが、推移的な依存（build.rs を含む）は実行のたびに解決される。o3-recalc.yml は Cargo.lock の SHA-256 を記録しているが、こちらは記録していない。`tools/lo-check/requirements.txt` はバージョンを固定しているがハッシュが無い。なお、権限は `contents: read` で、`persist-credentials: false` も付いているので、影響範囲は小さい。
- 修正案: `fuzz/Cargo.lock` と同じように `tools/lo-check/Cargo.lock` をコミットして `--locked` でビルドする。pip は `--require-hashes` にする（`pip-compile --generate-hashes`）。

### m3（Minor）: Writer の untagged PDF 回避策が全 docx に無期限で掛かる
- 判断: この回避策は本物の失敗を隠していない。読み込み失敗（終了コード・`LOAD_ERROR`）、レイアウト、全ページの描画は untagged でも通る経路で、外れるのは tagged PDF の構造ツリー生成だけ。ただし、`check.py:39-41,265` は全 docx に一律で掛かっていて、LibreOffice を上げて不具合が直っても誰も気づかない。
- 修正案: 既定の export を「未編集の footnote fixture では今も失敗する」というカナリアとして 1 回だけ走らせ、成功したら回避策の削除を促して落とす。または回避策を 10 と 14a だけに限る。

### m4（Minor）: notes・コメントの編集が未検査（文書では限界として書かれているが、安く検査できる）
- 根拠: `expected.json:61` の `pptx/32-M4-notes-edit` は slide 本文の `Lorem ipsum…` だけを見ている。`:16` の `docx/14b-comment` も本文だけを見ている。
- 修正案: 32 は再保存した pptx の `notes_slide` を python-pptx で読み、`Bold red notes, edited` と `3rd paragraph` を確かめる。14b は python-docx の comments part を読むか、Writer の `ExportNotes` を使う。

### m5（Minor）: docs/testing に「Calc の結果を期待値にした箇所」が書かれていない
- 根拠: `docs/testing/libreoffice-check.md:26-32,50-58` には、次の 3 点がどれも無い。
  - `xlsx/25-p2-dates-1900` の B2 `02/27/1900` は、Excel（02/28）とは違う Calc の値を固定している。
  - 05 のピボットは Calc が再計算する。
  - キャッシュの無い数式の上書き値（13・14・33 など）は、Excel との一致ではなく、固定した Calc での回帰検査にすぎない。
  
  「LibreOffice が開ける ≠ Office が修復なしで開ける」という一文も無い（修復ダイアログについては書かれている）。
- 修正案: 「分からないこと」の節にこれらを列挙する。

### n1（nit）: タイムアウト時に soffice.bin が残る
- `check.py:86` の `subprocess.run(timeout=90)` は直接の子プロセスしか kill しないので、`soffice.bin` が孤児として残りうる。`start_new_session=True` と `os.killpg` で、プロセスグループごと止める。

### n2（nit）: 未知の拡張子が黙って対象外になる
- `check.py:289-293` は `.xlsx/.xlsm/.docx/.pptx` だけを glob する。将来 `.docm/.pptm/.xltx` を出すケースが増えると、黙って対象外になる。`xlsx/ docx/ pptx/` 直下の `.md` 以外の未知ファイルはエラーにする。

## その他の確認
- 指示された範囲に収まっている: ライブラリの examples は変更なし。xtask の変更はマニフェストのコピーだけ。
- 実行時間は 4 分 18 秒で、`timeout-minutes: 15` は妥当。
- LibreOffice の版と SHA-256 は o3-recalc.yml と同じ。分割ダウンロードは、各範囲を `--max-filesize` で制限し、`wait` の失敗で止まり、連結したファイルを SHA-256 で検証している。actions は SHA で固定されている。
- PR #85 が `docx/29-bookmark-list-style.docx` を追加する。後からマージする方がマニフェストに項目を足す必要がある（足さないと `check.py:294` で落ちるのは正しい挙動）。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 5 / nit 2）
