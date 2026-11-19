# PR #87 レビュー（Opus, round 1）

対象: impl/pptx-phase3a-text-format @ dd00a574f9ee568051911e7489273c31e4462a79

## 結論（先に）
converged: no。範囲書式が、範囲の内側に丸ごと入る `a:fld`（と `a:br`）の `a:rPr` に書かず、`Ok` のまま選択の一部が無書式で残る（M1）。
それ以外の §1 原則（直接要素だけに書く、clear で継承に戻る、同値は無変更、分割で文字位置・段落番号・`a:rPr` の子（`hlinkClick`、許可リストの `a:ext`）を保つ、ロック下で許可、拒否は byte-identical、検証は最初の分割の前）はプローブで成り立った。
生成コードの公開名は main から 1 つも消えておらず（`dml.rs` の公開シグネチャ行を rustfmt 後に比較、削除 0）、CI は 2 ジョブとも pass。

## 前ラウンドの解消状況
round 1 のため無し。

## Findings

### M1（Major）: 範囲の内側にある `a:fld` / `a:br` に書式が書かれない
- 根拠: `crates/ooxml-pptx/src/write/formatting/range.rs:67` は `item.is(Run::QNAME)` のときだけ編集を計画する。
  プローブで `<a:r><a:t>ab</a:t></a:r><a:fld type="slidenum"><a:rPr lang="en-US"/><a:t>12</a:t></a:fld><a:r><a:t>cd</a:t></a:r>` に対して
  `format_range(1, 5, bold)` を呼ぶと `Ok` になり、`b` と `c` は `b="1"` になるが、範囲内のフィールド `12` の `a:rPr` には何も書かれない（`<a:rPr lang="en-US"/>` のまま）。
  範囲内の `a:br` の `a:rPr`（改行の行の高さを決める）も同じ。
  ADR-0016 §3 では、フィールドの文字は範囲の数え方に入り（`RangeSplitsField` はフィールドの途中で始まるか終わる範囲だけを拒否する）、範囲の内側の「every run」に書くと定めている。DrawingML では `a:fld` も `EG_TextRun` の一員。
  PowerPoint で同じ範囲を選んで太字にした結果と食い違い、ページ番号などが黙って書式から外れる。
- 修正案: 範囲に丸ごと入る `a:fld` と `a:br` を、分割せずにそれぞれの直接の `a:rPr` に `RunFormat::apply` と同じ書き込みで書く（`a:fld` の `a:rPr` は先頭の slot なので `get_or_insert` で入る。同値なら無変更）。
  加えて、フィールドを挟む範囲で `Source::Direct` になることと、フィールド本文・`id`・`type` が byte 単位で変わらないことのテストを足す。
  意図的に除外するなら、ADR の post-acceptance note とメソッドの doc に「フィールドと改行は書式化しない」と書く（ただし推奨しない）。

### m1（Minor）: プロパティ要素の中の `mc:AlternateContent` を見ていない
- 根拠: `para_shape` / `body_paragraphs`（`crates/ooxml-pptx/src/write.rs:228`, `:257`）は `a:rPr` / `a:pPr` / `a:bodyPr` の中を検査しない。
  プローブでは、`a:rPr` の中にある `mc:AlternateContent`（Choice に `a:solidFill`）に対して `color()` を呼ぶと、拒否は byte-identical だが、エラーは `Core(UnresolvedSibling)` で、§11 の `InsideAlternateContent` ではない。`a:pPr` に対する `bullet` / `space_before`、`a:bodyPr` に対する `autofit` も同じ。
  一方、`a:bodyPr` の AC の Choice に `a:normAutofit` があるときの `clear_autofit()` は `Ok` で、何も消えない。この場合 AC の中の直接値が残るので、§1 原則 2（clear で継承に戻る）が守られない。`clear_color` / `clear_bullet` なども同じ形になる。
  同梱の fixture の slide と notes には該当が 0 件なので、Minor とした。
- 修正案: 書き込みと clear の検証で、対象のプロパティ要素が `mc:AlternateContent` を子に持つときは `InsideAlternateContent` を返す。属性だけを書く setter を除外するかは決めて ADR の note に書く。テストを 1 つ足す。

### m2（Minor）: `UnderlineLine::matches` が余分な子を無視するので、古い値が残る
- 根拠: `crates/ooxml-pptx/src/write/formatting/underline.rs:78-110` は `w`/`cap`/`cmpd`/`prstDash`/fill/head/tail だけを比べ、線の接合（`a:round`/`a:bevel`/`a:miter`）、`a:custDash`、`algn`、`a:extLst` を見ない。
  プローブでは、`<a:uLn><a:round/></a:uLn>` に `underline_line(UnderlineLine::Line(LineSpec::default()))` を呼ぶと、同値と判定されて `a:round` が残る（`Ok`、bytes 不変）。
- 修正案: 子要素の数（と属性の集合）が `LineSpec` から書く形と一致することも同値の条件に入れる。

### m3（Minor）: 書体を差し替えたときの `panose`/`pitchFamily`/`charset` の削除がテストされていない
- 根拠: `crates/ooxml-pptx/src/write/formatting.rs:207-209`。`view.remove_pitch_family()?` を消す変異を入れても `tests/formatting.rs` は 10/10 pass する（手元で確認）。
  §2 の規則（古い書体の記述を残さない）を守っているのは、今はプローブだけ（`<a:sym typeface="Wingdings" pitchFamily="2" charset="2"/>` が `font(Typeface("Symbol"), Symbol)` で `<a:sym typeface="Symbol"/>` になる）。
- 修正案: 既存の `a:sym` / `a:latin` を差し替え、3 つの属性が消えることを assert するテストを足す。

### m4（Minor）: §13 の provenance オラクルの網羅が狭い
- 根拠: `Source::Direct` と、clear 後に継承元へ戻ることを確かめているのは、`bold`、`italic`（`end_format`）、`align` だけ（`tests/formatting.rs:109`, `:443`）。
  fill、font、highlight、`underline_line`/`underline_fill`、spacing、bullet の 3 つの外観グループ、tab、`font_align` については、resolver に追加した lookup（`resolve.rs` の `highlight`/`underline_*`/`tab_stops`/`bullet_*`/`font_align`）も含めてテストが無い。
- 修正案: setter ごとに write → `Source::Direct`、clear → 元の source を確かめる表形式のテストを 1 つ足す。

### m5（Minor）: setter を 1 回呼ぶたびに part の木全体を clone する
- 根拠: `RunMut::format`（`formatting.rs:448`）は、変更があるかどうかを判定する前に `self.doc.clone()` を呼ぶ。同値の書き込みでも slide の木全体を複製する。
  `end_format`（`:464`）、`autofit`（`autofit.rs:22`）、`format_range`（`range.rs:20`）も同じ。大きな slide で run の setter を多数呼ぶと、計算量が呼び出し回数 × part の大きさになる。
- 修正案: 先に `format.changes(...)` を元の木で評価し、変更がある場合だけ clone する（`update_properties` と同じ順にする）。挿入が失敗しない経路では、原則 6 の degenerate case として in-place で編集してもよい。

### n1（nit）: 空の範囲を拒否している
- `format_range(k, k)` で `k` がフィールドの内側にあると `RangeSplitsField` になる（`range.rs:62-66`）。空の範囲は何も変えないので、`start == end` は先に `Ok(())` にしてよい。

### n2（nit）: doc コメントが全ての run setter に付いている
- `run_attributes!` の doc「Text size and spacing changes leave cached autofit …」が `bold`、`lang` などにも付く（`formatting.rs` の `run_attributes!` 内）。`size`/`spacing`/`kerning` だけに付ける。

## 他の PR との干渉
- PR #82（LibreOffice check）が先に merge されると、この PR が追加する manual check のファイル `pptx/p3a-text-formatting.pptx`（`examples/manual_check.rs`）に `tools/lo-check/expected.json` の entry が必要になる（`check.py` の manifest coverage で落ちる）。
  例: `"pptx/p3a-text-formatting.pptx": {"text": ["Lorem ipsum dolor sit amet"], "xml": [["ppt/slides/slide1.xml", ".//a:r/a:rPr[@b='1'][@sz='2800']", 1]]}`
- PR #85（docx）とは触るファイルが重ならない。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 5 / nit 2）
