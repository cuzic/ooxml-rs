# PR #26 レビュー（Opus, round 1）

対象: `core/insert-at` @ `31827b5`（`LensMut::insert_at` / `check_insert_at`、`Error::InsertIndexOutOfRange`）。
照合したもの: `lens.rs` の `insert_by` / `place` / `insert_at_slot`、`tests/lens.rs`、README、呼び出し元（`perf/xlsx-row-index` の `worksheet.rs:234`、`:323`）。
プローブと mutant は scratchpad（`review-14r5-26/pr26/`）の作業コピーで実行した。リポジトリは変えていない。

- `probe_r1.rs`: 総当たり。3 つのスキーマ（テストの `P`、`Tbl`（`mark` が 2 つの slot にある）、新しく作った `Lim`（`m*` と `(m|z){0..1}`））について、子の列（長さ 3〜5、空白・コメント・未登録要素・wildcard・`mc:AlternateContent` を含む）と挿入する型と index のすべての組を試した。計 50,614 件。各件で次を比べた。
  - `check_insert_at` と `insert_at` の結果（Ok か、Err の種類と文言）。
  - Err のとき、木が変わっていないか（`serialize` と `is_dirty`）。
  - Ok のとき、結果が正しいか。オラクルは、AC の各分岐を展開したうえで、`place` と同じ貪欲な slot 割り当てと上限を確かめる。
  - Err のとき、実は正しい挿入ではなかったか（過剰な拒否）。
  - `insert_by` が Ok になる場合に、同じ結果を出せる index があるか。
- `probe_r1b.rs`: 深さの上限、名前空間の prefix、dirty の義務、計算量。
- `mutants.py`: 17 個の mutant。

## 結論（先に）

**未収束（Major 1 件）。**

- 曖昧な QName（2 つの slot にある名前）が隣にあると、近傍だけの順序検査が**スキーマ上不正な木を `Ok` で作る**（F1）。
- 基本の設計（「listed な最も近い隣だけを見る」）は、曖昧な名前がなく AC もない親では正しい。総当たりで、不正な結果も過剰な拒否も 0 件だった。
- `check_insert_at` と `insert_at` は 50,614 件のすべてで一致した。Err の後に木が変わる経路はなかった。
- 今の呼び出し元（`sheetData` の `row`、`row` の `c`。どちらも上限のない単一の slot）は F1〜F3 に当たらない。生成済みのスキーマ（sml、pml、dml。`Slots` を持つ要素 47 個）にも曖昧な QName は 0 件である。ただし core の汎用の API であり、WML（`w:tbl` の範囲マーカーなど）で当たる。テストの `Tbl` は、まさにその形を模している。

ほかに Minor が 4 件ある。

- F2: AC の中身を上限の計数で見落とす。
- F3: 曖昧な名前を上限で数えすぎて、正しい挿入を拒否する。
- F4: mutant 17 個のうち 8 個が生き残る。とくに `insert_at` の深さの上限にテストがない。
- F5: 「O(1)」の説明が、上限のある slot には当てはまらない。

F1〜F3 は、修正 1 つ（下記）でまとめて片付くことを、作業コピーで確かめた。

## 指摘

### F1 [Major] 曖昧な QName の隣では、近傍の検査が不正な順序を通す

**根拠**（`lens.rs:477-496`）
- 前の隣については「候補の slot の**どれか**が `slot` 以下」、後ろの隣については「**どれか**が `slot` 以上」を確かめている（`c.iter().any(..)`）。
- ところが、曖昧な名前が実際にどの slot に入るかは、`place`（`lens.rs:338-377`）の貪欲な割り当てで、その前の子によって決まる。候補のどれかが合うことは、必要条件にすぎない。
- 総当たりの `Tbl`（`mark* pr? (mark|row)*`）で、**430 件**が `Ok` なのに結果が不正だった。例:
  - `<tbl><mark/><pr/></tbl>` に `insert_at::<Row>(0)` → `row mark pr`。`mark` が slot 2 に押し出され、`pr` が後ろに来る。
  - `<tbl><row/><mark/></tbl>` に `insert_at::<Pr>(2)` → `row mark pr`。前の隣の `mark` は、候補 {0, 2} のうち 0 が `pr` の slot 以下なので通る。しかし、実際の割り当ては 2 である。
- `insert_by` / `insert_in_slot` は、同じ入力でこうした木を作らない（`place` が全体を割り当てる）。doc の「Schema order is checked locally」と README の「Schema order is checked against the nearest listed neighbours」は、この場合の保証を言い過ぎている。

**提案**
- 前の隣は候補の**すべて**が `slot` 以下、後ろの隣は**すべて**が `slot` 以上、という条件にする（`any` を `all` にする）。元の木が正しい順序なら、これで十分である。前の隣の割り当ては変わらない。後ろの隣の割り当ても「`slot` 以上の最小の候補」で変わらない。そのため、その後ろの割り当ても変わらない。
- 作業コピーでこの修正を試した。3 スキーマの 50,614 件で、不正な結果は 0 件になった。その代わり、`Tbl` で 1,238 件が安全側に拒否される（例: `<mark/><row/>` の間への `pr`）。
- 拒否を減らしたいなら、隣が曖昧なときだけ `place` に落とす方法もある（O(n) だが、曖昧な隣があるときに限られる）。どちらの場合も、doc に「曖昧な隣では `UnresolvedSibling`（`insert_by` / `insert_in_slot` を使う）」と書く。
- `Tbl` で `insert_at` を試すテストを足す。

### F2 [Minor] 上限の計数が、離れた `mc:AlternateContent` の中身を見ない

**根拠**（`lens.rs:470-475`、`497-509`）
- AC を拒否するのは、listed な最も近い隣が AC の場合だけである（意図した緩和で、doc にもそう書いてある）。
- しかし上限の計数は、AC を数えずに飛ばす。`insert_by` は、AC があれば親ごと拒否する（`lens.rs:350`）。
- 総当たりの `P` で **140 件**、AC のある親で `Ok` なのに、ある分岐で上限を超えた。例:
  - `<p><a/><AC Choice=a Fallback=b/></p>` に `insert_at::<A>(0)` → 3 個目の `(a|b)` になる（`max_total` 2）。
- doc の「slot limits are enforced as by `insert_by`」と食い違う。
- 実際に当たるには、上限が 2 以上の slot か、複数の名前を持つ上限つきの slot が必要である。さらに、AC がその slot のメンバーを包んでいて、挿入位置の隣にない必要がある。そのため Minor とする。
- 提案: 上限のある slot では、`place(slots, slot, C::QNAME, |_, _| false)` をそのまま呼ぶ。今の計数のループもすでに O(n) なので、計算量は変わらない。これで AC を全体で拒否し、貪欲な割り当てで数えるようになり、F3 も消える。作業コピーで確かめた（F1 の修正と合わせて、`P` の不正な結果が 0 件、既存の `lens` テスト 14 件も通る）。

### F3 [Minor] 曖昧な名前を上限で数えすぎて、正しい挿入を `AlreadyPresent` で拒否する

**根拠**（`lens.rs:501`）
- `candidates(..).contains(&slot)` で数えている。そのため、貪欲な割り当てでは別の slot に入っている曖昧な子まで数える。`place` は、割り当ての結果（`rank == slot`）で数える。
- `Lim`（`m*` と `(m|z){0..1}`）で 23 件が過剰に拒否された。
  - 例: `<lim><m/></lim>` に `insert_at::<Z>(1)` → `AlreadyPresent`。`m z` は正しい木であり、`insert_by::<Z>` は通る。
  - `insert_by` が Ok なのに、どの index の `insert_at` でも同じ結果を出せない場合が 17 件あった。
- 安全側の誤りなので Minor とする。F2 の修正（`place` を呼ぶ）で消える。

### F4 [Minor] テストの穴: mutant 17 個のうち 8 個が生き残る

`mutants.out` の結果:

| mutant | 結果 | 備考 |
|---|---|---|
| M1 AC の隣を拒否しない、M2/M3 `<=`→`<`・`>=`→`>`、M4/M5 前後の検査を外す、M9 `mark_path` を外す、M10 `>`→`>=`、M14 前を順方向に走査、M17 上限を外す | 検出 | |
| **M6** `max_each` の `>=`→`>` / **M7** `max_total` の `>=`→`>` | 生存 | テストの上限は `one`（each も total も 1）だけで、片方を壊しても他方が拒否する。`(a|b)` の `max_each 2` / `max_total 2` を `insert_at` で試すテストがない。 |
| **M8** `insert_at` の `own_dirty` を外す | 生存 | `lens.rs:337-340` のテストは `serialize` しか見ない。writer は子のある self-closing を直すので、出力では区別できない。`insert_by` のテスト（`self_closing_parent_gains_and_loses_children`）と同じく、`own_dirty()` を assert する。 |
| **M11** `check_insert_at` の深さの検査を外す / **M12** `+2`→`+3` | 生存 | **`insert_at` の深さの上限にテストがない。** `insert_at` には自分の深さの検査がない（`check_insert_at` の `lens.rs:460` だけ）。そのため M11 では、`TooDeep` を素通りして、parse し直せない木を作る。probe（`probe_r1b::depth`、`max_depth 4`）では今のコードは正しい。 |
| **M13** `check_insert_at` の prefix の検査を外す | 生存 | `NamespaceNotInScope` での `check_insert_at` と `insert_at` の一致（validate-then-apply の約束）にテストがない。M13 では、check が Ok なのに insert が Err になる。 |
| **M15** 前の隣を見た後の `break` を外す | 生存 | 前の側で「最も近い listed な隣の先にある AC は見ない」ことを確かめるテストがない（後ろの側はある）。 |
| **M16** 曖昧な slot で最初を選ぶ | 生存 | `insert_at::<Mark>` が `AmbiguousSlot` になることにテストがない。 |

提案: 上の 7 つの形のテストを足す。総当たりの `check_insert_at` と `insert_at` の一致（小さなスキーマで、全 index、Err ⇒ 不変）を 1 本入れると、M13 と F1〜F3 の回帰をまとめて押さえられる。

### F5 [Minor・文書] 「O(1)」は上限のない slot に限られる

**根拠**
- 上限のある slot では、`lens.rs:499-504` が全部の子を走査する。しかも子ごとに `candidates` が `Vec` を確保する。
  - 100,000 個の子を持つ親で、`check_insert_at::<Ext>` は 1 回 8.5 ms かかった（`probe_r1b::perf`）。
- 前と後ろの走査も、未登録の要素（拡張要素など）が並ぶ区間では、その長さだけかかる。
- 上限のない slot（`row`、`c`）では問題ない。100,000 回の末尾追加で 81 ms、10,000 回で 13 ms だった。今の呼び出し元の目的（行の追加が二乗になるのを避ける）は果たしている。
- 提案: doc と README の「O(1)」に「上限のない slot では」と添える。F2 の修正で上限つきの slot が `place` を使うようになれば、そのことを書けば済む。

## 確かめて問題がなかったもの

- **`check_insert_at` と `insert_at` の一致**: 50,614 件で、Ok か Err か、Err の種類、文言のすべてが一致した。`insert_at` は、`check_insert_at` と、決定的な `choose_element_prefix` の後にだけ木を変える（`lens.rs:439-448`）。
- **Err ⇒ 変更なし**: 総当たりのすべての Err（`InsertIndexOutOfRange`、`UnresolvedSibling`、`AlreadyPresent`、`TooDeep`）で、`serialize` が元と同じで、`is_dirty()` は偽だった。
- **`InsertIndexOutOfRange`**: `index == len` は末尾への挿入として通る。`len + 1` 以上は拒否される。拒否は深さの検査より先に来る（`probe_r1b::depth` で `len 0` の親に `5` → `InsertIndexOutOfRange`）。
- **曖昧な名前も AC もない親**（`P` の AC を含まない列）: 不正な結果は 0 件、過剰な拒否は 0 件だった。`insert_by` が Ok の場合は、どれも同じ出力の index があった。未登録の子と wildcard の扱いも `insert_by` と揃っている。
- **深さ**: `max_depth 4` で、深さ 3 の親への挿入は通り、深さ 4 の親では `check_insert_at`、`insert_at`、`insert_by` がそろって `TooDeep` になった。結果は同じ上限で parse し直せる。
- **prefix**: prefix だけで束縛されている場合は `t:item` になる。既定の名前空間が途中で別の URI に束縛し直されている場合も `t:item` になる。`t` が子で別の URI に束縛し直され、`u` が同じ URI のままの場合は `u:item` になる。どれも `insert_by` と同じ `choose_element_prefix` を使っている。
- **dirty の義務**: self-closing の親（`<p/>`）だけが `own_dirty` になる。閉じタグのある空の親（`<p x="1"></p>`）は `own_dirty` にならない。祖先は `subtree_dirty` になり、触っていない兄弟は clean のまま残る。2 回目の挿入でも余計な dirty はつかない。
- **既に順序が壊れている親**: `insert_by` は親全体を拒否するが、`insert_at` は近傍が合えば通す。局所的な検査という設計上、受け入れてよい。doc に一言あるとよい。
- `cargo test -p ooxml-core` は 43 件が通過し、`cargo clippy -p ooxml-core --all-targets -- -D warnings` もクリーンだった（作業コピー、Rust 1.99.1）。

## 判定

**converged: no**。

- Blocker: なし。
- Major: F1 の 1 件。曖昧な QName の隣で、`any` による近傍の検査が不正な順序を `Ok` にする（`Tbl` で 430 件）。今の xlsx の呼び出し元には当たらないが、core の汎用 API の約束が破れている。
- Minor:
  - F2: 離れた AC の中身を上限で見落とす（140 件）。
  - F3: 曖昧な名前の数えすぎによる過剰な拒否（23 件）。
  - F4: mutant 8 個が生き残る。`insert_at` の深さの上限、`own_dirty`、prefix の一致、`AmbiguousSlot`、上限の片側。
  - F5: 「O(1)」の但し書き。
- 修正の目安: `any` を `all` にする（または、曖昧な隣では `place` に落とす）。上限のある slot では `place` を呼ぶ。テストを 7 本ほど足す。これで F1〜F4 が片付く。この組み合わせを作業コピーで試し、総当たり 50,614 件で不正な結果が 0 件、既存のテストもすべて通ることを確かめた。
