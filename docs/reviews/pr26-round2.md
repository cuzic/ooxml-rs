# PR #26 レビュー（Opus, round 2）

対象: `core/insert-at` @ `631fc90`（round 1 の修正 F1〜F5）。
照合したもの: `lens.rs` の `check_insert_at` / `insert_at` / `place`、`tests/lens.rs` の追加テスト 4 本、README。
プローブと mutant は scratchpad（`review-14r6-26r2/`）の作業コピーで実行した。リポジトリは変えていない。

- `pr26_probe_r2.rs`: 総当たりを広げた（計 265,774 件）。
  - スキーマ 5 つ:
    - round 1 の `P`、`Tbl`、`Lim`
    - 新しい `Q2`: 曖昧な名前が 2 つある（`x` は slot {0,2}、`y` は {1,2}）。上限のある slot が曖昧な名前を含む（`y|z`、each 1 / total 2）。上限のある wildcard slot もある
    - 新しい `WC`: `o:*` が 2 つの wildcard slot（`List(urn:o)` と `Any`、後者は total 1）に入るので、wildcard だけで曖昧になる
  - 子の列: AC を任意の位置に置く（`ac_mark`、`ac_xy`、`ac_w`、`ac_oz` など）。
  - 各件で比べたもの:
    - `check_insert_at` と `insert_at` の一致
    - Err のとき木が変わらないこと
    - オラクル（AC の各分岐を展開し、貪欲な割り当てと上限を確かめる）による結果の正しさ
    - **`insert_by` の全位置**（`before = |i, _| i >= k`、k = 0..）との比較: 両方が `Ok` なら、listed な要素の並びが同じか。`insert_at` が `Ok` で `insert_by` がどの位置でも拒否する場合は、その理由を分類した
  - 深さ: `max_depth` 1〜7 × 親の深さ 1〜6（81 件）。
  - 名前空間: prefix・既定の名前空間・束縛し直し・未束縛・兄弟にだけ束縛があるもの（72 件）。どれも `insert_by` と出力を比べた。
- round 1 の `probe_r1.rs`（50,614 件）と `probe_r1b.rs` もそのまま再実行した。
- `mutants_pr26.py`: 25 個の mutant。round 1 の 17 個を今のコードに合わせ、F1 と F2 の修正を戻すもの、走査の範囲を変えるものを足した。

## 結論（先に）

**収束（Major なし）。**

- F1〜F5 はすべて解消した。
- 広げた総当たり 265,774 件と、round 1 の 50,614 件の再実行で、次のものは **0 件**だった。
  - 不正な結果（`Ok` なのにスキーマ上不正）
  - `check_insert_at` と `insert_at` の不一致
  - Err の後に木が変わる例
  - 両方が `Ok` なのに `insert_by` と結果が違う例
  - `insert_by` がスキーマを理由に拒否するのに `insert_at` が通す例（文書化された 2 つの緩和を除く）
- 残りは Minor 1 件と Nit 1 件だけである。
  - G1: 上限が片方だけの slot、または max_each < max_total の slot のテストがない。そのため mutant 3 個が生き残る。
  - G2: doc の「Disorder elsewhere in the parent is not examined」は、上限のある slot には当てはまらない。

## 解消状況

| round 1 | 状況 | 根拠 |
|---|---|---|
| **F1** 曖昧な隣で不正な順序を通す | **解消** | `lens.rs:491`、`501` が `all` になった。不正な結果は `Tbl` で 0 件（round 1 では 430 件）、新しい `Q2`（曖昧な名前が 2 つ）と `WC`（wildcard だけで曖昧）でも 0 件だった。安全側の拒否は `Tbl` で 1,238 件、`Q2` で 2,054 件、`WC` で 75 件あった（例: `<x/><y/>` の間への `z`。`insert_by` なら `x z y` にできる）。doc は「use `insert_by` or `insert_in_slot` there」と案内している。テストは `insert_at_with_ambiguous_neighbours`。 |
| **F2** 離れた AC の中身を上限で見落とす | **解消** | 上限のある slot では `place` を呼ぶ（`lens.rs:512-514`）。AC が親のどこにあっても拒否される。`P` の不正な結果は 0 件（round 1 では 140 件）。 |
| **F3** 曖昧な名前の数えすぎ | **解消** | `place` の割り当て（`rank == slot`）で数える。`Lim` の過剰な拒否は 0 件（round 1 では 23 件）。 |
| **F4** mutant 8 個が生き残る | **解消** | round 1 の M6〜M8、M11〜M13、M15、M16 は、どれも検出されるようになった（下表）。ただし M6 は、検査が `place` に移った後の新しい形では生き残る（G1）。 |
| **F5** O(1) の但し書き | **解消** | doc（`lens.rs:426-430`）と README の L101 が「for a slot without limits」に限っている。上限のある slot は O(children) と明記されている。100,000 子での `check_insert_at::<Ext>` は 1 回 4.1 ms（round 1 では 8.5 ms）。上限のない slot に 100,000 回末尾追加すると 69 ms で、二乗にはならない。 |

## 総当たりの結果（`pr26_probe_r2.rs`）

| スキーマ | 件数 | Ok | 不正 | 不一致 | Err で変化 | `insert_by` と結果が違う | `insert_by` が拒否し `insert_at` が通す |
|---|---|---|---|---|---|---|---|
| P | 35,325 | 5,395 | 0 | 0 | 0 | 0 | 1,054（親がすでに不正: 978、AC が離れた位置にある上限なし slot: 76） |
| Tbl | 45,962 | 6,332 | 0 | 0 | 0 | 0 | 4,509（2,924 / 1,585） |
| Lim | 1,661 | 44 | 0 | 0 | 0 | 0 | 0 |
| Q2 | 130,092 | 13,257 | 0 | 0 | 0 | 0 | 8,597（7,753 / 844） |
| WC | 52,734 | 5,631 | 0 | 0 | 0 | 0 | 5,115（4,966 / 149） |

- 「`insert_by` が拒否し `insert_at` が通す」場合は、すべて doc に書かれた 2 つの緩和のどちらかだった。
  1. 親がすでに順序を崩している（「Disorder elsewhere in the parent is not examined」）。
  2. 上限のない slot で、AC が最も近い listed な隣ではない。
- 2 の 2,654 件は、オラクルが AC の全分岐で正しいと判定した。最も近い隣が `all` を満たせば、隣の割り当ては AC の分岐によらず変わらない。そのため、この緩和は健全である。
- 1 と 2 のどちらにも当たらない例は 0 件だった。
- 深さ（81 件）: `insert_at::<Item>`、`insert_at::<D>`、`insert_by` の Ok と Err の境目が、どれも `depth + 1 <= limit` と一致した。`check_insert_at` との不一致は 0 件、Err の後の変化も 0 件だった。`Ok` の結果は、同じ上限でどれも parse し直せた。
- 名前空間（72 件）: 結果（Ok か Err の種類）と出力の文字列が、すべて `insert_by` と一致した。次の場合も含む。
  - 既定の名前空間を `urn:x` や `""` に束縛し直したもの
  - `t` を子で束縛し直し、`u` だけが残るもの
  - `o` を親で別の URI に束縛し直し、`o2` だけが残るもの
  - 兄弟にだけ束縛があるもの（`NamespaceNotInScope`）
- round 1 の `probe_r1.rs` の再実行（50,614 件）: 不一致 0、Err で変化 0、不正 0。安全側の拒否は `Tbl` の 1,238 件だけで、round 1 で予告したとおりである。

## 指摘

### G1 [Minor・テスト] 上限が片方だけの slot、max_each < max_total の slot のテストがない

`mutants_pr26.out`（25 個中 22 個を検出）:

| mutant | 結果 |
|---|---|
| M1〜M5、M7〜M19、M22〜M25 | 検出（F1 の修正を戻す M18 と M19、走査の範囲を変える M23〜M25 を含む） |
| **M6** `place` の `max_each` で `>=` を `>` にする | **生存** |
| **M20** `place` を呼ぶ条件を `max_each.is_some()` だけにする | **生存** |
| **M21** 同じく `max_total.is_some()` だけにする | **生存** |

- テストのスキーマの上限は、`one`（1/1）と `(a|b)` の 2/2 だけである。どちらも max_each と max_total が同じなので、片方を壊しても他方が拒否する。
- コミットメッセージは「each limit separately」と書いている。しかし `<a/><a/>` に `A` を足す場合も、`max_total 2` が拒否する。
- 生成済みのスキーマ（sml、pml、dml）では、上限は 1/1、1000/1000、32767/32767 だけである。どれも max_each と max_total が同じなので、**今の呼び出しには影響しない**。ただし core の汎用の約束であり、M6 は `insert_by` の `place` も同時に壊す。
- 総当たりの `Q2`（`y|z` が each 1 / total 2）は、この形を正しく扱えることを確かめた。
- 提案: テストに次の 2 つを足す。
  - `Slot { names: [a, b], max_each: Some(1), max_total: Some(2) }`: `<a/>` に `A` を足すと `AlreadyPresent`、`B` なら `Ok` になる
  - `max_total: None` で `max_each: Some(1)` だけの slot

### G2 [Nit・文書] 上限のある slot では、親の別の場所の乱れも拒否される

- doc（`lens.rs:438-439`）は「Disorder elsewhere in the parent is not examined (where `insert_by` refuses the whole parent)」と書いている。
- しかし上限のある slot では `place` が親全体を割り当てるので、別の場所の乱れも `UnresolvedSibling` になる。
  - 例: `<p><item/><ext/><head/></p>` に対して、`check_insert_at::<Item>(1)` は `Ok` だが、`check_insert_at::<A>(0)` は `UnresolvedSibling` になる。
- 安全側なので Nit とする。「(for a slot without limits; a slot with limits is checked as `insert_by` checks it)」などと添えるとよい。

## 確かめて問題がなかったもの

- `insert_at` は `check_insert_at` を通った後でだけ木を変える（`lens.rs:447-456`）。`choose_element_prefix` は決定的なので、2 回呼んでも結果は同じである。
- 証明の筋: 元の木が正しい順序で、前の隣の候補がすべて `slot` 以下、後ろの隣の候補がすべて `slot` 以上なら、次のことが言える。
  - 前の隣の割り当ては変わらない。
  - 後ろの隣は「`slot` 以上の最小の候補」に割り当てられるが、すべての候補が `slot` 以上なので、元の割り当てと同じになる。
  - そのため、それより後ろの割り当ても変わらない。
  - wildcard だけで曖昧な名前（`WC`）でも、AC が離れた位置にある場合でも、総当たりはこの筋どおりだった。
- `cargo test -p ooxml-core` はすべて通過した（lens 18 件を含む）。`cargo clippy -p ooxml-core --all-targets -- -D warnings` もクリーンだった（作業コピー、Rust 1.99.1）。

## 判定

**converged: yes**。

- Blocker: なし。
- Major: なし。
- Minor: G1 の 1 件。上限が片方だけの slot、または each < total の slot のテストがない。mutant M6、M20、M21 が生き残る。今の生成済みスキーマには当たらない。
- Nit: G2 の 1 件。「Disorder elsewhere … not examined」は、上限のない slot に限った話である。
- F1〜F5 は解消した。広げた総当たり 265,774 件（曖昧な名前を 2 つの slot に置くもの、wildcard だけの曖昧さ、任意の位置の AC、each と total、深さ、名前空間を含む）で、不正、不一致、`insert_by` を超える受け入れは、文書化された緩和を除いて 0 件だった。G1 と G2 は、マージの前でも後でも直してよい。
