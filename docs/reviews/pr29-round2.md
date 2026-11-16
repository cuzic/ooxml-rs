# PR #29 レビュー（Opus, round 2）

対象: `perf/xlsx-row-index` @ `7df471f`。round 1 の後に入った変更は次の 3 つである。
- `668437c`（m1–m4, m6 の修正）
- `impl/xlsx-phase1` の merge 2 回（`89d6dbb`, `7df471f`。O3 の期待値と core `remove_part` の round 5）

比較の基準（old）は `impl/xlsx-phase1` の head `0298520` である。index を持たない、現行の Phase1 にあたる。
検証はすべて scratch 上の複製で行った。
- 場所: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr29-r2/`
- 使ったテスト（`keep/` に保存）:
  - `zz_review_diff2.rs`: round 1 の差分テストを拡張したもの。
  - `zz_review_new_wb2.rs`: `Workbook::new` 経路の検査。
- m4 の検証では、`m4/` に `write_cell` を途中で失敗させる改変を入れて確かめた。

## 結論（先に）

**収束。** round 1 の m1、m3、m4、m6 は解消し、m2 は doc で明記された。新しい Blocker/Major/Minor はない。

- **差分テスト（拡張版）**: 3,353 シナリオ × 40 ops = 134,000 ops を流した。
  - 毎回の操作の後に検査し、維持された index は開き直した workbook と全件一致した。`cells()` と `cell()` も全件一致した。
  - old と new のトランスクリプト（操作結果、全 probe の読み取り、最終パッケージ全 entry の CRC）は **3,353 / 3,353 がバイト単位で一致**した。
- **round 1 の差分テスト（元のまま）**: 3,153 シナリオ × 40 ops を流した。old と分かれたのは syn2291 の 1 件だけで、これは m2 として doc に書かれた報告順の違いである。round 1 で分かれていた m1 由来の 18 件はすべて消えた。
- **セルの挿入（m3）**: `insert_at` を使う新しい実装は、旧 `insert_by` と位置も拒否も一致した。extLst が cell より前にある行の拒否も同じである。
- **core**: このブランチの core `insert_at` は、#26 の最新（`631fc90`、F1–F5 修正後）と同じものである。

## 解消状況

| round 1 | 状態 | 確認内容 |
| --- | --- | --- |
| m1 AC で剥がしが拒否される | **解消** | AC を `SheetIndex.alternate` と `RowEntry.alternate` に分けて記録するようにした。剥がしは `rows()` と `cells()` を使い、AC を見ない。対象セルの読み書きと `cells()` は `target_rows` と `target_cells` を使い、従来どおり拒否する。付属テスト `stripping_ignores_alternate_content_the_target_refuses_it` は、sheetData の AC と行の AC の両方を検査している。round 1 の差分テストで m1 由来の差異は 0 件になった。 |
| m2 エラーの報告順 | **解消（doc）** | `rows()` の doc に、報告順が full scan と異なり得ることを明記した。`index.rs` 冒頭の「exactly as a full scan」も直した。差分テストで残った差異は syn2291 の 1 件だけで、これがその文書化された差である。どちらの場合も拒否される点は同じで、変更は起きない。 |
| m3 hot path に O(n) が残る | **解消** | 詳しくは下の「m3 の同値性」を参照。<br>実測（release、load 約 32）: 1 行 16,384 セルの昇順が 0.13 s（round 1 では 15.0 s）、降順が 0.56 s。<br>40k 行の降順挿入は 6.3 s（他テストと並列で計測）で、これは README の新しい記述どおり O(後続の行数) のコストである。<br>100k セルの書き込みは 0.79 s、1M セルに対する 1,000 回の編集は 0.01 ms/edit。 |
| m4 `write_cell` 失敗時の index | **解消** | `Err` のときに `SheetIndex::build` で作り直す（`edit.rs:343-355`）。<br>検証: 複製に、新しいセルを挿入した直後に `Err` を返す改変を入れた。6 つの位置（行内の先頭・途中・末尾、新しい行を間・先頭・末尾）で試し、poisoned 状態の `cells()` と `cell()` は、開き直した workbook と一致した。 |
| m5 メモリ | 対応不要（nit） | `RowEntry` に `alternate`、`cells_end`、`misordered` が増えた。増加は行あたり十数 B である。 |
| m6 付属テストが狭い | **解消** | 次のテストが追加された。<br>- `random_writes_of_every_kind_match_a_fresh_open`: 400 ops。数値・文字列・数式・数式の除去を、シートをまたぐ dependent も含めて書き込み、開き直した workbook や `cell()` と比べる。<br>- `new_cells_at_indexed_positions`: extLst の前、セルの間、両端への挿入と、misordered 行の拒否を検査する。<br>- perf に `wide_row_and_descending_rows` が追加された。 |
| 関連 nit（`plan_target` の `&mut`） | 未対応（任意） | 現状のままで問題ない。 |

### m3 の同値性（新しいセルの位置と拒否）

`CT_Row` の子は `c*`、`extLst?` の 2 slot だけで、wildcard はない（`sml.rs` の生成コード）。そのため、旧 `check_insertable::<Row>` と `insert_cell_by` の振る舞いは、次の 4 つで完全に決まる。

| 旧の振る舞い | 新の対応 | 一致 |
| --- | --- | --- |
| 行内に AC があれば拒否する | `target_cells` が `RowEntry.alternate` で拒否する（同じ `UnresolvedSibling(row)`） | ○ |
| extLst の後に c があれば拒否する | `misordered` で拒否する（同じエラー） | ○ |
| 挿入位置は、列番号が大きい最初の c の前 | `cells.get(i).1`（index の昇順が保証されている） | ○ |
| そのような c がなければ、最初の extLst の前。それもなければ末尾 | `cells_end`（最後の c より後で最初の extLst、なければ `children.len()`） | ○ |

- 未知要素・空白・コメントが挟まる場合も、位置は旧と一致した。差分テストの生成器には、`<x:bar/>` と extLst の組み合わせ、`<extLst/><extLst/>`、extLst が c より前にある行を入れてある。
- `cell_inserted` の `cells_end >= item` による更新も正しい。追記するときは `item == cells_end` になり、extLst が 1 つ後ろへずれる。新しい行では 0 → 1 になる。
- 計画段階の `check_insert_at` は local な検査である。隣接要素はどちらも名前が一意（c か extLst）なので、#26 F1 の「all」判定は旧と同じ結果になる。cell の slot には limit がないので、F2/F3 の `place()` 経路は通らない。

## 実施した検証

| 項目 | 結果 |
| --- | --- |
| `cargo test --workspace` | 30 binaries、passed 231、failed 0、ignored 9（timing 7、既存 2） |
| `cargo test -p ooxml-xlsx --features fault-injection` | 15 binaries、passed 125、failed 0 |
| O3（`phase1_o3`、ignore を外した LibreOffice 期待値） | 2 passed |
| `cargo clippy --workspace --all-targets -- -D warnings`（`fault-injection` 付きも） | 警告なし |
| 拡張差分テスト `zz_review_diff2`（合成 3,200 + fixture 51 × 3 seed、各 40 ops、毎回の操作の後に開き直しと比較） | 134,000 ops（成功 103,901、拒否 30,099）。index と開き直し、`cells()` と `cell()`、同一ノード性のすべてで不一致 0、panic 0。old とのトランスクリプトは 3,353 / 3,353 がバイト単位で一致 |
| round 1 の差分テスト（元の生成器、3,153 シナリオ） | old とは 3,152 件が一致。残る 1 件（syn2291）は m2 |
| fault injection: 6 step × 拡張生成器（合成 600 + fixture） | poisoned になった 3,037 件すべてで、index が開き直した workbook と一致 |
| `Workbook::new` 経路（399 seed × 600 書き込み、300 行 × 120 列にランダム） | old と new の sheet XML がバイト単位で一致 |
| m4 の失敗注入（改変した複製） | 6 位置すべてで、poisoned の読み取りが開き直した workbook と一致 |
| #26 の取り込み | `origin/core/insert-at`（`631fc90`）は HEAD の祖先である。`crates/ooxml-core/src/lens.rs` は同ブランチの head と差分なし |

拡張差分テストで round 1 に追加したもの:
- **生成するシート**:
  - 幅の広い行（1/8 の確率で 10〜89 セル）
  - 行末の extLst。未知要素や空白を挟む場合、extLst が 2 つある場合を含む。
  - extLst が c より前にある行
  - 欠陥のないシートにも AC（行、sheetData）を低い確率で入れる。
- **操作**:
  - 1 行の中で列を降順に書く。
  - 行を降順に挿入する。
  - 1 行の末尾に昇順で追記し続ける（最大 16,000 列まで）。

## 指摘

新しい指摘はない。

参考（PR の範囲外）:
- 欠陥のある合成シートで、ある行が別の行を指すセル（例: row 12 の `B13`）を持つ場合がある。そのシートへの数式書き込みが、`malformed workbook: … the formula census and the tree disagree about B13` で拒否された（2 シナリオ）。
- old でも同じ文言、同じタイミングで起きる。拒否されるだけで poisoned にはならず、変更も起きない。
- ほかの経路では同じ欠陥が `InvalidReference("B13 in row 12")` として報告される。文言を揃えるかどうかは Phase1 側の判断でよい。

## 判定

**converged: yes**

round 1 の Minor はすべて解消した（m2 は doc での明記）。拡張した差分テストでも、index の整合性と旧実装との同値性に反する例は見つからなかった。merge してよい。
