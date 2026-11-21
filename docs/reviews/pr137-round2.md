# PR #137 レビュー（Opus, round 2）

対象: impl/docx-phase3e-fields @ 1bc79fdd4372dadb37a76fa84f3b797c7f8cd9c2

## 結論（先に）
round 1 の M1 は挙動で解消を確認した。5 run すべてに同じ `rPr` が付き、begin だけに付ける変異は `phase3e` のテストで落ちる。
m1〜m3 と n1〜n3 も解消している。新しい Blocker と Major はない。
新たに Minor が 2 件ある。SEQ の識別子の検査が ASCII に限られ、日本語版 Word の既定の図表番号ラベル（`図`、`表`）を拒否する（round 1 の m3 で示した修正案が狭すぎた）。
もう 1 件は、日付型の DOCPROPERTY が `w:dirty` だけに頼っていること。
CI は Rust ジョブを含む 5 ジョブすべて pass。converged: **yes**。

## 前ラウンドの解消状況
- M1: 解消。プローブでは `<w:b/>` の run の後ろに挿入すると、begin、instrText、separate、結果、end の 5 run すべてに同じ `rPr` が付いた。
  `insert_field.rs` の複製を `offset == 0` に限る変異では、`pagination_cache_copies_format_and_changes_only_the_target_paragraph` が落ちた（phase3e.rs:255）。ADR の D3e note も直っている。
- m1: 解消。段落をまたぐ範囲、表のセルの中の段落、`bookmarkEnd` の直前、の 3 つの挿入位置で `InvalidValue`（self-referencing REF bookmark）になり、出力は無変更だった。
  同じ段落で `bookmarkEnd` より後ろの位置と、挿入位置に置かれた折りたたみのブックマーク（start と end が同じ位置）は、範囲の外として Ok になる。PAGEREF は従来どおり Ok。
- m2: 解消。日付型の 4 種（CREATETIME、LASTSAVEDTIME、LASTPRINTED、custom の `vt:filetime`）は、空の結果 run（`<w:r><w:rPr>…</w:rPr></w:r>`。CT_R の子は省略可能なので妥当）と、begin の `w:dirty` を持つ。
  再オープン、本文テキストの読み出し、opt-in なしでの続く編集は、テストとプローブで通った。D3d のバインディング検査（`binding.rs` の `write`）は命令文から求めたキーだけを見るので、キャッシュが空でも影響しない。
  python-docx の CI は pass した。ただし manual-check にこのケースはない（下の m5）。
- m3: 解消。ただし検査の範囲が狭すぎる（下の m4）。不正な識別子は `InvalidValue` になり、出力は無変更。
- n1: 解消（doc コメントと note に記載）。
- n2: 解消。PR 本文に 84 件の内訳がある（w:sdt 46、StaleFieldRisk 29、TrackRevisionsOn 6、InsideRevision 2、NotDetachable 1）。
- n3: 解消（doc コメントと note に記載）。

## Findings

### m4（Minor）: SEQ の識別子が ASCII に限られ、`図` や `表` を拒否する
- 根拠: `insert_field.rs` の `FieldSpec::Seq` の分岐は `is_ascii_alphabetic`、`is_ascii_alphanumeric`、`_` だけを許す。
  テスト `sequence_identifier_requires_an_ascii_letter_then_ascii_word_characters` は `図表` の拒否を期待している。
  プローブでは `Seq { identifier: "図" }` が `InvalidValue` になった。日本語版 Word の図表番号の既定ラベルは `SEQ 図 \* ARABIC` と `SEQ 表 \* ARABIC` で、日本語の文書では SEQ の主な用途である。
  これは安全側の過剰拒否で、壊れたファイルは作らない。round 1 の m3 で `[A-Za-z0-9_]` を修正案に挙げたのはこちらの誤り。
- 修正案: 先頭の文字を `char::is_alphabetic`、残りを `char::is_alphanumeric` か `_` で検査する。テストの `図表` を受理側に移し、`1図` と `図 表` を拒否側に加える。

### m5（Minor）: 日付型の DOCPROPERTY が `w:dirty` だけに頼っている
- 根拠: 日付型の DOCPROPERTY は W2 で安全なフィールドなので、`updateFields` を要求しない。そのため、Word が値を表示するかどうかは `w:dirty` だけで決まる。
  ADR-0015 §7 は「`w:dirty` alone is not relied on（Word の扱いは *(u)*）」としている。Word が dirty を無視すれば、手動で更新するか印刷するまで空のまま表示される。
  round 1 の生の W3CDTF 文字列よりは害が小さいが、この選択は note に書かれているだけで、owner の判断は記録されていない。
  manual-check と LibreOffice の検査にもこのケースがない。LibreOffice は DOCPROPERTY CreateTime を自前の文書情報フィールドとして読むので、表示はおそらく問題ない *(u)*。
- 修正案: manual-check と `tools/lo-check/expected.json` に CREATETIME のケースを 1 件加え、Word で確認する項目にする。
  あわせて、空のキャッシュと dirty だけでよいか、Word の既定の書式で整えたキャッシュも入れるかを、ADR の Open questions として owner に残す。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 0）
