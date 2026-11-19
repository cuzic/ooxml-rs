# PR #87 レビュー（Opus, round 2）

対象: impl/pptx-phase3a-text-format @ 0bf99d726e7f4ec2687aecfcb4ecc1f6fd5c59f9

## 結論（先に）
converged: yes。round 1 の M1 と m1〜m5 は、タスク仕様のとおり挙動で直っている（round 1 のプローブを再実行し、各修正を消す手書きの変異はどれも `tests/formatting.rs` で落ちる）。
round 1 以降のコミット（7930898, 0bf99d7）に退行は無く、Err ⇒ byte-identical、局所性（`a:fld` の開始タグは引用符まで不変）、生成コードの公開名（生成物に変更なし）も保たれている。CI は head で 2 ジョブとも pass。

## 前ラウンドの解消状況
- M1: 解消。`range.rs:71` で範囲に丸ごと入る `a:fld` / `a:br` も計画し、`apply_range_edit`（`range.rs:144-163`）が分割せずに直接の `a:rPr` へ書く。プローブ: `a:rPr` 無しの `a:fld`、`a:pPr` だけを持つ `a:fld`（`a:rPr` が先頭に入る）、`a:rPr` 付きの `a:br`、フィールドだけの段落、split run + br + fld + br + split run の混在で、いずれも `Ok`、順序と本文は正しく、2 回目は byte-identical。境界がフィールドの内側の 5 通り（開始・終了・両方・空範囲・先頭から途中）は `RangeSplitsField` で byte-identical。
- m1: 解消。`checked_properties`（`formatting.rs:13`）を run の `format`、`end_format`、`update_properties`、`bullet`、`format_range`（`a:fld`/`a:br` の `a:rPr` を含む）、`autofit`/`clear_autofit`/`reset_autofit_scale` が通る。`a:endParaRPr`・範囲内の `a:fld` と `a:br` の `a:rPr` の AC、属性だけの `bold` も `InsideAlternateContent` で byte-identical。範囲外の `a:fld` の AC は拒否しない。
- m2: 解消。`underline.rs:89-112` が子の数と属性の数も比べる。`a:round`/`a:custDash`/`a:extLst`/`algn` に加え、fill + `a:round`、`prstDash` + `a:custDash` も `<a:uLn/>` に置き換わる。
- m3: 解消。`changing_typeface_drops_metadata_for_the_old_font` があり、`remove_pitch_family` を消す変異で落ちる。
- m4: 解消。run 17 属性（fill・highlight・font 4 種・underline line/fill を含む）と段落 10 setter（bullet の 3 外観・tab・`font_align` を含む）を write → `Source::Direct` → clear → `ShapeListStyle` で表駆動に確かめ、autofit も clear で元の provenance に戻る。
- m5: 解消。`format`/`end_format`/`format_range`/`autofit` は元の木で比較してから clone する。`equal_formatting_keeps_the_existing_slide_tree` が早期 return を消す変異で落ちる。
- n1, n2: 未対応（任意の nit。再指摘しない）。

## Findings
無し。

補足（指摘ではない）: `checked_properties` はプロパティ要素の子孫すべてを見るので、`a:rPr/a:extLst/a:ext` の中の AC でも拒否する。安全側で、ADR-0016 の post-acceptance note（「inside the edited property element」）とも合っている。

## 他の PR との干渉
- PR #82（LibreOffice check）は未マージ。#82 が main に入ったあとは、この PR の manual check のファイル `pptx/p3a-text-formatting.pptx` に `tools/lo-check/expected.json` の entry が要る（round 1 の案を使える）。タスク仕様どおり、#82 のマージ後に main を取り込んで足す。この PR への指摘ではない。
- PR #89（xlsx tables）とは触るファイルが重ならない。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 0）
