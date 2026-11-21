# PR #121 レビュー（Opus, round 1）

対象: impl/xlsx-phase5c-comments @ 0d49444d2cd4b841e420e87634cd6cda4a54b0ee

## 結論（先に）
ADR-0017 §2.1・§2.2・§4・§5 の規則を壊しにいったが、壊れたファイル、ぶら下がり参照、`Ok` での黙った誤りは見つからなかった。
拒否はすべて全バイト不変で、VML ブロックと shape 番号は不変条件どおりだった。
残るのはテストの穴と安全側の過剰拒否で、どれも Minor 以下。
**converged: yes**（Blocker 0 / Major 0 / Minor 3 / nit 2）。Minor は follow-up でよい。

## 前ラウンドの解消状況
- round 1 なので無し。

## 確かめたこと（プローブ。コミットしていない）
- 2 スレッド（B2・C3）で B2 に返信を 2 回入れた（1 回は開き直した後）。返信は B2 スレッドの末尾、C3 の前に入る。`parentId` はルート。
  fallback 本文は `[Threaded comment]\n\n` + 各本文の空行区切りで、`&` `<` `"`・CR（`&#13;`）・`_x0041_`（`_x005F_x0041_`）が往復した。
  人物名 `A&<"` は属性で正しくエスケープされた。
- 接頭辞付きの comments ルート（`<x:comments xmlns:x=…>`）: 追加と `set_note_text` の結果が正しい名前空間で読める。
- スレッド削除で空になった `<ThreadedComments/>`（自己終了タグに書き換え）への再追加: 正しく展開され、新しい GUID と shape 番号が振られる。
- 端のセル（XFD1048576 と A1）: `x:Anchor` は範囲内（`16380, 49, 1048571, 14, 16382, 49, 1048575, 8` と `1, 15, 0, 2, 3, 15, 3, 16`）。`x:Row`/`x:Column` は 0 始まり。
- `legacyDrawing` が `legacyDrawingHF` に変わったシート: 新しい VML に、ワークブックで未使用のブロック（`data="2"`、番号 2049）が振られる。`legacyDrawing` は `legacyDrawingHF` の前に入る。
- 2 シートで VML とスレッドのパートを共有させた場合: `add_note`（両シート）、`remove_comment_thread`、`reply` はどれも `PartNotEditable`（"part ownership is ambiguous"）で、全バイト不変。
- 生成物: PR 自身の差分（`origin/main...HEAD`）で既存の生成済み公開名の削除・改名は無い（`Selection` は位置が動いただけ）。
- `origin/main` とはそのままマージできる（`git merge-tree` で衝突なし）。open PR #122（core の leaf text）とはファイルが重ならない。
- CI 5 ジョブは全緑（test/clippy/codegen、python-pptx・python-docx・openpyxl の読み戻し、LibreOffice）。

## Findings

### m1（Minor）: VML・スレッド・persons パートの所有権拒否にテストが無い
- 根拠: `crates/ooxml-xlsx/src/comments/mod.rs` L208（スレッド削除）、L251（既存 VML の staging）、L257（VML shape の削除）の
  `xml::require_owner` を 3 つとも外す変異を入れても、`cargo test -p ooxml-xlsx --lib --all-features comments` の 13 件はすべて通った。
  `ambiguous_part_links_and_insertion_slots_are_atomic` が共有させているのは comments パートだけ。`threads::append`（L298）と
  `Workbook::person`（threads.rs L160）の所有権検査も同じ状態と思われる。挙動はプローブで正しいと確認した（上の「確かめたこと」）。
  チェックが外れると、共有 VML に shape が増えて他のシートのメモが壊れる。AGENTS.md の「新しい拒否には必ず Err ⇒ byte-identical のテスト」にも当たる。
- 修正案: 既存の ambiguous テストに、VML の共有（`add_note` と `remove_note`）、スレッドの共有（`reply` と `remove_comment_thread`）、
  persons の共有（workbook 以外からの関係を足して `person`）の 3 ケースを足し、`rejected` で全バイト不変を確かめる。

### m2（Minor）: スレッドを失った fallback note を消せない
- 根拠: `add_comment` 後に threadedComment の要素だけを取り除いた入力（作者が `tc={id}` で `xr:uid` も残る）では、
  `remove_note` と `set_note_text` が `ThreadedComment`、`remove_comment_thread` が `NoSuchNote` になる。
  このメモは API で編集も削除もできない（`mod.rs` L111・L118 の `is_fallback` 判定）。ADR §4.1 が拒否を求めるのは「スレッドのあるセル」だけ。安全側なので Minor。
- 修正案: セルにスレッドの要素が無いときは `is_fallback` で拒否せず通常のメモとして扱う。または、この挙動を ADR の P5c の注記に書く。

### m3（Minor）: threadedComment の `text` に ST_Xstring エンコードを適用している
- 根拠: `threads.rs` L108 で `escape::encode` をかけているので、本文 `_x0041_` は `_x005F_x0041_` で書かれる（プローブで確認）。
  MS-XLSX の `CT_ThreadedComment/text` は `xsd:string` で、Excel がここで `_xHHHH_` を復号するかは確かめていない。
  復号しなければ、`_xHHHH_` の形や制御文字を含む本文だけが Excel のスレッド表示で化ける。fallback note 側（ST_Xstring）は正しい。
- 修正案: Excel の確認項目（XL）に「スレッド本文の `_x005F_` と制御文字の表示」を加える。結果に合わせて、エンコードを残すか、XML で書けない文字だけを拒否するかを決める。

### n1（nit）: 挿入要素ごとに既定名前空間を宣言し直している
- 根拠: `<author xmlns="…main">`、`<comment xmlns="…main">` の中の `<text xmlns="…main">`、`<threadedComment xmlns=…>`、`<person xmlns=…>`。
  XML として正しく、LibreOffice も openpyxl も読めるが、親と同じ既定名前空間なら宣言は要らない。
- 修正案: 挿入先で既定名前空間が同じ URI に束縛されているときは `xmlns` を省く（任意）。

### n2（nit）: GUID の大文字小文字の扱いが揃っていない
- 根拠: `notes::is_fallback` は `tc=` の後と `xr:uid` を大文字小文字を区別して比べるが、`reply` と `remove_comment_thread` の照合（`mod.rs` L145）は `eq_ignore_ascii_case` を使う。
  小文字の GUID を書く producer のファイルでは、判定が経路によって変わり得る。
- 修正案: `is_fallback` も `eq_ignore_ascii_case` で比べる。

## 追補: 独立レビュー 2（同じ head 0d49444d2cd4b841e420e87634cd6cda4a54b0ee）

上の round 1 と並行して、別のクローンで独立に壊しにいった結果。上の結論（converged: yes）は変わらない。下の判定の件数は追補を含む。

### 確かめたこと（プローブ。コミットしていない）
- POI コーパス（`47dcff76`、xlsx/xlsm 366 件）で `poi_comments_sweep` を再実行: `334 checked, 32 unreadable, 0 without a worksheet; 331 note successes, 3 refusals; 331 thread successes, 3 refusals`。PR 本文の数値と一致。明示ケース 6 件は 2 insertions、`BrNotClosed.xlsx` は両方 `PartNotEditable`。
- `64759.xlsx`（Excel のスレッド）: C2 への `set_note_text`・`remove_note` は `ThreadedComment` で全バイト不変。`reply` は VML を 1 バイトも変えず、返信を既存エントリの直後に `parentId` 付きで入れ、fallback は `[Threaded comment]\n\ntesting\n\n\nreply` に置き換わる。`xr` はルートで宣言済み・ignorable なのでローカル宣言は付かない。B3（`xr:uid` 付きだが作者が `tc=` でない普通のメモ）の `set_note_text` は通る。`remove_comment_thread` は shape `_x0000_s1030` だけを消し、B3 の `_x0000_s1031` は残る。同じセルへの再 `add_comment` も正しい。
- フォームコントロールのある VML: `45540_form_Header.xlsx`（41 shape）、`ExcelWithAttachments.xlsm`（8 shape）、`mv-calculator-final-2-20-2013.xlsm`（チェックボックス 163 個は別 VML）で add → remove すると、増えた 1 shape だけが消え、他の shape 数は元どおり。全パートが XML として読め、ぶら下がる関係は無い。
- 最後のメモとスレッドを消した後: comments は `<commentList></commentList>`、VML は shapelayout と shapetype だけ、`legacyDrawing` と関係は残り、ぶら下がりは無い（ADR §4.2 の「空でも残す」どおり）。
- `legacyDrawing` の挿入位置の後ろに `mc:AlternateContent`（`controls`）があるシート: `add_note` は `UnresolvedSibling` で全バイト不変。
- 結合セル `A1:C3` の B2（左上でないセル）へのメモは `Ok`。ADR は拒否を求めていないので指摘にしない。
- `origin/main`（#99・#102 マージ後、`25bfff8`）との試しマージ: `codegen/report.txt` と `tools/lo-check/expected.json` は自動マージでき、マージ結果で `cargo xtask codegen --check` が通る（9 generated files up to date）。expected.json のキーは `xlsx/40`・`xlsx/41` で、main が足した `docx/33`〜`36`・`pptx/33` や open PR #123 の `pptx/08` と重ならない。
- 上の m3 の訂正: `reference/openxml-sdk-data/schemas/schemas_microsoft_com_office_spreadsheetml_2018_threadedcomments.json` では `xltc:threadedComment` の子 `text` は `x:ST_Xstring/xltc:text` で、`xsd:string` ではない。`escape::encode` はスキーマどおりで、m3 は Excel での表示確認（XL）だけが残る。

### m4（Minor）: 新しい VML パートのルート `<xml>` に `v`・`o`・`x` の宣言が無い
- 根拠: `comments/vml.rs` L337 は `<xml>{prefix}</xml>` を書き、名前空間は `o:shapelayout`・`v:shapetype`・`v:shape` の要素ごとに宣言される（プローブ出力: `<xml><o:shapelayout xmlns:o=… xmlns:v=…>…<v:shape xmlns:v=… xmlns:o=… xmlns:x=…>`）。
  XML としては正しく、LibreOffice と openpyxl は読める。ただ Excel の VML 読み込みは通常の XML パーサではないことで知られ、Excel・RX・openpyxl・POI はどれもルートで宣言する。Excel での確認（XL）は未実施。
- 修正案: 新しい VML パートは `<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel">` で書く。新しいパートなので局所性の制約は無い。XL 項目に「ルート宣言なしの VML を Excel が開くか」も加える。

### m5（Minor）: コーパススイープの「スレッド」の数え方と範囲が本文の説明より狭い
- 根拠: `comments/tests.rs` の `sweep_file` は、スレッドの 3 拒否のうち 2 件（`invalid_cell_reference.xlsx`、`poc-shared-strings.xlsx`）を `Workbook::person` の拒否（`person refused`）として数えている。`add_comment` まで届いた拒否は `BrNotClosed.xlsx` の 1 件だけ。
  また、スレッドは同じシートにメモを入れた直後に足すので、コーパス上では「VML・comments パートが無いシートへのスレッド追加」の経路を通らない。`reply` と `remove_comment_thread` はコーパスで回していない。
- 修正案: PR 本文の内訳に「person で拒否 2・add_comment で拒否 1」と書く。スイープで、メモを入れる前の Workbook のコピーにもスレッドを足す（新規パートの経路）か、追加したスレッドに `reply` と `remove_comment_thread` を続けて (1)(2) を確かめる。

### n3（nit）: スレッドのあるセルへの `add_note` は `ThreadedComment` でなく `NoteExists`
- 根拠: `mod.rs` L101-106。fallback note があるので `notes.append` が `NoteExists` を返す（`note_thread_refusals_preserve_the_archive` がそう固定している）。ADR §4.1 の「スレッドのあるセルはメモの編集と削除を `ThreadedComment` で拒否」とは別の規則に当たるので誤りではない。
- 修正案: ADR の P5c 注記に、この組み合わせのエラーを 1 行書く。

### n4（nit）: 新しい shape がどれも `z-index:1`
- 根拠: `vml.rs` L191。Excel と RX は shape ごとに z-index を増やす（`64759.xlsx` は 1、2）。表示は重なり順だけの差。
- 修正案: 既存 shape の最大 z-index + 1 を使う（任意）。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 5 / nit 4。追補の m4・m5・n3・n4 を含む）
