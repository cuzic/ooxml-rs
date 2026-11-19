# PR #57 レビュー（Opus, round 3）

対象: impl/pptx-phase1b-tables @ 0a6f72f

範囲: round 2（64272bd, converged）以降の main マージ 0a6f72f の衝突解消だけ（`git show --remerge-diff 0a6f72f`）。

## 結論（先に）
衝突解消の中身（allowlist の重複排除、CI、manual_check の番号付け、anchor 分類の共有）は正しく、
生成型の公開名・アクセサは main 側・PR 側のどちらからも 1 つも失われていない。
ただし crate ルートの `Color` を main（#56）の resolver 型から PR の書き込み用 enum に付け替え、
resolver 型を `ResolvedColor` に改名した点は、マージ済み公開 API の黙った破壊的変更で、どこにも記録がない。
converged: no（M1）。PR 側の未マージ型を改名すれば main の API は無傷で済む。

## 前ラウンドの解消状況
- round 2 は converged（指摘なし）。マージで PR のコードの挙動が変わっていないことを下で確認した。

## 確認したこと（問題なし）
- **allowlist / overrides**: `[read]` から消えた 9 件（dml の `lstStyle` `bodyPr` `off` `ext` `solidFill` `noFill` `srgbClr` `schemeClr`、pml の `a:CT_Transform2D/p:xfrm`）はすべて `[write]` に残る。
  `codegen/report.txt` の「名前 (slots, attrs)」集合は main ∪ PR と完全一致（差分は Read→Write の種別だけ）。
  生成 `dml.rs` / `pml.rs` のトップレベル型名は同一、`impl` ごとの `pub fn`/`pub const` は削除 0、
  追加は PR が allowlist に入れた `a:extLst` による `*View::extension_list` だけ。ADR-0002 の公開名変更なし、resolver の読み取り経路も不変。
  overrides は PR の a16 rowId/colId と main の `DmlPercent` 群が両方残っている。
- **CI**: `table_cases` + `tables.py` と `resolve_dump` + `resolve_check.py` の両方が残る（ci.yml 52–60 行）。
- **refs.rs**: `classify_anchor_descendants` は PR 側の `text_like`/`safe` と同じ判定で、`scan_table_anchor` は従来どおり safe 判定の前に走る。
  table のロックは保たれている。`cargo test -p ooxml-pptx --lib refs` 2/2 pass。
- **manual_check**: `pptx_manual_check` を実行し 25 ファイル。17–22 が M1–M7、23–25 が表、CHECKLIST の見出しも 23–25。

## Findings
### M1（Major）: マージ済みの公開名 `ooxml_pptx::Color` の意味を変え、resolver 型を `ResolvedColor` に改名したが、記録がない
- 根拠: `crates/ooxml-pptx/src/lib.rs:50` `Color as ResolvedColor`、`:61` `write::Color`。main（628166f）では `ooxml_pptx::Color` = `resolve::Color<'a>`（struct、#56）。
  マージ後は同じパスが `write::table::Color`（enum `Rgb`/`Scheme`）を指すため、main の API で書いた利用側コードは型不一致でコンパイルできなくなる。
  ADR-0008 は `Color::theme_slot`（111 行）と `Fill::Solid(Color<'a>)`（923–924 行）で resolver 型を `Color` と呼んでいて、
  `ResolvedColor` という名前は ADR-0008/ADR-0013・README・PR 本文のどこにも出てこない（AGENTS.md 41 行「新しい公開 API・挙動変更は ADR の note か PR 本文に記録する」）。
- 修正案: まだマージされていない PR 側の enum を改名し（例: `FillColor` または `ColorChoice`。`CellFill::Solid(..)`・`Line::Solid { color }`・tests/examples の 3 か所を追従）、`resolve::Color` を `Color` のまま公開する。
  改名を残すなら、ADR-0008 の post-acceptance note と PR 本文に「`Color` → `ResolvedColor`（#57）」を書く。

### m1（Minor）: PR 本文の manual-check 番号が古い
- 根拠: PR 本文 77 行が「Manual-check items 17–19」のまま。実際は 23–25。
- 修正案: PR 本文を 23–25 に直す。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 1 / nit 0）
