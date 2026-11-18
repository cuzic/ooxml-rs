# PR #57 レビュー（Opus, round 2）

対象: `impl/pptx-phase1b-tables` @ `64272bd`（round 1 の修正 `5ab08a2` と origin/main のマージ）。round 1（`docs/reviews/pr57-round1.md`、`2b3e343`）の指摘を照合した。
リポジトリは変えていない。作業は scratchpad の `review-51r2-57r2/r57/` で行い、`git clone` したコピーを使った。

- `pr57/`: `64272bd` のまま。`tests/rev_probe2.rs`（プローブ）と `examples/rev_model.rs`（round 1 のモデルの駆動部）を置いた。
- `mut57/`: レビュアーのテストを入れていないコピー（`rev_model.rs` は example なので置いた）。`scripts/mutate2.py` の mutant 26 個を流した。
- `scripts/{gen,check}.py`: round 1 のランダム連鎖モデルを拡張したもの。40% のデッキで、セルの 25% から `a:txBody` を取り除く（m2 の経路を通すため）。

## 結論（先に）

**収束（Blocker 0、Major 0）。** M1 と m1〜m8 はすべて解消した。

- M1: `set_style` は照合に成功した正規の綴りを書く。小文字、前後の空白（タブと改行を含む）は、ビルトインの大文字 `ST_Guid` になる。定義済みの id は定義の綴りになる。同じ style を別の綴りで指定しても、パッケージは変わらない。テストも直っている。
- round 1 で生存した mutant X5、X7、X9 は、今回の著者テストですべて検出された。新しい経路の mutant 12 個も、すべて著者テストで検出された（後述）。
- ランダム連鎖モデル（txBody のないセルを加えたもの）は 1,500 表 × 30 手 = 45,000 操作で不一致 0。python-pptx での読み戻しも 1,500/1,500 で問題なし。
- 残るのは Nit 2 件だけである。

## 検証したこと

| 項目 | 結果 |
|---|---|
| `cargo test --release -p ooxml-pptx`（git clone 上） | 全 pass |
| `cargo xtask codegen --check` | inputs verified, 7 generated files up to date |
| コーパス sweep（`table_sweep`、round 1 と同じ 617 decks、114 表） | 成功はすべて再オープンと `check()` が OK、拒否はすべてバイト不変。merge の `NoTextBody` 4 件は 0 件になり、merge ok が 209 → 213 件。保存した出力は 470 → 472 件 |
| python-pptx oracle `tests/oracle/tables.py` | sweep 出力 472 件 ok、`table_cases` 176 件 ok |
| `a:tc` の子の順序（レビュアーのスクリプト、sweep 出力すべて） | `txBody → tcPr → extLst` の違反 0。origin に txBody がない merge 2 件（`00181`、`00186`）は、移すテキストがないので txBody を作っていない |
| ランダム連鎖モデル（レビュアー作、拡張版） | 45,000 操作（merge 成功 1,907、うち txBody のない origin への merge 48 件。テキストの移動あり 27 件、なし 21 件）で不一致 0。txBody の有無もモデルと照合した |
| python-pptx 読み戻し（モデル出力 1,500 本） | 0 bad |
| mutant 26 個（round 1 の 14 個と、今回の修正経路の 12 個） | 著者 suite での非等価な生存 0（後述） |

## 解消状況

| round 1 | 内容 | 状況 |
|---|---|---|
| M1 | `set_style` が受理した綴りをそのまま書く | **解消。** `builtin()` が正規の綴りを返し、`set_style` は `canon` を比較と書き込みに使う（`write/table.rs:946-960`、`builtin_styles.rs:89-92`）。プローブ: `"{073a0daa-…}"` と `"\t{073A0DAA-…}\n"` はどちらも `{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}` で書かれる。波括弧のない id と空文字列は `TableStyleNotDefined` で、パッケージは不変。テスト `style_ids_defined_or_built_in`、`a_defined_style_is_written_in_the_definitions_spelling` が固定している |
| m1 | inline `a:tableStyle` | **解消。** `Some` と `None` の両方を `TableStyleInline` で拒否し、パッケージは不変（テスト `an_inline_table_style_is_refused`）。ADR-0013 の実装ノートにも記載がある |
| m2 | origin に txBody がないと merge が拒否される | **解消。** 移すものがなければ txBody を作らない。移すものがあれば `a:bodyPr` と空の `a:p` を作り、その空段落は移動の後で消える（`ensure_body`、`write/table.rs:1241-1258`）。コーパスとモデルで確認した（上表）。子の順序もスキーマどおりである |
| m3 | 表ロックの粒度 | **解消。** 表の種類以外のアンカーは、名指しした id だけをロックする。id のないアンカーは、引き続きすべての表をロックする（`refs.rs:190-207`）。冗長だった `gridColMkLst` の条件も消えた。ただし Nit n1 を参照 |
| m4 | R2-6 の compile-time assert | **解消。** 書き込み先の型 16 個を `const _: () = assert!(…)` に入れた（`write/table.rs:1362-1380`）。`set_attr`/`set_text` の呼び出し先（`TableCell`、`TableRow`、`GridColumn`、`TableProperties`、`TableStyleId`、`TableCellProperties`、罫線 4 種、色 2 種、`Extension`、a16 id 2 種、`dml::Extents`）と突き合わせ、漏れはなかった |
| m5 | ADR-0002 note（wildcard parents） | **解消（記載）。** generator 側での除外はしていないが、round 1 の提案の最低線は満たしている |
| m6 | a16 id の重複 | **解消。** `ids()` が重複を `TableNotWellFormed("duplicate a16:…")` にする（`write/table.rs:437-449`）。テスト `duplicate_ids_are_not_well_formed` がある |
| m7 | コメントの不一致 | **解消**（`write/table.rs:687`） |
| m8 | X5、X7、X9 のテスト | **解消。** `deleting_shrinks_the_frame`、`insert_column_copies_width_and_format_from_the_left`、`fault::a_failure_after_some_writes_leaves_the_slide_untouched`（`#[cfg(test)]` のフォールト注入）。3 個とも著者 suite で検出された（後述） |

## mutant 結果（26 個、`mut57/`、著者 suite = `cargo test --release -p ooxml-pptx`）

MUTANT_TABLE

## Findings

### Blocker / Major

なし。

### Minor

なし。

### Nit

**n1. 枠全体を指す `graphicFrameMkLst` アンカーが、表をロックするようになった。ADR-0013 §2 の本文と食い違う**
`crates/ooxml-pptx/src/refs.rs:190-207`、`docs/adr/0013-pptx-phase1b-tables-and-smartart.md:234-241`

プローブ（`rev_probe2.rs::rp_anchor_kinds`、表 id=4）:

| アンカー | `insert_row` | セルの `set_text` |
|---|---|---|
| `ac:deMkLst` + `graphicFrameMk id=4` | Ok | Ok |
| `ac:graphicFrameMkLst` + `graphicFrameMk id=4`（表の種類の list を含まない） | `CommentAnchoredTable` | `CommentAnchoredText` |
| `ac:spMkLst` + `spMk id=99` | Ok | Ok |
| `ac:fooMkLst`（id なし） | `CommentAnchoredTable` | `CommentAnchoredText` |

§2 は「`graphicFrameMkLst` は、表の種類の list を含むときだけ」表の内側とし、枠全体のアンカーはロックしないと書く。round 1 の実装も、`graphicFrameMkLst` を既知の安全な名前として扱っていた。今回の実装では、名指しされた id として表をロックする。安全側なので害はなく、実装ノート（「outside the table kinds locks only the shape ids it names」）からも読める。ただし §2 本文とは合わない。§2 の該当文に「`deMkLst` 以外の list が枠を名指しした場合はロックする（review #57 round 1）」と一言足せば十分である。

**n2. 表ロックの「安全な名前」が名前空間を区別しない。Phase 0 の `scan_comments` とずれている**
`crates/ooxml-pptx/src/refs.rs:196`（`ns_ok && matches!(local, "sldMkLst" | "deMkLst")`）

`scan_comments`（143 行）は `pc:sldMkLst` と `ac:deMkLst` だけを安全とする。表ロックは `ac`/`pc` のどちらでもよい。プローブでは、存在しない `pc:deMkLst` が `spMk id=4` を含むとき、テキストはロックされるが（`CommentAnchoredText`）、表の構造編集は通る。実在しない入力で、round 1 から変わらない挙動なので Nit とする。`a.is(QName::new(PC_NS, "sldMkLst")) || a.is(QName::new(AC_NS, "deMkLst"))` に揃えるのが簡単である。同様に、`ac:deMkLst` に `txMk` がある（テキストのアンカーだが、セルのモニカーがない）場合、テキストはロックされるが構造編集は通る（`structural()` が見るのは `guard.table` だけ）。PowerPoint はこの形を出さないと思われる。

## 判定

**converged: yes**

- Blocker: なし。
- Major: なし（M1 は解消）。
- Minor: なし（m1〜m8 は解消）。
- Nit: n1（`graphicFrameMkLst` の扱いを ADR §2 本文に反映）、n2（安全な名前の名前空間）。マージを止めるものではない。
