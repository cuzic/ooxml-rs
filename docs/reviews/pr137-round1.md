# PR #137 レビュー（Opus, round 1）

対象: impl/docx-phase3e-fields @ 2a0256e77b4cd95ee80467ecc35bdbd2c65573d7

## 結論（先に）
複合フィールドの形（begin → `instrText xml:space="preserve"` → separate → 結果 → end）、命令文の組み立てと拒否、
フィールド内・変更履歴内・ラッパー内への挿入拒否、`Err ⇒ byte-identical`、T2 局所性はプローブで確認でき、壊れたファイルは作れなかった。
ただし、書式はフィールドの 5 run のうち結果 run にだけ複製されている。Word は `\* MERGEFORMAT` のないフィールドを更新すると、
結果に命令文の先頭文字の書式を当てるので、複製した書式は最初の更新で消える（M1）。
converged: **no**（Major 1）。

## 前ラウンドの解消状況
round 1 のため前ラウンドはない。同梱した PR #131 round 2 の nit は次のとおり。
- n1（全角の日付 token）: 解消。ADR-0015 の D3d note に「`ｙｙｙｙ` などの全角英字と `年` `月` `日` はリテラル」と明記された。
- n2（`#[path]` による test helper の取り込み）: 解消。`controls/wrap.rs` は `crate::test_package::docx` を使い、`#[path]` はなくなった。

## 確認できたこと（プローブ、コミットしない）
- 既存の複合フィールドの begin・instrText・separate・結果の各 run の後ろへの挿入は `InsideField` を返し、出力は無変更。end run の後ろへの挿入は Ok。
  begin が前の段落にあり、end が後ろの段落にある複数段落フィールドの途中の段落も `InsideField` になる。
- `w:hyperlink`、run レベルの `w:sdt`、`w:fldSimple` の中の run を指す `After` は `UnsupportedContext`。`rPrChange` を持つ run を指す `After` と、`w:ins` の中の run を指す `After` は `InsideRevision`。いずれも出力は無変更。
- `EndOf` は、段落の最後の要素が `w:ins`、`w:del`、`w:moveTo`、`w:smartTag`、run レベルの sdt、`fldSimple` のときも、
  その後ろに直接の run として挿入し、書式は複製しない。再オープンでき、分類も変わらない。
- 表のセル、ヘッダー（`w:sz` の複製を含む）、空白で整形された XML（`After` は直後の run の前に、`EndOf` は末尾の空白ノードの後ろに挿入）で、位置は正しかった。
  テキストボックス内の段落は `paragraphs()` に現れないので、指定する経路がない。
- 命令文: `DATE \@ "yyyy &amp; &lt;MM&gt;"` は正しくエスケープされる。`"`、`\`、全角・タイポグラフィの引用符は `InvalidValue`。
  `'de'` のようなリテラルは通る（月名は英語。note に記載がある）。
- `updateFields`: 既存の `w:updateFields` を再利用し、二重に作らない（`enable_update_fields_on_open`）。セッションの opt-in がないと、
  内容依存の 4 種は `StaleFieldRisk` になる（入力の settings に `updateFields` があっても同じ）。
- id: 新しい id は作らない（REF/PAGEREF は既存のブックマークを要求し、無ければ `UnknownBookmark`）。
- `mc:AlternateContent` を含む `rPr` の run を指す `After` は `NotDetachable` で拒否され、出力は無変更（安全側）。
- 他の作業との干渉: open PR は #125（xlsx）だけで、触るファイルは重ならない。D3d との共有は `controls::date::format` の公開範囲だけ。
  D3d のバインディング編集（`binding.rs` の `write`）は、その時点のストーリーを走査するので、同じセッションで挿入した `DOCPROPERTY Title` も検出する。
  D3c とは、W1 ゲートと、変更履歴内への挿入の拒否で分かれている。
- CI 5 ジョブはすべて pass。`cargo test -p ooxml-docx --test phase3e` と lib の `insert_field` テストも手元で通過。

## Findings

### M1（Major）: 書式を結果 run にしか複製しないので、Word がフィールドを更新すると書式が消える
- 根拠: `crates/ooxml-docx/src/insert_field.rs:212-232`。`properties` は `result` run（index + 3）にだけ挿入され、
  begin、instrText、separate、end の run は `rPr` を持たない。プローブでは、ヘッダーの `<w:sz w:val='18'/>` の run の後ろに PAGE を挿入すると、
  `<w:r><w:rPr><w:sz w:val="18"/></w:rPr><w:t>1</w:t></w:r>` になり、他の 4 run には `rPr` がなかった。
  命令文に `\* MERGEFORMAT` はない。Word の書式スイッチの説明では、`\* MERGEFORMAT` がなければ、更新した結果は前の結果の書式を引き継がず、
  フィールドコードの書式で表示される（`\* CHARFORMAT` は先頭文字の書式）。そのうえ、この経路で作るフィールドは更新される前提である。
  REF、PAGEREF、SEQ、TOC は `updateFields` の opt-in が必須で、Word は開くときに更新する。DATE/TIME/PAGE/NUMPAGES はレイアウトや開くときに再計算される。
  そのため、PR 本文と ADR note の「結果 run は直前 run の書式を複製」は、Word では最初の更新で無効になる。
  具体例として、太字や 9pt のヘッダーに入れたページ番号は、本文の既定書式に戻る。手元に Word がないので、この挙動は文書で確認したもので、実機では未確認 *(u)*。
  Word 自身は、挿入位置に書式があると 5 run すべてに同じ `rPr` を書く。
- 修正案: 同じ `rPr` を 5 run すべてに複製する（`\* MERGEFORMAT` を付けると W2 の分類と命令文の固定集合に影響するので、付けない方がよい）。
  あわせて、全 run に `rPr` があることを確かめるテストと、それを壊す変異（たとえば instrText run の複製を外す）を加え、ADR の D3e note の 1 項目目を直す。

### m1（Minor）: 自分を含むブックマークへの REF を Ok で作る
- 根拠: `<w:bookmarkStart w:name='bm'/><w:r><w:t>Self</w:t></w:r><w:bookmarkEnd/>` の run の後ろに `Ref { bookmark: "bm" }` を挿入すると Ok になり、
  REF はブックマークの範囲の中に入る（キャッシュは `Self`）。`updateFields` が必須なので、Word は開くときに更新し、
  自己参照のエラー表示（"Error! Not a valid bookmark self-reference."）になると見込まれる *(u)*。PAGEREF は自己参照でも問題ない。
- 修正案: `Ref` の挿入位置が対象ブックマークの範囲内なら拒否し、無変更を確かめるテストを付ける。拒否しない場合は ADR note に記載する。

### m2（Minor）: 日付型の DOCPROPERTY のキャッシュが W3CDTF の生の文字列になる
- 根拠: `insert_field.rs:358-401` の `builtin_field_property` は `value.text()` をそのまま返す。`CREATETIME`（`dcterms:created`）、`LASTSAVEDTIME`、`LASTPRINTED`、
  および `custom_field_property` が返す `vt:filetime` のカスタムプロパティは、`2024-01-01T00:00:00Z` のような文字列がキャッシュになる。
  これらは W2 で安全なフィールドなので dirty が付かず、opt-in も不要である。そのため、Word で手動更新するまで、この文字列が表示され続ける。
  ADR の「current value」の字面には合っているが、Word の表示とは違う。
- 修正案: 日付型は共有の整形器で Word の既定の書式（`M/d/yyyy h:mm:ss am/pm` 相当）に整えるか、生の値を使うことを ADR note と doc コメントに書く。

### m3（Minor）: SEQ の識別子の構文を検査していない
- 根拠: `Seq { identifier: "My Fig" }` は Ok になり、命令文は ` SEQ "My Fig" ` になる。`quote` が検査するのは、空、引用符、`\`、制御文字だけである。
  Word の SEQ の識別子は、文字で始まり、英数字と `_` からなる名前とされる *(u)*。そうでない識別子は Word での更新結果が不定で、
  ADR-0004 のスキャナは正規化した `MY FIG` で数えるので、Word と数え方がずれうる。
- 修正案: 識別子を英字で始まる `[A-Za-z0-9_]`（長さ上限あり）に制限して `InvalidValue` で拒否するか、許す範囲を ADR note に書く。

### n1（nit）: SEQ のキャッシュは `\r`、`\c`、`\h` などのスイッチを考慮しない
- 既存の `SEQ Figure \r 5` の後ろでも、件数 + 1 になる。dirty と `updateFields` で更新されるので実害はない。note に「近似」と一言書く程度でよい。

### n2（nit）: コーパス走査の 84 件の拒否の内訳が PR 本文にない
- `sdk_field_insertion_sweep` はエラー別の件数を出力しているので、その内訳を PR 本文に貼る（過剰拒否がないかを後から追えるように）。

### n3（nit）: block レベルの `w:sdt` や `w:customXml` の中の段落は `UnsupportedContext` になる
- `para_location` の既存の規則（脚注・画像と同じ）によるもので、挙動は安全側。ただ、D3e の note と `insert_field` の doc コメントには書かれていない。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 3 / nit 3）
