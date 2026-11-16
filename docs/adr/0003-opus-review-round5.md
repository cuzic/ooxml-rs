# PR #20 レビュー（Opus, round 5）

対象: PR #20 `impl/xlsx-phase1` @ `773a62e`（`98e394f` ADR rev 5 の後に main `848b057` を merge したもの）。ADR-0003 revision 5（Status: Accepted）、`crates/ooxml-xlsx`、`reference/fixtures/xlsx/synthetic/o1_duplicate_caches.xlsx`。

方針: 読み取り専用の敵対的レビュー。`git clone` した複製（scratchpad の `review-20r5-49/pr20`、`pr20m`）で次を走らせた。

- round 3 と round 4 の probe（`zz_probe.rs`、`zz_probe4.rs`、`zz_o3.rs`）の再実行。round 4 の出力と行ごとに diff した。
- 依存規則のミュータント M1〜M12（round 4 の `mutants.py`）と、今回の修正に対するミュータント R1〜R3。
- `--all-features` と既定 feature（`fault-injection` なし）の両方での test と clippy。
- 現在の `origin/main`（`48aa8f0`、#14/#26/#18/#19 を含む）との試し merge と、workspace 全体の CI 相当の検査。
- `build.py` を別ディレクトリで実行し、コミット済みの synthetic fixture とバイト比較した。

## 結論（先に）

**round 4 の指摘はすべて解消された。新しい Blocker と Major は無い。**

- **R4-M1 は解消。** apply (iii) が `while c.remove_cell_value() {}` と `while c.remove_inline_string() {}` になった。round 4 の再現 Q1、Q1b、Q2 では、重複した `<v>`/`<is>` が出力から消えた。対象は、通常の依存先、`t="str"`、CSE の result セル、`is` が2つのセルである。
  - 新しい O1 試験 `o1_duplicate_caches_are_all_stripped` は、修正を戻すミュータント（`v` だけ戻す R1、`is` だけ戻す R2）をどちらも検出する。
- **R4-m6 は解消。** `inject_apply_panic` で、6つの step それぞれに panic を注入する試験がある。「apply の前に poison する」行を消すミュータント（R3）は、この試験で落ちる。
  - hook は非既定の feature `fault-injection` の中だけにある（`#[cfg]`、`#[doc(hidden)]`）。既定の build では、`fault` フィールドも `inject_*` も存在せず、`fault_point` は `#[inline(always)]` の no-op になる。workspace の中でこの feature を有効にする crate は無い。
- **ADR rev 5 の文面（R4-m4、R4-m5）と open questions 1〜4 の締めは、round 3 と round 4 の推奨に一致する**（下の表）。
- **round 3 と round 4 の probe の再実行**: Q1、Q1b、Q2 以外の出力は、round 4 と行単位で同一だった。退行は無い。
- **ミュータント**: M1〜M12 と R1〜R3 の15件は、すべて何らかの試験で落ちる。M7 と M12 を落とすのは単体試験だけで、round 4 と同じである（R4-m1 として ADR に記録済み）。
- **main との merge**: ブランチに入っている main は `848b057`（#40、#41、#30 まで）である。依頼文にある #14、#26、#18（と #19）は、**まだ入っていない**。
  - 現在の `origin/main` との試し merge では、`crates/ooxml-core/src/package.rs` の1か所（2 hunk）だけが衝突した。原因は `let mut declared: u64 = 0;` の位置の違いだけである。
  - main 側を採ると、`crates/ooxml-core` は main とバイト単位で同一になる。workspace 全体で 333 passed / 0 failed / 2 ignored、`clippy --workspace --all-targets --all-features -D warnings` は clean、`cargo xtask codegen --check` は up to date だった。退行は無い。
  - `xlsx_manual_check` の example 名（#41/#42）は入っている。

**判定: converged = yes。** 残りは Minor 2件と Nit だけである。R5-m2（main の merge）は merge の前に済ませる手順の問題で、コードの変更は要らない。

---

## round-4 指摘の解消状況

| ID | 内容 | 判定 | 根拠 |
|---|---|---|---|
| R4-M1 | 重複した `v`/`is` が依存先に残る | **解消** | `edit.rs` L318-323。probe Q1 の出力は `<c r="B1"><f>A1</f></c>`、Q2 も同じ、Q1b では `C2` が `<c r="C2"></c>` になる。O1 fixture と試験あり。ミュータント R1、R2 は `o1_duplicate_caches_are_all_stripped` で落ちる |
| R4-m1 | 独立 oracle が届かない規則 | **記録済み** | ADR の「Known limits」。M7 と M12 は依然として単体試験だけが検出する（round 4 と同じ） |
| R4-m2 | O3 が output のバイトに縛られている | **記録済み** | Consequences に「follow-up 候補」として明記 |
| R4-m3 | 揮発性の除外が定義名を見ない | **解消** | `phase1_o3.rs` の `has_volatile` が `workbook.xml` の `definedName` も走査する。Nit n2 も参照 |
| R4-m4 | §2.4 (a) の根拠 | **解消** | 「LibreOffice は名前のスコープのシート（O3 で観測）、Excel は未検証（XL 項目）」に直り、§7.4 に XL 項目が足された |
| R4-m5 | 古い記述と open questions | **解消** | rev 4 の注記は「run 43821205800 で生成済み」になった。Consequences から「ignored test」が消えた。§7.1 には provenance の参照がある |
| R4-m6 | panic による poisoning の試験 | **解消** | `a_panic_during_apply_poisons_the_workbook`。ミュータント R3（`self.poisoned = Some(..)` を apply の前に置かない）はこの試験だけで落ちる |
| R4-m7 | 全角/半角のシート名 | **記録済み** | Known limits と §7.4 の XL 項目 |

### Open questions 1〜4 の締めと、過去の推奨の照合

| Q | rev 5 の決定 | round 3 の推奨（`0003-opus-review-round3.md` §範囲の判断） | 一致 |
|---|---|---|---|
| 1 `KeepStaleCaches` | 残す。既定ではない。読み手が stale 値を見ることを doc で警告する | 残してよい。名前で危険を示すか、doc の冒頭で LibreOffice/calamine/pandas を明記する。既定は今のまま | 一致。`workbook.rs` L63-66 の doc は「**Excel only**: LibreOffice, calamine and pandas show the stale values」 |
| 2 O3 のデータ | 手動の CI workflow で生成してコミット済み（run 43821205800） | 手動の CI workflow。版を固定し、merge の前に一度回す | 一致（LibreOffice 25.2.7.2 を固定） |
| 3 閾値 | 設けない。`Workbook::dependents` で事前に確認できる | `dependents` で足りる。最悪ケースを doc に書く | 一致。最悪ケースは ADR の性能の表にある |
| 4 manual モード | 現状維持（`fullCalcOnLoad` を足さない）。XL で記録 | 維持。XL は記録のため | 一致 |

---

## 新しい probe（今回追加したもの）

| 確認 | 結果 |
|---|---|
| fixture の再現性: `build.py` を別ディレクトリで実行し、27 個の `.xlsx` をコミット済みと比較 | 27/27 がバイト一致した（`o1_duplicate_caches.xlsx` を含む） |
| 既定 feature での build と試験（`fault-injection` なし） | `cargo test -p ooxml-xlsx`: 113 passed / 0 failed。`clippy -p ooxml-xlsx --all-targets -D warnings`: clean。CI は `--all-features` しか回さないので、この組み合わせは CI で検査されていない（Nit n1） |
| `--all-features` の全試験 | 単体 21、edit 19、oracle 5、package 8、phase1_fixtures 8（1 ignored）、formulas 11、O1 9、O2 1、O3 2、review_r3 9、refusals 21。すべて pass |
| apply の中で値を消す箇所が、ほかにも1回ずつ消していないか | `remove_cell_value`/`remove_inline_string` を呼ぶのは `edit.rs` の strip だけで、どちらも `while` になっている。`write_cell` は `doomed` ですべての `f`/`v`/`is` を消す |

## ミュータント（`mutants5.py`）

| ミュータント | 落とした試験 |
|---|---|
| M1 SUMIF の resize を除去 | 単体、O1、O2、O3 |
| M2 他シートの参照を無視 | 単体、O1、O2、O3、formulas ほか |
| M3 推移をたどらない | O1、O2、O3、formulas |
| M4 spill の潜在領域を seed しない | O1（`o1_arrays_and_spill_growth`）、O3 |
| M5 共有式のオフセットを0にする | fixtures、O1、O3 |
| M6 3-D の先頭シートだけ | 単体、O1、O2、O3 |
| M7 範囲演算子の右辺を捨てる | 単体のみ（R4-m1、記録済み） |
| M8 名前を無視 | 単体、O1、O2、O3、review_r3 |
| M9 列全体を1行にする | O1、O2、O3 |
| M10 result を剥がさない | fixtures、O1（新しい `o1_duplicate_caches_are_all_stripped` も）、O3 |
| M11 相対参照を含む名前を精密に扱う | 単体、O1 |
| M12 `sum_range` を左上だけにする | 単体のみ（等価ミュータント、round 4 と同じ） |
| **R1** strip で `v` を1回だけ消す | `o1_duplicate_caches_are_all_stripped` |
| **R2** strip で `is` を1回だけ消す | `o1_duplicate_caches_are_all_stripped` |
| **R3** apply の前に poison しない | `a_panic_during_apply_poisons_the_workbook` |

---

## Minor

### R5-m1 ADR-0003 の状態を指す記述が、3か所で古いまま

ADR 本体は「Accepted (revision 5)」になった。一方、次の3か所は古い状態を指したままである。

- `docs/adr/README.md` L14: 「Proposed (rev 2); rev 4 and its implementation are in #20」。レビューの列も「r1; later rounds in #20」のまま。
- `crates/ooxml-xlsx/README.md` L5: 「(Proposed, revision 3)」。
- `docs/roadmap/xlsx.md` L325: 「ADR-0003, Proposed, revision 3」（L7 の「Proposes:」も同様）。

提案: 3か所を「Accepted (revision 5)」にし、README の索引に round 2〜5 のレビューへのリンクを足す。docs だけの変更である。

### R5-m2 ブランチは現在の main を含んでいない（merge の前に main を merge する）

- `773a62e` が merge した main は `848b057` である。それ以降に main に入った #14（`9091ef9`）、#26（`a6d1bc1`）、#18（`131f7b3`）、#19（`48aa8f0`）は含まれていない。`gh pr view 20` の mergeable も `UNKNOWN` だった。
- 試し merge の衝突は `package.rs` の `from_bytes_with` の中の1か所だけだった。ブランチが持つ #14 の旧版では `let mut declared` が `seen` の後にあり、main では前にある。**main 側を採れば**、`crates/ooxml-core` は main と完全に一致し、上に書いた全検査が pass する。
- xlsx 側のコードに必要な core の変更は、main の #14 にすべて含まれている（試し merge の後、`ooxml-core` の差分は 0）。

提案: memory の運用どおり、`origin/main` を merge（rebase と force-push はしない）し、`package.rs` は main 側を採る。

---

## Nit

- **n1** `fault-injection` は公開された feature である。Cargo の feature は加算的なので、依存グラフのどこかで有効にされると、`inject_*` が使えるようになる。ただし、hook を呼ばない限り挙動は変わらない。
  - 提案: `Cargo.toml` のコメントに「テスト専用、ライブラリから有効にしない」と書く。
  - 既定 feature の `cargo test`/`clippy` を CI に1行足す（現状は `--all-features` だけで、`#[cfg(not(feature))]` 側の `fault_point` は CI で build されていない）。
- **n2** R4-m3 の走査（`split("<DEFINEDNAME")`）は、接頭辞つきの `<x:definedName>` に当たらない。試験側のコードで、外れても偽陽性（試験が落ちる）の方向にしか働かないので、記録だけでよい。
- **n3** ADR §2.4 の「Excel's documented UI behaviour suggests the using cell's sheet」には出典が無い。XL 項目に回している以上、「documented」は外して「推定」と書く方が正確である。
- **n4** `o1_duplicate_caches_are_all_stripped` は、CSE の master `C1` の出力を文字列で確かめていない（`value_edit` の依存先リストには入っている）。`<c r="C1" s="0"><f t="array" ref="C1:C2">A1:A2*3</f></c>` の assert を1行足すと、M10 系の検出が O1 単独でも明示的になる。

## 判定

- **converged: yes.** Blocker/Major は無い。R4-M1、R4-m3〜m6 は実装と試験で解消され、R4-m1、m2、m7 は ADR に既知の限界として記録された。
- **merge の前に**、R5-m2（`origin/main` の merge、`package.rs` は main 側を採る）を済ませる。R5-m1 の docs 修正は同じ commit で足りる。

## 再現物

scratchpad の `review-20r5-49/`:

- `pr20/`（PR head の clone。round 3 と round 4 の probe を `crates/ooxml-xlsx/tests/zz_*.rs` に置いた）
- `probes20.txt`（今回の出力）、`probes20_old.txt`（round 4 の worktree での出力）
- `mutants5.py`、`mut20.log`（M1〜M12、R1〜R3）
- `pr20m/`（`origin/main` との試し merge。branch `trial`）、`trial20.log`
- `fxgen/`（`build.py` の再生成）
