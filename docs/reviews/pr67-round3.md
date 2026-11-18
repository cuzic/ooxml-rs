# PR #67 レビュー（Opus, round 3）

対象: `fix/xlsx-f8-row-mismatch` @ `a5d03ea`（round 2 は `ff88edd`）。fuzz finding F-8（`c/@r` が `row/@r` と別の行を指すセル）への対応。
確認は scratchpad の `git clone --reference` 作業コピーで、使い捨ての integration test（コミットしない）を使い、同じ入力を `origin/main` でも流して比べた。

## 結論（先に）

**収束（Blocker 0 / Major 0）。** round 2 の M1・m1 は挙動で解消を確認した。`row` の中にある `c` は、`mc:AlternateContent` の入れ子・Choice/Fallback のどちら側・`c` の中の AC・名前空間不明のラッパー（`mc:Ignorable` の有無を問わない）のどれに包まれていても census が見つけ、`edit_refusal()` で全編集を拒否する。`Err` の後の出力は、どの入力・どの編集でも入力とバイト単位で同一だった。
残りは、`mc:ProcessContent` 付きのラッパーで行やセルを index から隠す入力の重複セルで、main と同じ挙動の別問題（F-8 の範囲外）。follow-up とする。ほかに、空の `row` の扱い（`!empty`）を守るテストが無い（nit）。

## 前ラウンドの解消状況

- M1（`row` の中の AC に包まれた不一致セル）: 解消。round 2 の入力は `InvalidReference("…: B2 in row 1")` で、B1/B2/A1/C3 への `set_number`・`set_formula` がすべて `Err`、出力はバイト単位で同一（main は `Ok` で B2 が 2 つ）。`alternate_content_in_row_blocks_mismatched_value_cell` が追加されている。
- m1（`</row>` で `row_number` を戻さない）: 解消。`sheetData` 直下の AC に包まれた正しい行は `edit_refusal() == None`、Sheet1 は `UnresolvedSibling(sheetData)`、Sheet2 は編集可（main と同じ）。`alternate_content_in_sheet_data_only_refuses_its_own_sheet` が追加されている。

## 確認できたこと（指摘ではない）

各入力で B1・B2・A1・C3 に `set_number` と `set_formula` を試した。

| 入力（Sheet1） | PR | main |
|---|---|---|
| 行 1 の中の AC の中の AC（入れ子）に `c r="B2"` | 全編集拒否、`Err` 後も同一 | `Ok`、B2 が 2 つ |
| 行 1 の AC: Choice に `B1`、Fallback に `B2` | 全編集拒否、同一 | `Ok`、B2 が 2 つ |
| 行 1 の AC: Choice に `B2`、Fallback に `B1` | 全編集拒否、同一 | `Ok`、B2 が 2 つ |
| `c r="A1"` の中の AC に `c r="B2"` | 全編集拒否、同一 | `Ok`、B2 が 2 つ |
| 行 1 の中の名前空間不明の要素（`q:w`、`mc:Ignorable` なし／`Ignorable="q"`／`Ignorable` + `ProcessContent="q:w"`）に `c r="B2"` | 全編集拒否、同一 | `Ok`、B2 が 2 つ |
| `sheetData` 直下の AC が行 1・行 2 を包み、行 1 に `B2` | `edit_refusal()==None`、Sheet1 は `UnresolvedSibling(sheetData)`、同一 | 同じ |
| 行 1 の後に、AC に包まれた 2 つ目の `row r="1"`（`B2`） | 同上 | 同じ |
| `<row r="1"/>`（空）の後の行 2 の AC に `c r="B3"` | 全編集拒否（`B3 in row 2`）、同一 | `Ok` |
| 他の open PR | `census.rs`・`index.rs`・`row_mismatch.rs` に触るのは #67 だけ | — |

## Findings

### n1（nit）: 空の `row` で `row_number` を更新しない分岐（`!empty`）を守るテストが無い

- 根拠: `crates/ooxml-xlsx/src/census.rs` L255 の `if !empty {` を `if true {` に変えても、`row_mismatch` 8 件と lib 29 件はすべて通る。変えると `<row r="5"/>` の後の `sheetData` 直下 AC に包まれた `row r="2"` の `c r="B2"` が行 5 と比べられ、ワークブック全体の過剰拒否になる（安全側）。
- 修正案: `<row r="5"/>` の後に `sheetData` 直下の AC で包んだ `row r="2"`（`c r="B2"`）を置く入力で `edit_refusal() == None` を確かめるテストを 1 件足す。

## 範囲外（follow-up）

`mc:Ignorable="q" mc:ProcessContent="q:w"` の `q:w` で行やセルを包むと、index（`index.rs` の `has_ac` は `mc:AlternateContent` しか見ない）がその中身を見ず、`Ok` で重複を書く。main と同じ挙動で、行番号の不一致が無くても起きるので F-8 ではない。

```text
<row r="1"><c r="A1"/><q:w><c r="B1"><v>2</v></c></q:w></row>        → B1.set_number = Ok、B1 が 2 つ
<row r="1">…</row><q:w><row r="2"><c r="B2">…</c></row></q:w>       → B2.set_number = Ok、行 2 が 2 つ
<q:w><row r="1"><c r="A1"/><c r="B2"/></row></q:w>                   → A1・B2 とも Ok で重複
```

`ProcessContent` を消費者が処理すると中身が有効になるので重複セル・重複行になる。対応は「`mc:ProcessContent` を宣言したシート（または `row`/`sheetData` の子に SML 以外の要素がありそれが ProcessContent 対象）は `UnresolvedSibling` 相当で拒否する」が最小。別 Issue にする。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 1）
