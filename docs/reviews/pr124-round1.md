# PR #124 レビュー（Opus, round 1）

対象: impl/pptx-phase3e-styles-theme @ e3be8fd4b6e2817516efd291e291bc6146028c00

## 結論（先に）
converged: yes。ADR-0016 §9 の表スタイル追加（関係によるパートの発見、新規作成時の `@def`・関係・Override、
UUID v8 の決定性と衝突回避）と §10 のテーマ編集（12 スロット、主・副フォント 3 書体、`a:font@script` の保持、
`SharedTheme`）は、プローブで確かめた範囲では正しく動いた。拒否はすべて入力とバイト単位で同一だった。
Blocker と Major は無い。Minor 3 件と nit 2 件は follow-up でよい。

## 前ラウンドの解消状況
初回のため無し。PR #102 round 1 の指摘の解消状況:
- m1（宛先の無い `notesSlide` 関係）: 解消。`ensure_notes` が `UnsupportedContent` で拒否し、
  `dangling_notes_relationship_is_refused_without_changes` が `ensure_notes_slide` と `notes_mut_or_create` の両方で
  バイト一致を確かめている。
- m2（本文プレースホルダーの無いマスター）: 解消。`master_without_body_still_provides_editable_notes` が、本文の編集と読み戻し、
  空の `p:spPr`、マスターのバイト不変を確かめている。位置が無いことは ADR の note に記載された。
- m3（`mc:AlternateContent` の中の `notesMasterIdLst`）: 解消。説明的な `UnsupportedContent` になり、バイト一致テストがある。
- n1・n2: 解消（`notes.rs` のモジュール文書、`notes_mut_or_create` の文書）。

## 確認したこと（プローブはリポジトリ外。コミットしていない）
- **公開名**: 生成物 `dml.rs` の差分は追加のみ（`-` 行 0）。allowlist の変更は、既存の Read 型 19 個を Write に移したものと、
  新しい名前の追加。生成型の名前は変わっていない（`report.txt` は Read→Write の行だけ）。`Error` への追加
  （`MasterIndexOutOfRange`、`SharedTheme`）は `#[non_exhaustive]` の範囲。既存の公開名の削除・改名は無い。
- **id の生成**: version nibble は 8、variant は `10xx`、16 進は大文字で、`ST_Guid` の形。別々に開いた 2 つの deck で同じ名前を
  2 回追加すると、同じ 2 つの id とバイト同一の出力になった（1 回目と 2 回目の id は異なる）。既存の id が小文字
  （`{5c22544a-…}`）でも、比較は大小文字を無視する。組み込み id の判定は `builtin()`（ADR-0013 §3）と同じ。
- **既存の `tableStyles.xml`**: 既定名前空間（接頭辞なし）のルートと先頭のコメントでは、接頭辞なしの `tblStyle` が末尾に追加され、
  コメントと既存の定義はバイトどおり残った。名前の `&` `<` `"` はエスケープされた。`@def` は変わらない。
- **パートの新規作成**: 関係とパートを消した入力では、`ppt/tableStyles.xml`、関係、Override が作られ、`@def` は
  `{5C22544A-…}` で、新しい id ではなかった。ぶら下がった関係、または残った Override がある入力は `Core(NameInUse)` で拒否され、
  バイト一致（既存テスト）。
- **テーマ**: `sysClr` の `dk1` に `Rgb` を書くと、`sysClr` が `srgbClr` に置き換わった。`lt1` に `System(highlight, 090909)` を書くと
  `lastClr` 付きの `sysClr` になった。主フォントの編集後も `a:font@script` 30 件はすべて残った。`ea` の無いテーマは
  `UnsupportedContent` で拒否され、バイト一致。同じ値の再設定では part は dirty にならず、出力はバイト同一。
- **provenance**: `tests/resolve.rs` の追加テストが、`Source::Direct` の維持、新しい `typeface`／`base_rgb`、`themeOverride` のある
  スライドで `Unresolved(ThemeOverride)` のままであることを確かめている。
- **CI**: `gh pr checks 124` は e3be8fd で全件 pass（Rust 1.99 の test・clippy・codegen、python-pptx、python-docx、openpyxl、LibreOffice）。
- **他の open PR**: #123 は `tools/lo-check/expected.json` に触れるが、ケース 08 のエントリで、34・35 とは別のキー。#122、#125 とは重ならない。

## Findings

### m1（Minor）: 配布資料マスターとの共有テーマを `SharedTheme` として検出しない
- 根拠: `poi/45545_Comment.pptx` の `handoutMaster1.xml.rels` を、スライドマスターと同じ `theme1.xml` に向けると、
  `master_theme_mut(0)` は `Ok` を返す。`theme.rs:143-146` は送り元のコンテンツタイプを `slideMaster` と `notesMaster` だけで判定する。
  ADR-0016 §10 は「more than one master or a master and a notes master」と書いており、配布資料マスターも master に含まれると読める。
  その場合は、配布資料の配色とフォントが許可なしに変わる。ファイルは壊れず、PowerPoint は通常テーマを分けるため、Minor にする。
- 修正案: `handoutMaster` のコンテンツタイプも判定に加える（または、自分以外の送り元からの `theme` 関係をすべて共有とみなす）。どちらにしたかを ADR の note に書く。

### m2（Minor）: スライドマスター同士の共有テーマの分岐にテストが無い
- 根拠: `theme.rs:145` の `Some(CT_MASTER | CT_NOTES_MASTER)` を `Some(CT_NOTES_MASTER)` に変える変異で、`ooxml-pptx` の全テストが通った。
  `shared_theme_requires_opt_in` はノートマスターとの共有だけを使う。ADR が名指しする「複数のマスター」の場合が固定されていない
  （コーパスには 0 件）。現在のコードの挙動は正しい。
- 修正案: 2 つ目のスライドマスターの関係を同じテーマに向けた入力で、`SharedTheme` とバイト一致、`allow_shared: true` での編集を確かめるテストを 1 件足す。

### m3（Minor）: `mc:AlternateContent` を含む `tblStyleLst` やテーマのスロットが、汎用のコアエラーで拒否される
- 根拠: `tblStyleLst` の子に `mc:AlternateContent`（中に `a:tblStyle`）を入れると、`add_table_style` は
  `Err(Core(UnresolvedSibling(a:tblStyleLst)))` を返す。`a:accent2` の色を `mc:AlternateContent` で包むと、
  `set_color(Accent2, …)` は `Err(Core(UnresolvedSibling(a:accent2)))` を返す。どちらもバイト一致で、安全側に倒れている。
  PR #102 m3 と同じ形で、利用者には原因がわからず、この入力のテストも無い。
- 修正案: `UnsupportedContent` などの説明的なエラーに変え、Err ⇒ byte-identical テストを足す（PR #102 m3 と同じ扱い）。

### n1（nit）: `ThemeFonts` の値に対する文書と検査
- 根拠: `set_major_font` は 3 つの書体をすべて書くので、`..ThemeFonts::default()` を使うと既存の `ea`／`cs` が空文字列で上書きされる
  （テストもこの書き方をしている）。また、テーマ自身に `+mj-lt` などの参照を書いても `Ok` になる（プローブで `typeface="+mj-lt"` が出力された）。
  resolver は再帰しないので壊れはしないが、自己参照で意味の無い値になる。
- 修正案: 3 つとも書き込み、空文字列は消去を意味することを `ThemeFonts` の文書に書く。`+mj-*`／`+mn-*` は `ValueOutOfRange` などで拒否するか、文書で注意する。

### n2（nit）: 新規作成した `ppt/tableStyles.xml` に XML 宣言が無い
- 根拠: `table_style.rs:172-174` が宣言なしで文書を作る。プローブの出力は `<a:tblStyleLst …>` で始まる。UTF-8 の既定どおりで、
  python-pptx と LibreOffice の CI は通っている。ただし `slides.rs` の新しいスライドは `standalone="yes"` の宣言を付けており、揃っていない。
- 修正案: 他の新規パートと同じ宣言を付ける。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 3 / nit 2）
