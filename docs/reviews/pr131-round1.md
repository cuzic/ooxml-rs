# PR #131 レビュー（Opus, round 1）

対象: impl/docx-phase3d-content-controls @ d54be5392eaf77d1d3449fad220a8e0e08df3ada

## 結論（先に）
値の編集、データバインディング（同じノードの全コントロールの一括更新、接頭辞の解決と `[1]` の補完、storeItemID の大小文字、
store が無い場合や itemID が一致しない場合、拒否される名前空間への書き込み）、M5、ゲート、#99 m1/m2、#122 n1/n2 はプローブで正しく動いた。
Err のときは出力がバイト一致した。
Major が 2 件ある。1 つは、グレゴリオ暦以外の `w:calendar` を持つ日付コントロールに、`Ok` のままグレゴリオ暦の表示を書くこと。
もう 1 つは、囲い込みの拒否（フィールド・変更履歴・SDT の祖先・section break）にテストが無く、ガードを外しても全テストが通ること。
converged: no。

## 前ラウンドの解消状況
（round 1 のため無し）。Issue の追加項目:
- #99 m1: 解消。`merge_restarts_after_deletion` を共有し、tracked 削除は `TrackedPropertyChange { element: "w:vMerge" }` で拒否する。拒否時はバイト一致する。ADR の注記もある（`tracked_delete_refuses_to_orphan_a_vertical_merge`）。
- #99 m2: 解消。挿入位置の拒否とテンプレート行のマーカーの拒否を、別々の入力で確かめている。
- #122 n1: 解消（`leaf_text.rs` に metaAttributes の行が加わった）。n2: 解消（`smoke.rs` に独立した `fn leaf_text`）。

## Findings

### M1（Major）: グレゴリオ暦以外の `w:calendar` で、`set_date` が `Ok` を返し、グレゴリオ暦の表示を書く
- 根拠: `<w:date><w:dateFormat w:val='yyyy/MM/dd'/><w:lid w:val='en-US'/><w:calendar w:val='hijri'/></w:date>` に `set_date(2026-11-21)` を呼ぶと、`Ok` で表示テキストが `2026/11/21` になる。`japan`、`taiwan`、`thai` でも同じ（プローブ）。Word はこれらの暦の年（ヒジュラ暦 1448、民国 115、仏暦 2569 など）で表示し、unbound なコントロールでは sdtContent の文字列をそのまま表示する。`render_date`（`controls.rs:264`）と `date::format` は `w:calendar` を読まない。ADR §6.1 と Q6 は、ロケールに依存する整形を `UnsupportedDateFormat` にする方針である。この入力では `Ok` のまま誤った表示を書くので、その方針に反する。
- 修正案: `w:calendar` があり、値が `gregorian`（必要なら `gregorianUs` などの Gregorian 系）でなければ `UnsupportedDateFormat`（token に暦名を入れる）を返し、`set_date_with_display` に誘導する。Err の場合はバイト一致することをテストする。

### M2（Major）: 囲い込みの拒否のガードにテストが無い（手書きの変異 3 件が生き残る）
- 根拠: 次の変異を 1 つずつ入れて `cargo test -p ooxml-docx --test phase3d --lib` を走らせると、どれも全テストが通った。
  - `wrap.rs:174` の `run.check_settable()?` を無効化した。フィールドの中の run、`fldSimple`、区切りの脚注、文字種のチェックが外れる。プローブでは、元のコードはフィールドの instrText と結果の run を `InsideField` で拒否している。
  - `wrap.rs:279` の `self.check_control_runs(first.story(), &path)?` を無効化した。段落の囲い込みで、フィールド・変更履歴範囲・tracked な段落記号のチェックが外れる。プローブでは、元のコードは複数段落にまたがるフィールド、`moveFrom` の範囲、段落記号の `w:ins` を正しく拒否している。
  - `wrap.rs:110` の id 収集を最初のストーリーだけにした（§2 の id のスコープ。m2 も参照）。
  
  さらに、`wrap_in_control` の拒否（`InsideField`、`InsideRevision`、`UnsupportedContext { ancestor: "w:sdt" }`、sectPr を持つ段落、連続していない範囲、ストーリーをまたぐ範囲）には、Err のときにバイト一致することを確かめるテストが 1 件も無い。あるのは `BindingMismatch` の 1 件だけである。AGENTS.md の完了の定義（新しい拒否にはバイト一致のテスト）と、Issue が挙げる観点（「変更履歴やフィールドコードをまたぐ」）の両方に対して、テストが足りない。
- 修正案: インラインと段落について、フィールド（結果の run と instrText の run、段落をまたぐフィールド）、`w:ins`/`moveFrom` の範囲、段落記号のリビジョン、SDT の中の段落、sectPr、表のセル以外のコンテナ（テキストボックス）の拒否テストを、`to_bytes() == input` 付きで追加する。

### m1（Minor）: 日本語の日付形式 `yyyy年M月d日` が必ず `UnsupportedDateFormat` になる（安全側の過剰拒否）
- 根拠: `date.rs:23` は `first.is_alphabetic()` で token を判定する。CJK 文字も alphabetic なので、`年` が token として拒否される（プローブ: `Err(UnsupportedDateFormat { token: "年" })`、`w:lid='ja-JP'`）。Word の picture では、token でない文字は引用しなくてもリテラルとして扱われる。Q6 で Err にするのは和暦の `g`/`e`、曜日名、ロケール依存の月名であり、`年月日` は含まれない。日本語の文書で最もよく使われる形式が、常に `set_date_with_display` を必要とする。
- 修正案: token の候補を ASCII 英字（または Word の token 文字 `y M d H h m s g e a A` など）に限り、それ以外の文字はリテラルにする。

### m2（Minor）: M5 の `INFO <prop>` の判定と、SDT id を全ストーリーから集める処理にテストが無い
- 根拠: `binding.rs:258` の `|| (… code.strip_prefix("INFO ") == Some(shorthand))` を消しても、全テストが通った。挙動自体は正しく、プローブで `INFO Title` は `StaleFieldRisk { codes: ["INFO TITLE"] }`、テキストボックスの中の `TITLE` も拒否された。id の収集範囲を本文だけにした変異も生き残った（M2 を参照）。後者は 32 bit 乱数なので衝突はまれだが、§2 のスコープの規則にテストが無い。
- 修正案: `INFO Title` のケースを core properties のテストに加える。ヘッダーにある SDT の id だけを返す決定的なソースで、`wrap_with_ids` が別の id を選ぶことを確かめる単体テストを加える。

### m3（Minor）: `select` が `w:dropDownList`/`w:comboBox` の `w:lastValue` を更新しない *(u)*
- 根拠: `<w:dropDownList w:lastValue='k2'>…` に `select(c, "k1")` を呼ぶと、表示は `One` になるが、`w:lastValue='k2'` が残る（プローブ）。Word がこの属性を選択状態の判定に使うかどうかは未確認であり、ADR §6.1 の表にも記載が無い。
- 修正案: `w:lastValue` が存在するときは、選んだ value で更新する（無いときは追加しない）。ADR の表に 1 行の注記を入れる。

### n1（nit）: チェックボックスとして囲むと、元のテキストがそのまま内容に残る
- 根拠: `wraps_partial_inline_ranges_and_sibling_paragraphs` は `αβγδ` をチェックボックスとして囲み、`w14:checked=0` の状態のまま内容を `αβγδ` にする。ADR §6.3 の「現在のテキストを内容にする」には沿っているが、状態とグリフが一致しない。
- 修正案: ドキュメントコメントで、チェックボックスでは呼び出し側がグリフを持つ run を選ぶことを明記するか、`set_checked` と同じ規則でグリフに置き換える。

## 確認した項目（指摘なし）
- 同じノードの一括更新（別ストーリーのピアを含む）、`/a:root[1]/a:value` と `/b:root/b:value[1]` の同一視、別の葉は変えないこと（T2）。同じ store を 2 回続けて編集すると、2 つの変更が両方残る（custom XML、core.xml）。`&` と `<` はエスケープされる。
- 拒否される名前空間（WML）の store への書き込み、itemID が一致しない場合、属性の述語を含む XPath は、どれも `UnsupportedBinding` で、出力はバイト一致する。storeItemID の小文字は受け付ける。
- ピアが `mc:AlternateContent` の中にあると、全体が `UnsupportedControl` になり、出力はバイト一致する。ドロップダウンの表示は displayText で、store には value が入る。コンボボックスのピアは自分の displayText を表示する。表示テキストで `select` すると `InvalidValue` になる。チェックボックスの store は `true` で、プレーンテキストのピアも `true` を表示する。
- 囲い込み: 表のセルの段落の囲い込み、エスケープ（`&"<`）、連続した 2 回で id が異なること、ロックされた SDT の中への囲い込みの拒否、テキストボックスの段落が段落列に現れないこと。
- 他の PR との干渉: #129 とは `codegen/report.txt` と `tools/lo-check/expected.json` が重なるが、`git merge-tree` では衝突しない（lo-check の番号は docx 側と xlsx 側で別系列）。#125 と #132 はファイルが重ならない。#127 は存在しない。origin/main（#124、#130）とも衝突しない。
- CI: 5 ジョブすべて pass（test/clippy/codegen、python-docx、python-pptx、openpyxl、libreoffice）。

## 判定
converged: no（Blocker 0 / Major 2 / Minor 3 / nit 1）
