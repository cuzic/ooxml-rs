# PR #67 レビュー（Opus, round 2）

対象: `fix/xlsx-f8-row-mismatch` @ `ff88edd`（round 1 は `6c16a51`）。fuzz finding F-8（`c/@r` が `row/@r` と別の行を指すセル）への対応。
確認は scratchpad の `git clone --reference` 作業コピーで、使い捨ての integration test（コミットしない）を使って行った。

## 結論（先に）

**未収束（Major 1 件、Minor 1 件）。** round 1 の M1・m1・n1・n2 は挙動で解消を確認した。census は row index と同じ文字列（どちらも `normalized_value`）を同じ関数（`parse_u32` と `>= 1`）で読むようになり、`row/@r` の書き方（先頭 0、`+1`、タブ・改行、`&#32;` などの文字参照、u32 の上限前後）で両者の判断が食い違う入力は見つからなかった。
残る穴は、`row` の中で `mc:AlternateContent` に包まれたセルだけ（M1）。census はこのセルを見ないので、`edit_refusal()` が `None` のまま `set_formula` が 2 つ目の `c r="B2"` を書く。`row_number` を `</row>` で戻さないため、`sheetData` 直下の AC で包まれた正しい行が誤って F-8 と判定される問題もある（m1）。どちらも census.rs の数行で直り、手元で直した版で両方の入力と既存テストが通ることを確かめた。

## 前ラウンドの解消状況

- M1（`row/@r` の空白）: 解消。`" 1"`、`"&#9;1"`、`"&#10;1"`、`"&#13;1"`、`"1&#9;"`、`"&#x20;1&#x20;"`、`" +1 "` はどれも `InvalidReference("…: B2 in row 1")` で全編集を拒否し、`Err` の後の出力は入力とバイト単位で同一。
- m1（`row r="0"`）: 解消。`"0"`、`"-1"`、`"1.0"`、`"4294967296"` は `edit_refusal() == None`、そのシートだけ `BadRowR`（`InvalidReference("<値>")`）、別シートは編集できる（main と同じ）。
- n1（`c/@r` の二重 parse）: 解消。`CellScan.at` に一度だけ parse した結果を持ち、`finish_cell` はそれを使う。
- n2（コーパステストの assert）: 解消。`census::tests::no_fixture_sheet_has_a_row_mismatch` が part ごとに `scan_sheet` を呼び、`EditBlock::RowMismatch` を直接見る。
- M2（#58 の範囲操作が `census.block` を見ない）: 本 PR の範囲外（#58 側）。

## 確認できたこと（指摘ではない）

| 入力 | 結果 |
|---|---|
| `row r="01"`、`"00000000000000000001"`、`"+1"` + `c r="B2"` | 全編集を拒否、`Err` 後もバイト単位で同一 |
| `row r="4294967295"` + `c r="B2"` | 拒否（`B2 in row 4294967295`）。index も同じ値を読み、`WrongRow` になる |
| `row r="1048577"` + `c r="A1048577"` | 既存の `FormulaScanFailed`（`CellIndex::parse` が範囲外を拒否）。index と同じ |
| `r` の無い row（先頭、または `row r="1"` の後） | `edit_refusal() == None`、そのシートは `MissingReference(row)`、別シートは編集可（main と同じ） |
| 行の順序違い（`row r="3"` の後に `row r="1"` + `B2`） | 拒否。census は行ごとに検査を続ける |
| `r` の無い c の後に `c r="B2"`（行 1） | 拒否。`r` の無い c だけなら不一致にならない |
| `c r="$b$2"`（行 1） | 拒否（`B2 in row 1`） |
| 名前空間の違う `x:row`（`mc:Ignorable` なし）の中の `c r="B2"` | census も index も見ない。SML の行ではなく、Excel はこの入力を開かないので対象外 |
| 他の open PR | `census.rs` に触るのは #67 だけ |

## Findings

### M1（Major）: `row` の中の `mc:AlternateContent` に包まれた不一致セルを census が見ず、重複セルを書く

`crates/ooxml-xlsx/src/census.rs` L257 の `"c" if parent == Some("row")` は、親が `mc:Choice`/`mc:Fallback`（stack 上は `""`）の `c` を飛ばす。index の `row_cells` も行の直接の子だけを見る。行 2 への書き込みは行 1 の AC を見ないので、`target_cells` の `UnresolvedSibling(row)` にも当たらない。

```text
<row r="1"><c r="A1"><v>1</v></c><mc:AlternateContent …><mc:Choice Requires="x14"><c r="B2"><v>2</v></c></mc:Choice></mc:AlternateContent></row>

refusal=None
B2.set_formula("A1*3") = Ok(())
→ <row r="1">…<mc:Choice …><c r="B2"><v>2</v></c></mc:Choice>…</row><row r="2"><c r="B2"><f>A1*3</f></c></row>
```

Choice が選ばれると B2 が 2 つになる。F-8 と同じ出力で、README の「a `c/@r` naming another row … **anywhere in any sheet** refuses **every edit**」とも合わない。main でも同じ挙動なので退行ではないが、この PR が閉じると言っている類の入力。
包まれたセルに `f` があれば既存の「f outside worksheet/sheetData/row/c」で `FormulaScanFailed` になるので、穴は値だけのセル。

- 修正案: 行の中の AC に包まれた `c` にも同じ不一致検査をかける。手元で次を足すと、上の入力は `InvalidReference("…: B2 in row 1")` で全編集を拒否し、`row_mismatch` テスト 6 件と lib テスト 29 件も通った。テストに上の入力を足す。

```rust
                    // a c wrapped in mc:AlternateContent inside a row
                    "c" if parent != Some("row") && stack.iter().any(|s| s == "row") => {
                        if row_mismatch.is_none()
                            && let (Some(row), Some(Ok(at))) = (row_number, attr(e, "r")?.as_deref().map(CellIndex::parse))
                            && at.row + 1 != row
                        {
                            row_mismatch = Some((at.to_string(), row));
                        }
                    }
```

### m1（Minor）: `row_number` が `</row>` の後も残り、AC に包まれた正しい行を F-8 と誤判定する

`row_number` は `sheetData` 直下の `row` の開始でしか更新されない。`sheetData` 直下の AC に包まれた行（`<mc:Choice><row r="2"><c r="B2">`）の c は、親が `row` なので検査され、前の行の番号（1）と比べられる。

```text
<row r="1"><c r="A1"><v>1</v></c></row><mc:AlternateContent …><mc:Choice Requires="x14"><row r="2"><c r="B2"><v>2</v></c></row></mc:Choice></mc:AlternateContent>

main: refusal=None、Sheet1 は UnresolvedSibling(sheetData)、Sheet2 は編集可
PR:   refusal=Some(InvalidReference("…: B2 in row 1"))、Sheet2 も拒否
```

安全側の過剰拒否だが、拒否がシート単位からワークブック全体に広がり、文言（`B2 in row 1`）も事実と違う。

- 修正案: `Event::End` で `sheetData` 直下の `row` を閉じたときに `row_number = None` に戻す。M1 の修正と合わせて試し、この入力は main と同じ挙動に戻った。

```rust
                if local == "row" && stack.last().map(String::as_str) == Some("sheetData") {
                    row_number = None;
                }
```

## 判定

converged: no（Blocker 0 / Major 1 / Minor 1 / nit 0）
