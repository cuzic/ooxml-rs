# PR #86 レビュー（Opus, round 1）

対象: impl/xlsx-phase5a-images @ bee0da00f6f613e92ff8ff3d1c8911aa5258152e

## 結論（先に）
converged: yes。Blocker と Major は見つからなかった。ADR-0017 の §2.1（plan → validate → apply、
`<drawing>` の挿入だけを apply 後に行う、編集した drawing だけを現在の状態から stage する）、
§2.2 B2（drawing の全分岐、VML `id`/`o:spid`、`legacyDrawingHF`、control/OLE `shapeId`、セッションの最大値）、
§3.1–3.3 は挙動で確認できた。残りは過剰拒否 2 件、テストの穴 2 件、文書 1 件で、follow-up に回してよい。
CI（test/clippy/codegen --check、python-pptx オラクル）は両方 pass。

## 前ラウンドの解消状況
初回のため無し。

## 確認したこと（プローブ。コミットしていない）
- **B2 のコーパス**: `45540_form_Footer`/`_Header` → 2141（既存最大 2139 の上）、`ExcelWithAttachments.xlsm`
  シート 1 → 1039（VML 1032–1038 の上）、`mv-calculator-final-2-20-2013.xlsm` の MV Calculator → 1332
  （1133 の上）、`BrNotClosed.xlsx`（XML でない VML）→ 1026（バイト走査）。どれもそのシートの drawing・VML・
  control の番号と衝突しない。
- 削除後の再利用なし: 2, 3 を挿入 → 3 を削除 → 次は 4。再オープン後のセッションでも、削除した id の次から振る。
- `Err ⇒ byte-identical`: 新しい drawing の経路で `xml_part_mut` を呼んだ後に registry が拒否する（空の hyperlink
  → `InvalidTarget`）場合も保存バイトは同一。`StalePlan` 注入、`ImageDrawing` での Err/panic による poison は既存テストどおり。
- drawing の root が既定名前空間かつ自己終了（`<wsDr xmlns="…"/>`）、UTF-8 BOM 付きの drawing への挿入・削除は
  well-formed のまま局所的に行われる。
- メディア共有: 別シートの既存 drawing が埋め込んだ同一バイトを再利用し、片方の削除ではメディアを残し、
  最後の参照の削除で part と `Override` を消す。シート削除でメディアが消えた後の挿入は新しい part（古い索引を使わない）。
- **生成物の公開名**: `dml.rs`/`sml.rs`/`wml.rs` を origin/main と impl 単位で比較し、既存の型・関数・定数の
  削除・改名は 0（差分の `-` 行は並び替え）。追加のみ。`ooxml-core` の変更は `Package::part_name` の追加だけで既存挙動は不変。
- 他の open PR: #85 が `generated/dml.rs`/`wml.rs` に触る。後からマージする側が再生成する。

## Findings

### m1（Minor）: `CellRef::image` が OPC 等価な target を解決できない
- 根拠: `crates/ooxml-xlsx/src/images/rich.rs:31` は `rels.resolve` の結果をそのまま `pkg.read_part(&name)`
  （完全一致）に渡し、`:147` も `pkg.relationships(&name)` に同じ名前を渡す。workbook の rels で
  `richData/rdrichvalue.xml` を `richData/RDRichValue.xml` に変えた calamine fixture では、2 つの画像セルが
  `Err(UnresolvedRichValue { step: "rich value data" })` になる。drawing とメディアは bee0da0 で
  `Package::part_name` による正規化が入ったが、rich value 側には入っていない。fail closed なので Minor。
- 修正案: `part()` と `:147` で `pkg.part_name(&name)` を通してから読み、テストを 1 つ足す。

### m2（Minor）: DOCTYPE 付きの SVG を `ImageFormat` で拒否する
- 根拠: `crates/ooxml-xlsx/src/images/mod.rs:143` は `XmlDocument::parse` で SVG を検査するので、
  `<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" …>` 付きの SVG（Illustrator などが出力する）が
  `Err(ImageFormat)` になる（プローブで確認）。SVG は保存するだけで、ルート要素の確認に DTD は要らない。安全側の過剰拒否。
- 修正案: エンティティを解決せずにルート要素名だけを確認する方法（DOCTYPE を読み飛ばす）にするか、
  拒否を post-acceptance note に書く。

### m3（Minor）: ヘッダー解析の fuzz target が無い
- 根拠: ADR-0017 §3.1（review m4）は「ヘッダー解析は … fuzz target である」とする。`fuzz/fuzz_targets/` に
  画像ヘッダーの target は追加されていない（PR 本文の libFuzzer 実行はローカルのみ）。`header.rs:162-236` の
  切り詰めテストはあり、`take`/`get` による境界チェックは読んだ範囲で正しい。
- 修正案: `fuzz/fuzz_targets/xlsx_image_header.rs`（`Image::from_bytes` と `Image::from_svg(_, data)`）を追加し、
  `fuzz/README.md` の表に載せる。

### m4（Minor）: B2 の不変条件を検査するテストが CI に無い
- 根拠: テストオラクル 2・3 は「シートごとに `cNvPr` ∪ VML 番号（`id` と `o:spid`）が一意」と、明示ケース
  `mv-calculator…`、`45540_form_*`、`ExcelWithAttachments.xlsm` を求める。`tests/images.rs:352` の sweep は
  `OOXML_P5A_POI` が無いと何もせず、drawing 内の `cNvPr` の重複しか見ない。CI で走るのは合成 VML の 3 ケース
  （`:490`）だけ。挙動は上のプローブで正しい。
- 修正案: sweep の検査を、そのシートの drawing の `cNvPr`・VML の番号・control/OLE の `shapeId` の和集合で新しい id が
  一意かに広げ、上記ファイルをログに明示する（fixture に同梱できないなら sweep のログで足りる）。

### m5（Minor）: 新しい拒否が ADR に記録されていない
- 根拠: `crates/ooxml-xlsx/src/error.rs` の `ImageAnchor`（アンカー範囲外、`from` > `to`）と、代替テキストが
  255 UTF-16 単位を超えたときの `InvalidSetting`（`images/mod.rs:601-608`）が §11 の表にも P5a の post-acceptance note にも無い
  （note は `NoSuchImage` だけを挙げる）。AGENTS.md「新しい拒否は ADR の note か PR 本文に記録」。
- 修正案: P5a の post-acceptance note に 2 つの拒否を 1 文ずつ足す。

### n1（nit）: `regex`・`sha2` を crate 直下で宣言している
- 根拠: `crates/ooxml-xlsx/Cargo.toml:21-22` は `regex = "1"`、`sha2 = "0.11"` を直接書いている。ほかの依存は `workspace = true`。
- 修正案: ワークスペースの `[workspace.dependencies]` に移す。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 5 / nit 1）
