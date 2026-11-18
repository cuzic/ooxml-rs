# PR #67 レビュー（Opus, round 1）

対象: `fix/xlsx-f8-row-mismatch` @ `6c16a51`（base `5e906ef` = `origin/main`）。fuzz finding F-8（`c/@r` が `row/@r` と別の行を指すセル）への対応。
リポジトリは変えていない。確認はすべて scratchpad の `git clone` 作業コピー（`review-pr67/`）で行った。

## 結論（先に）

**未収束（Major 1 件、#58 側の Major 1 件、Minor 1 件、nit 2 件）。** 方針（fail closed、ワークブック全体で編集を拒否）は妥当で、コーパスへの影響は 0 件だった。ただし census と row index で `row/@r` の読み方が違うため、F-8 と同じ重複が残る入力がある（M1）。

- M1: `row r=" 1"`（XML 空白付き）では census が行番号を読めず、検査が素通りする。row index のほうは空白を除いて行 1 と読むので、`set_formula` は今も 2 つ目の `c r="B2"` を書く。PR head で再現した。
- M2（#58 側）: #58 の `merge`、`set_autofilter`、`protect_sheet` は `census.block` を見ない。#67 と #58 をローカルでマージすると、`edit_refusal()` が `Some` のワークブックで `merge("A2:B3")` が通り、B2 の値が結合範囲に隠れる。
- 2914 ファイル（重複除去後）で `origin/main` と比べた。編集可から拒否に変わったのは 1 件だけで、PR #29 レビューで作った合成ファイル（意図的な不一致）だった。

## 確認できたこと（指摘ではない）

| 観点 | 結果 | 根拠 |
|---|---|---|
| F-8 再現入力 | 解消 | `<row r="1">…<c r="B2"><f>…` は `InvalidReference("xl/worksheets/sheet1.xml: B2 in row 1")`。4 種の編集すべてが拒否され、パッケージはバイト単位で変わらない |
| コーパス影響 | 0 件 | ホスト上の `*.xlsx/xlsm/xltx/xltm` 25298 本を sha256 で重複除去し、2914 本を main と PR で比較した（`main.tsv` / `pr.tsv`）。内訳: rust_xlsxwriter 1026、round-trip/o3 876、xlsxfx 320、fuzz corpus 273、calamine 85、umya 73、POI 66、その他 194。open 失敗 93 本と既存の拒否 8 本は main と同じ。差分は `review-pr29/dump/syn2291.xlsx`（合成、`C4 in row 3`）の 1 件だけ。LibreOffice と Open XML SDK の xlsx はホスト上に無かった（`reference/openxml-sdk-data` に xlsx は無い） |
| ワークブック全体での拒否 | 妥当 | 他シートの編集でも、依存セルの cache 除去は row index で場所を探し、calcChain の判断は `c/@r` で引く census に基づく。その不一致を残したまま編集させるのは危ない。シート単位に絞れるのは数式の無いワークブックだけで、コーパス影響が 0 件なので、分ける価値は小さい |
| 小文字と `$` | 検出する | `c r="$b$2"`（`row r="1"` の中）は拒否される。census と index は同じ `CellIndex::parse` を使うので、読み方は一致する。`row r="+1"` も両方 1 と読み、拒否される |
| `r` の無い row | 範囲外で問題なし | census は検査しない。index は `MissingRowR` でそのシートの読み書きを拒否する。数式のあるシートなら既存の `MissingReference` がワークブックを止める（プローブ `row_without_r`） |
| `r` の無い c | 問題なし | 暗黙の位置はその行の中なので、行の不一致は起こらない。index はその行を `MissingCellR` で拒否する。B2 への書き込みは正当な新しい行 2 を作る |
| 行の順序違い | 問題なし | index が `RowsOutOfOrder` でシート単位に拒否する。census は行ごとに不一致を検査し続ける |
| 不正な `c/@r` | main と同じ | `" B2"`、`B02` は main でもすでに `FormulaScanFailed` でワークブック全体を止めている |
| open 時の性能 | 差は測れない | 1M セル（100000×10、1000×1000）の `from_bytes` を 7 回ずつ交互に測った。負荷平均が約 31/32 で揺れが大きいが、最小値は main 1813/1432 ms、PR 1770/1425 ms。足された処理（セルごとの `CellIndex::parse` と比較）だけを測ると 1M 回で 17 ms で、open の約 1% 以下 |
| #50（Phase2） | 同じゲートを通る | `setters.rs` の全 setter は `edit_cell_ext` を通り、`edit.rs:183` で `census.block` を見る |
| #53（Phase3） | 同じゲートを通る | `structural_guard` が `census.block` を見る。F-8 のワークブックでは `add_sheet`、`set_tab_color` なども拒否される。既存の block と同じ扱いなので許容 |
| #58（Phase4） | 通らない | M2 |
| エラー型 | 妥当 | `InvalidReference` は index の `RowErr::WrongRow` と同じ型と文言（part 名の接頭辞だけが違う）。`Error` は `#[non_exhaustive]` なので専用の variant も作れるが、今の型で十分 |
| ビルドとテスト | ok | `cargo test -p ooxml-xlsx --release --test row_mismatch`、census の unit test が pass |

## 指摘

### M1（Major）: `row/@r` の空白で検査が素通りし、重複セルが書かれる

`crates/ooxml-xlsx/src/census.rs` L250（`row_number = r.and_then(|r| r.parse().ok());`）

census は `str::parse` で読むので、`" 1"` は `None` になり、その行のセルは検査されない。index（`index.rs` L138）は `ooxml_core::value::parse_u32`（`trim_xml_ws` の後で parse）で読むので、`" 1"` は行 1 になる。`ST` の `xsd:unsignedInt` は whitespace collapse なので、`r=" 1"` は XSD 上は有効な値。

PR head で再現した（プローブ `ws_row_r`）。

```text
refusal=None edit=Ok; sheet: <sheetData><row r=" 1"><c r="A1"><v>1</v></c><c r="B2"><f>A1*2</f><v>2</v></c></row><row r="2"><c r="B2"><f>A1*3</f></c></row></sheetData>
```

修正案: index と同じ関数と条件で読む。m1 もこれで閉じる。

```suggestion
                        row_number = r.as_deref().and_then(ooxml_core::value::parse_u32).filter(|r| *r >= 1);
```

テストに `<row r=" 1">…<c r="B2">` と `<row r="&#9;1">` を足す。

### M2（Major、#58 側で対応）: Phase4 の範囲操作は `census.block` を見ない

`impl/xlsx-phase4` の `range.rs`: `plan_merge`（L84）、`unmerge`（L228）、`set_autofilter`（L267）、`remove_autofilter`、`protect_sheet`、`unprotect_sheet` は `check_poisoned()` しか呼ばない。

#67 を `impl/xlsx-phase4`（`879fe76`）にローカルでマージして試した（衝突なし、プローブ `pr67_p4.rs`）。

```text
refusal=Some(InvalidReference("xl/worksheets/sheet1.xml: B2 in row 1"))
merge A2:B3 -> Ok(())
autofilter -> Ok(())
<sheetData><row r="1">…<c r="B2"><v>2</v></c></row></sheetData><autoFilter ref="A1:B3"/><mergeCells count="1"><mergeCell ref="A2:B3"/></mergeCells>
```

`filled_cells` は row index で行 2 を探すので、行 1 にある B2 を見つけない。そのため `MergeWouldHideValues` が出ずに結合され、B2 の値が隠れる。`edit_refusal()` の doc（「every edit of this workbook is refused」）とも合わない。

`MissingReference` など既存の block でも同じ穴があるので、原因は #58 にある。#58 の `plan_merge` と各 `pub(crate)` 入口で `census.block` を見るか、ADR-0014 で対象外と決めて `edit_refusal` の doc を直す。#67 の判定はこれで止めないが、どちらかがマージされる前に #58 に指摘を移すこと。

### m1（Minor）: `row r="0"` の拒否がワークブック全体に広がり、文言も誤解を招く

`<row r="0"><c r="A1">` は、main ではそのシートの編集だけが `InvalidReference("0")` で拒否されていた。PR では `InvalidReference("…: A1 in row 0")` でワークブック全体が拒否される。行 0 は不正な行番号で、セルが別の行を指しているわけではない。M1 の修正案（`filter(|r| *r >= 1)`）で、index の `BadRowR` と同じ扱いに戻る。

### n1（nit）: `c/@r` を 2 回 parse している

`finish_cell`（L403）でも同じ `c.r` を `CellIndex::parse` している。不一致の検査を `finish_cell` に移せば 1 回で済む。コストは 1M セルで 17 ms 程度なので、急がない。

### n2（nit）: コーパステストの assert が弱い

`no_fixture_is_refused_by_the_row_check` は `InvalidReference` でないことだけを見る。block は先勝ちなので、先に別の block がある fixture では検査が働かない。`EditBlock::RowMismatch` を直接見られる unit test か、main と同じ `edit_refusal` になることを見るほうが確実。

## 判定

**converged: no**（M1 が残る）。M1 と m1 は 1 行の修正とテストで閉じる。M2 は #58 のレビューに移す。
