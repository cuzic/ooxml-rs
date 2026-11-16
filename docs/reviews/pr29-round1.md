# PR #29 レビュー（Opus, round 1）

対象: `perf/xlsx-row-index` @ `973a6c1`。比較の基準は親の `bf86f0d` で、PR #20 `impl/xlsx-phase1` と core PR #26 `insert_at` を merge 済みのものを使った。
差分は `index.rs`（新規）、`edit.rs`、`worksheet.rs`、`workbook.rs`、`tests/row_index.rs`、`tests/perf.rs`、README の 7 ファイルである。
検証はすべて scratch 上の複製で行った。2 本の木を使い、`new` は worktree の複製、`old` は `git archive bf86f0d` である。
- 場所: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr29/`
- 使ったテスト（`keep/` に保存）:
  - `zz_review_diff.rs`: 差分テスト。
  - `zz_review_repro.rs`: m1 の再現。
  - `zz_review_new_wb.rs`: `Workbook::new` 経路での old/new 比較。
  - `perf_new_with_probes.rs`: RSS の計測と、不利な入力での計測。

## 結論（先に）

**収束（Blocker/Major なし、Minor 4 件と nit 2 件）。**

index の整合性は、試せた変更経路のすべてで保たれていた。対象は Phase0 の書き込み、Phase1 の数式書き込み、dependents の剥がし、shared formula の hand-off、行間への新しい行の挿入、`sheetData` の新規作成、apply 失敗後の poisoned 状態である。どの経路でも、新しく開き直した workbook と一致した。`cells()` も、セルごとの読み取りと同じノードを同じ順序で返した。

T2/L1/CR、Phase0、Phase1（O1–O3、review_r3）のテストは、すべてそのまま通る。old と new で出力のバイト列も一致した。

食い違いが出たのは 1 系統だけである（m1）。**dependents の剥がしが、他シートの `mc:AlternateContent` を新たに拒否するようになった。** 以前は成功していた編集が `UnresolvedSibling` で拒否される。変更は起きないので Minor だが、「以前と同じ検証」という説明（commit、README、`index.rs` の doc）とは合わない。

性能の主張は、負荷の高いホスト上でも再現した。

## 実施した検証

| 項目 | 結果 |
| --- | --- |
| `cargo test --workspace`、`cargo test -p ooxml-xlsx --features fault-injection` | 全通過（44 binaries、failed 0） |
| `cargo clippy --workspace --all-targets -- -D warnings`（`fault-injection` 付きも） | 警告なし |
| 差分テスト（下記）: 計 3,153 シナリオ × 40 ops = 126,000 ops（成功 106,318、拒否 19,682） | 毎回の操作の後に検査した。**維持された index と開き直した workbook は全件一致**。`cells()` も、開き直した側の `cells()` や `cell()` と全件一致 |
| old と new のトランスクリプト比較（操作の結果、毎回の probe 読み取り、最終パッケージ全 entry の CRC） | 3,131 / 3,153 シナリオがバイト単位で一致。残り 22 件は m1/m2 だけ |
| `Workbook::new` 経路（39 通りのランダム書き込み列） | old と new で sheet XML がバイト単位で一致 |
| fault injection: `ApplyStep` 6 種 ×（合成 600 + fixture 51） | 3,269 個の poisoned workbook すべてで、index は木と一致（`package().to_bytes()` から開き直したものと比べた） |

差分テストの内容:
- **対象**: fixture 51 個（`all_zip_fixtures` と `synthetic/*.xlsx`）を、それぞれ 3 seed で動かした。加えて、ランダムに生成した合成 workbook を 3,000 個使った。
- **合成 workbook の中身**: 1〜3 シートで、次のものを含む。
  - 空白・コメント・未知名前空間の要素（`<x:foo/>`）が行やセルの間にある。
  - 自己終了の `<row/>`、行番号や列の飛び。
  - plain/shared formula、シートをまたぐ参照。
  - 3 分の 1 には欠陥を入れた。`r` の欠落、行の順序違い・重複、セルの順序違い・重複、別の行を指すセル、sheetData・row・セルに置いた AC である。
  - `sheetData` がないもの、`<sheetData/>` だけのもの。
- **操作**: `set_number`、`set_string`、`set_formula`（同じシート・別シートを参照）、`remove_formula`、`CalcPolicy` の切り替え。
- **書き込み位置**: 既存セル、既存の行の新しい列、既存の行の間・前後、遠い位置（20 万行先）。

## 指摘

### m1 [Minor] dependents の剥がしが、他シートの `mc:AlternateContent` を拒否するようになった（以前は成功していた）

**根拠**
- old の `strip_positions` が検査したのは、行・セルの `r`、順序、行の不一致だけである。AC は検査しなかった。
- new では `strip_positions` が index の `rows()?`（`edit.rs:425`）と `cells(e)?`（`edit.rs:446`）を使う。index の構築時には、対象セルを読むための AC 拒否も記録される。
  - `index.rs:100` `if has_ac(sd)` → `SheetErr::Alternate("sheetData")`
  - `index.rs:73` → `RowErr::Alternate`
  - そのため、**他シート**の剥がしにもこの拒否が適用される。
- 再現（`keep/zz_review_repro.rs`）。S2!A1 は `S1!A1+1` で、cache を持つ。S1!A1 に 5 を書く。

  | S2 の状態 | old | new |
  | --- | --- | --- |
  | sheetData に AC | `Ok(())` | `Err(UnresolvedSibling(sheetData))` |
  | dependent の行に AC | `Ok(())` | `Err(UnresolvedSibling(row))` |

- 差分テストでも、合成 22 シナリオのうち 18 件がこの理由で old と分かれた。
- 拒否の文言は "cannot position a new child of sheetData" である。しかし、利用者が編集したのは別のシートのセルで、要素の挿入もしない。文言として誤解を招く。

**判断材料**
- census は AC の中の `c/f` を数えない。したがって、AC の中にある数式の cache は old でも剥がされていなかった。PR 以前からある古い cache の穴である。
- この点で new の拒否は、Phase0 F6/N3 の考え方と整合するとも言える。ただ、意図した変更として書かれていない。

**提案**
- どちらかに決めて明記する。
  - (a) 剥がしでは old と同じ検査にとどめる。index に AC フラグを別に持ち、`strip_positions` では見ない。
  - (b) 拒否を Phase1 の仕様にする。その場合は専用の error か文言を用意し、テストを付ける。README の「the same rules as before」も直す。

### m2 [Minor/nit] 剥がしで報告するエラーの優先順位が変わった

**根拠**
- old は文書順に走査し、訪問した行の行内エラーをその場で返していた。
- new は次の順に報告する。
  1. シート単位のエラー（AC、`r` の欠落、行の順序違い）を先に返す。
  2. 行内では AC をセルの順序より先に返す。
- 差分テストでの例:
  - syn2291: old は `invalid cell reference "F7 in row 6"`、new は `rows in xl/worksheets/sheet2.xml are not in ascending order`。
  - syn1300: old は `cells in row 15 … not in ascending order`、new は row の `UnresolvedSibling`。
- どちらの場合も拒否される点は同じで、変更は起きない。対象セル側の `locate` は old も全行を先に走査していたので、順序は old と同じである。
- `index.rs` の doc は「exactly as a full scan finds it」と書いているが、剥がしについては成り立たない。doc の 1 行を直せば済む。

### m3 [Minor] hot path に O(n) が残っている（行内のセル挿入、行の途中への行挿入）

**根拠**（`keep/perf_new_with_probes.rs` の `review_adversarial`、release）

| 入力 | old | new |
| --- | --- | --- |
| 1 行に 4,000 セルを昇順に書く | 1.08 s | 0.89 s |
| 1 行に 16,384 セルを昇順に書く | 未計測 | **15.0 s** |
| 10k 行を昇順に書く | 9.3 s | 0.062 s |
| 10k 行を降順に書く | 5.7 s | 0.24 s |
| 40k 行を昇順に書く | 未計測 | 0.22 s |
| 40k 行を降順に書く | 未計測 | 3.39 s |

- **行内のセル挿入は改善していない。** 計画時に `check_insertable::<Row>(row.elements(), …)`（`worksheet.rs:250`）を、適用時に `insert_cell_by`（`worksheet.rs:332`）を使う。後者は行内の全セルについて `c/@r` を `CellIndex::parse` する。そのため、行の幅に対して二乗のコストになる。
  - index はすでに挿入位置（`cells.partition_point`）を知っている。行と同じく `check_insert_at` と `insert_at` を使えば、O(1) とメモリ移動だけで済む。
- **行の途中への挿入は、まだ二乗のコストである。** `row_inserted` は挿入位置より後ろのすべての `RowEntry.item` を +1 する（`index.rs:177-184`）。木の側の `Vec::insert` と同じ程度の量なので、大幅に改善したうえでの残りである。
- `index.rs` 冒頭の「Lookups are binary searches: O(log …)」は lookup については正しい。挿入のコストは別に一言書いておくとよい。

### m4 [Minor/nit] `write_cell` が途中で失敗したとき、index を作り直しも破棄もしない

**根拠**
- `edit.rs:343-345` では、`write_cell(...)?` が `Err` を返すと `effect.update` に到達しない。`insert_at` の後に失敗した場合、木は変わっているのに index は古いままになる。
- workbook は poisoned になるが、`Worksheet::cell` と `cells()` は poisoned でも読める（`check_poisoned` を呼ばない）。古い index で `children()[ri]` などを引くと、範囲外アクセスの panic になるか、別のセルを返し得る。
- 現状では到達しない。plan が成功を保証しており、fault point は `write_cell` の外にある。6 step すべての注入で、index は木と一致した。

**提案**
- `write_cell` が `Err` を返したら、その場で `SheetIndex::build` で作り直す。
- `remove` は不可である。`Worksheet::index()` が `expect` するので、同じハンドルで読むと panic になる。

### m5 [nit] index のメモリ

| 項目 | 値 |
| --- | --- |
| `RowEntry` | 48 B |
| セル 1 個（`(u32, usize)`） | 16 B |
| 参考: `RawItem` / `RawNode` | 144 B |
| 1M セル × 100k 行の理論値 | 約 21 MB |
| VmRSS 実測 | old 1,690 MB → new 1,722 MB（+31 MB、+1.8%） |

- 許容範囲である。
- 位置を `u32` で持てばセルあたり 8 B に半減するが、任意である。

### m6 [nit] PR 付属の `row_index.rs` は検査範囲が狭い

**根拠**
- ランダム検査は 1 fixture の 40×12 の範囲で、`set_number` しか使わない。
- 次のものは検査していない。
  - string・formula の書き込み
  - `sheetData` の新規作成
  - Phase1 の剥がしと hand-off
  - 欠陥のあるシート
  - `cells()` と `cell()` の同一ノード性
- `zz_review_diff.rs` の形（維持した workbook と開き直した workbook の比較、`cells()` と `cell()` の比較）を縮めて取り込むとよい。

関連する設計上の nit: `plan_target` が `&mut XmlDocument` を取るのは、`check_insert_at` のためだけである。core の `check_insert_at` は `LensMut` を要求するが、`Lens` 版があれば計画段階の型が読み取り専用のまま保てる。

## 問題がなかった点（確認済み）

**old の走査と同じ意味になっていること（対象セルの読み書き）**
- `MissingReference`（`r` のない行、対象行の `r` のないセル）、`OutOfOrder`（行・セル）、`InvalidReference`（不正な `r`、別の行を指すセル）は、種類も文言も一致した。
- AC の拒否は、`worksheet`（`sheetData` がない場合）→ `sheetData` → 行 → セル/`is` の順で、old と同じである。
- 他の行の欠陥を無視する点も同じである。
- 重複した行・セルは `OutOfOrder` になる。

**位置の一致**
- 新しい行の位置（既存の行の直前、または `children.len()`）は `insert_by` の `place` と一致した。
- 空白・コメント・未知要素を挟んでも、出力のバイト列は old と同じだった。

**index の更新**
- `row_inserted` と `cell_inserted` の更新は正しい。
- 剥がし（`v`、`is`、`t` の除去）と hand-off は、行・セルの位置を変えない。これらの位置と対象セルの位置は、どれも書き込み前の木で計画し、target より先に適用する。
- `sheetData` を新規作成する経路（`sheet_data_created` の後に `row_inserted(…, 0)`）も正しい。

**読み取り**
- `Lens::root(c)` を根にした `CellRef` で読んだ値・formula・属性は、全 fixture で old と一致した。

**計画段階の dirty**
- `xml_part_mut` と `lens_mut` を取っても dirty にはならない。そのため、拒否された編集でパッケージのバイトは変わらない（既存の `assert_unchanged` 系テストが通る）。

**性能（ホストの load は約 32〜36）**

| 計測 | old 実測 | new 実測 | commit の主張 |
| --- | --- | --- | --- |
| 1M セルをセルごとに読む | 7.19 ms/セル（200 セル抽出）、外挿で約 7,190 s | 0.52 s | 12,500 s → 0.77 s |
| 1M セルを一括で読む | — | 0.38 s | 0.59 s |
| 新しいシートに 100k セルを書く | 38.3 s | 0.30 s | 79 s → 0.59 s |
| 1M セルに 1,000 回のランダム編集 | 7.76 ms/edit | 0.0035 ms/edit | 9.6–14.9 → 0.005–0.01 ms |

- 主張の桁は再現した。
- ランダム編集は既存セルの上書きだけなので、挿入のコストは m3 を参照。

## 判定

**converged: yes**

Blocker/Major はない。m1 は振る舞いの変更なので、merge 前に (a) か (b) のどちらにするかを決めることを推奨する。m2〜m6 は後続の PR でもよい。
