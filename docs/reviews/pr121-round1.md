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

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 3 / nit 2）
