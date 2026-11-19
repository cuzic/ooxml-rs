# PR #72 レビュー（Opus, round 2）

対象: fix/xlsx-ignorable-wrappers @ 0631da9f39cb251d993f469cc6f8fb8db517d22f

## 結論（先に）
converged: no。round 1 の M1・M2・m1・n1 は解消した（拒否が `SheetIndex::target_rows` に移り、親の不一致で判定、プローブはすべて `Err` かつバイト一致）。
ただし `contains_wrapped_grid` が `mc:AlternateContent` の部分木を位置に関係なく飛ばすため、外部ラッパーの中や `worksheet` 直下の `mc:AlternateContent` に入った `row`/`c`/`sheetData` が検出されず、round 1 M2 と同じ `Ok` での重複行が残る（M1）。

## 前ラウンドの解消状況
- M1: 解消。`set_row_height`/`set_row_hidden`/`set_row_outline`・`merge`・`merge_clearing`・`mc:ProcessContent` 内セルの `cell`/`cells()` がすべて `UnresolvedSibling(sheetData)` を返し、保存結果はバイト一致（`tests/ignorable_wrappers.rs` と手元プローブ）。`strip_positions` の `check_edit`（`edit.rs:796`）も残っている。
- M2: 解消（下の M1 の穴を除く）。SML の未知要素 `foo`、`extLst/ext`、`sheetData` を包む外部ラッパーの 3 入力が拒否され、バイト一致。
- m1: 解消。`Worksheet` の doc と PR 本文に「依存する数式があるシートでは他シートの編集も拒否」を明記、Open questions に記載。
- n1: 解消。`Worksheet` の `# Wrapped worksheet grids` に 1 か所でまとめ、`CellMut`/`Workbook` から参照。

## 検証の範囲
- マージコミット d15f236 の `git show --remerge-diff` は空（手での衝突解消なし）。
- 過剰拒否なし: 拡張名前空間の要素だけのラッパー（既存テスト）、`worksheet` 直下の `mc:AlternateContent`（controls 相当）、`extLst/ext` 内の `q:row`/`q:c` は編集可能のまま。ラップされたシートでも `set_column_width` やセルテキストを書かない `set_hyperlink` は `Ok`（index を使わない、重複は作らない）。
- `rename_sheet` はラップされた `f` を census が `FormulaScanFailed` で拒否し、バイト一致。
- 公開名: 新 variant なし、生成物の変更なし。index は built 時に `wrapped_grid` を計算し、増分更新は新たなラップを作らないので陳腐化しない。
- 他の open PR（#23 spike）と干渉なし。

## Findings

### M1（Major）: `mc:AlternateContent` の部分木を一律に飛ばすため、外部ラッパー内や `worksheet` 直下の AC 内の grid 要素が検出されない
- 根拠: `crates/ooxml-xlsx/src/index.rs:95` の `.filter(|child| !child.is(MC_ALTERNATE_CONTENT))` は親がどこでも AC に降りない。一方 `alternate`/`has_ac` が見るのは `sheetData` 直下・`row` 直下と「`sheetData` が無いときの `worksheet` 直下」だけ。プローブ（すべて `Ok`、出力が変わる）:
  - `<sheetData>{row1}<q:w mc:ProcessContent="q:w"><mc:AlternateContent><mc:Choice Requires="q"><row r="2">…A2…</row></mc:Choice></mc:AlternateContent></q:w></sheetData>` に `write_number(1,0,7.0)` → `</q:w><row r="2"><c r="A2"><v>7</v></c></row>`（`row r="2"` が 2 つ）。`set_row_height(1,30.0)` も同様に重複 `row`。
  - `<row r="1"><c r="A1"/><q:w …><mc:AlternateContent><mc:Choice …><c r="B1"><v>5</v></c></mc:Choice></mc:AlternateContent></q:w></row>` に `write_number(0,1,…)` → 重複 `c r="B1"`。`cell(0,1)` は `Ok(None)`。
  - `<sheetData>{row1}</sheetData><mc:AlternateContent><mc:Choice …><sheetData><row r="2">…</row></sheetData></mc:Choice></mc:AlternateContent>` に `write_number(1,0,…)` → 2 つ目の `row r="2"` が別の `sheetData` に残る。
- 修正案: AC を飛ばすのは既存検査が担う位置（親が `sheetData` か `row`）だけにする:
  `let skip_ac = parent.is(sml::SheetData::QNAME) || parent.is(sml::Row::QNAME);` として `filter(|c| !(skip_ac && c.is(MC_ALTERNATE_CONTENT)))`。手元でこの変更を入れると上の 3 入力はすべて `UnresolvedSibling(sheetData)` かつバイト一致になり、`worksheet` 直下の controls 用 AC は編集可能のまま、`cargo test -p ooxml-xlsx` は全件通過した。`target_rows` は `alternate` を先に見るので既存 AC 拒否のエラー値は変わらない。上の 3 入力を `ignorable_wrappers.rs` に足す。

### m1（Minor）: `worksheet` 直下に `sheetData` が 2 つあると 2 つ目の中身が index から見えない
- 根拠: `<sheetData>{row1}</sheetData><sheetData><row r="2">…</row></sheetData>` は親の不一致が無いので `wrapped_grid = false`、`write_number(1,0,…)` が `Ok` で 1 つ目に `row r="2"` を足し重複する。スキーマの多重度違反の入力で、本 PR 以前からの挙動。
- 修正案: `contains_wrapped_grid` か `build` で `worksheet` 直下の `sheetData` が 2 つ以上なら `wrapped_grid = true`（`index.rs:144`）。回帰テスト 1 件。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 1 / nit 0）
