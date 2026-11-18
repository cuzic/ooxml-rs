# PR #65 レビュー（Opus, round 1）

対象: `test/fuzz-xlsx-formula` @e994fb7（#47 @b82eacf に積む。この PR 固有の差分は bec87e9〈WIP〉、c5ffd67〈#47 のマージ〉、e994fb7）。
検証: 独立した `git clone` 2 つ（変異用と probe 用）。Rust 1.99.1、nightly 1.102.0（5f4effd57）、cargo-fuzz 0.13.2。

## 結論（先に）

**ターゲットの設計は良く、oracle の検出力も高い。** ooxml-xlsx に手で入れた変異 17 個のうち 14 個を、スモーク（`OOXML_SMOKE_ITERS=40`、7 秒）で検出した。
生き残った 3 個のうち 2 個は、出力に現れない等価変異である。
依存モデルが「過小報告しかしない」ことも、ライブラリと突き合わせて確認した。
`ROW`・交差・外部参照・未知の関数・逆順や小文字の配列 `ref`・`si="01"` で、ライブラリはモデル以上を返す。誤検出はなかった（スモーク 150 iters、nightly 6 並列×6 分で計 3.6 万回）。
F-8 も再現した。重大度は Low で妥当である。ただし、影響範囲は文書の記述より広い。`remove_formula` が `Ok` なのに数式が残る。

一方、次の 4 点は Minor である。

- 名前でシートを引くため、同名のシートで誤検出する（#36 m12 と同根）
- 編集したシートの「他のセル」を検査していない
- 共有数式の引き継ぎの `ref` を検査していない
- F-8 の回帰テストが、文書が提案する修正方法の 1 つで落ちる

- Minor 4 件（m1–m4）、Nit 4 件
- 判定: **converged: no**（Blocker・Major はない。ただし m1 は週次 CI の偽の失敗につながり、m4 は修正後の回帰テストが壊れる。どちらも数行で直るので、round 2 で確認したい）

## 確認したこと

### 依存モデルは過小報告だけか（誤検出の有無）

モデルが「ライブラリが無効化しなくてよい依存先」を主張すると、`dependents ⊇ model` と「依存先に `<v>` が残らない」が誤検出になる。
ライブラリの `precedents.rs`・`functions.rs` を読み、紛らわしい形をライブラリに直接与えて `dependents(A1)` を確認した（probe）。

| 形 | モデル | ライブラリ | 判定 |
|---|---|---|---|
| `ROW(A1)` | A1 を辺にする | `ROW` は `DOC_REFINFO`（引数の参照を残す） | 一致 |
| `SUM(A1:A3 A2:B2)`（交差） | 両方の範囲 | `Intersect` は両辺を walk する | 一致 |
| `[1]Sheet1!A1` | ローカルの Sheet1!A1 にしてしまう | `External` → `Everything` | 上位集合なので OK |
| `FOO(B9)`・`A1#`・`Table1[Col]` | 辺あり、または辺なし | 表にない関数・opaque は `Everything` | OK |
| 配列の `ref` が逆順・小文字・`$` つき・アンカー外 | 正規化して結果セルを追加 | 同じ範囲を結果にする | 一致 |
| `si="1"` のマスターと `si="01"` のメンバー | 別グループ（辺なし） | 同じグループとして平行移動する | 過小なので OK |
| シート名の大文字小文字 | ASCII だけで比較 | Unicode で畳み込み、曖昧なら `Everything` | 過小なので OK |

結論として、**モデルの辺は、ライブラリが必ず報告する辺の部分集合**になっている。例外は m1（同名シート）と Nit 2（非 ASCII の引用符つきシート名）で、どちらもハーネス側の名前の扱いが原因である。
なお、モジュールの doc にある「Every edge it finds is a real precedent」は厳密には正しくない。`ROW(A1)` は値の参照ではない。
正しくは「ライブラリも必ず報告する辺」である（Nit 1）。

### oracle の一覧（読んで確認）

| oracle | 状態 |
|---|---|
| `dependents ⊇ model`（ゲートも poison もないときのみ） | 正しい。名前の対応だけ m1 |
| 依存先に `<v>` が残らない | 正しい（変異 F2・F9 を検出） |
| `fullCalcOnLoad`（依存先ありか集合変化。手動モードは除外） | 正しい（F7 を検出） |
| calcChain（関係で解決、集合変化 ⇔ 消える、Override も消える） | 正しい（F5・F6 を検出） |
| 書いた値の読み戻し | 書いた直後のセルだけ。それまでに書いた値は再オープン後に見ていない（m2） |
| 数式セルの集合（毎回と再オープン後） | 正しい |
| `Err` ⇒ バイト列が不変、`Poisoned` は以後ずっと、`Ok` は来ない | 正しい |
| 編集したシート以外は `phase1_oracle`（1 編集ごと） | 正しい（F10・F11 を検出）。名前の対応だけ m1 |
| 編集したシート自体 | 対象セルの読み戻しと数式の集合だけ（m2） |

### ooxml-xlsx への手入れ変異（スモーク `OOXML_SMOKE_ITERS=40`、`xlsx_formula_edit` のみ、release）

| # | 変異 | 結果 |
|---|---|---|
| F1 | `strip_positions` が訪れた行の全セル（定数も）を剥がす | 検出（`sheet2.xml changed beyond …`） |
| F2 | 依存先を一切剥がさない | 検出（`dependent Sheet1!(2,0) … kept its cached value`） |
| F3 | シート名つき参照が辺を作らない（グラフの過小報告） | 検出（`dependents(Sheet1!2,1) misses …`） |
| F4 | 推移閉包を取らない | 検出（`misses`） |
| F5 | 集合が変わっても calcChain を消さない | 検出 |
| F6 | 毎回 calcChain を消す | 検出 |
| F7 | 集合の変化だけでは `fullCalcOnLoad` を立てない | 検出 |
| F8 | 共有数式の引き継ぎで新マスターの式を平行移動しない | 検出（`misses`） |
| F9 | 配列の結果を剥がさない | 検出（`kept its cached value`） |
| F10 | 無関係なシートの先頭セルの `v` を消す（#36 の X7） | 検出 |
| F11 | `calcPr` に `iterate="1"` も書く | 検出 |
| F12 | 引き継ぎ後の `f/@ref` が新マスターだけを覆う | **生存**（150 iters でも生存、m3） |
| F13 | `set_formula` した新しい数式をグラフに足さない | 検出（`misses`） |
| F14 | `Rect::intersects` の境界を 1 つずらす | 検出 |
| F15 | 剥がした後に census の `has_cache` を落とさない | 生存（等価: `strip_positions` が実物の `v` を見る） |
| F16 | 引き継ぎ後に census のマスターの式を更新しない | 生存（ほぼ等価: グラフは再構築されず、保存物は正しい） |
| F17 | **編集したシート**の先頭セルの `v` を消す | **生存**（150 iters でも生存、m2）。#36 の `xlsx_edit` では `(0,0) after reopen` で検出される |

### 誤検出の確認

| 実行 | 結果 |
|---|---|
| スモーク `xlsx_formula_edit`、150 iters（release、57 秒） | pass。成功した編集 10,921、確認した依存先 7,000、calcChain の判定 5,043、拒否 4,275 |
| `cargo test -p ooxml-robustness`（release）、clippy `--lib --tests -D warnings` | pass、警告なし（高負荷時の F-7 の 1 回の偽の失敗は #36 round 4 の Nit 3） |
| nightly `xlsx_formula_edit`、seed 593 件（全量）、6 並列×360 秒、`-timeout=10` | 計 3.58 万回、cov 12,593–12,597、失敗も timeout もなし |

### F-8 の確認（probe）

`wrong_row_formula_book()` の形（`<row r="1">…<c r="B2"><f>A1*2</f><v>2</v></c></row>`）で、ignore を外して回帰テストを実行した。
2 つ目の `c r="B2"` ができて失敗し、**再現した**。ほかの操作も試した。

| 操作 | 結果 |
|---|---|
| B2 に `set_formula`（文書の再現） | `Ok`。B2 が 2 つになり、calcChain が残る |
| B2 に `set_number` | `Ok`。行 2 に B2=7 ができ、行 1 の `<c r="B2"><f>A1*2</f>` が**残る**。calcChain は消える |
| B2 に `remove_formula` | `Ok`。行 2 に空の `<c r="B2"/>` ができ、行 1 の**数式はそのまま残る** |
| 既存の行 2 がある場合に B2 へ `set_number` | `Ok`。行 2 に B2 を挿入し、行 1 の B2 が残る |
| A1 に `set_number`（依存先 B2 が別の行にある） | `Err(InvalidReference("B2 in row 1"))`。安全に拒否する |
| `r` のない行に数式がある | open 時に `MissingReference` のゲートがかかる。安全 |
| 行 2 の中に `B1` の数式がある（逆向き） | `set_formula` B1 が `Ok` で、B1 が 2 つになる |

**重大度**: 入力はスキーマ上不正で、同じ行のセルの読み書きは `InvalidReference` で拒否される。この 2 点から **Low は妥当**である。
ただし、文書の「`set_formula` で重複し calcChain も残る」は影響の一部でしかない。
いちばん利用者に見えやすいのは、`remove_formula` が `Ok` を返すのに数式がファイルに残ることである。文書にこれも書くこと（m4）。

## 所見

### m1 [Minor] 同名のシートがあると、`dependents ⊇ model` と `phase1_oracle` が誤検出する（#36 m12 と同根）

**場所**: `xlsx_formula.rs` の `check_xlsx_formula`（`wb.dependents(&m.names[sheet], …)`、`lib` を `position(|x| x == n)` で引き戻す部分、`before_write(&mut wb, &m.names[sheet], …)`）

ハーネスは index でシートを編集するが、`dependents` と `before_write` には名前を渡す。結果も名前で引き戻す。
`<sheet name="Sheet1">` が 2 つあると、両方とも 1 枚目に解決される。
`formula_books()[0]` の `Sheet2` を `Sheet1` に変えたブックでは、ランダムな ops 3,000 本のうち 2,750 本が panic した（`sheet2.xml changed beyond …`）。
ライブラリの出力は正しい。曖昧な `Sheet1!` は `Everything` になり、2 枚目の依存先も剥がされる。

**修正案**: `model()` で `names` に完全一致の重複があれば `None` を返す。このターゲットは index でしか正しく扱えないので、F-8 と同じく対象外にする。
#36 の `phase1_oracle` 側の対応（#36 round 4 の m12）と揃えること。

### m2 [Minor] 編集したシートの、対象以外のセルを検査していない

**場所**: `check_xlsx_formula` の `Ok(())` の分岐

`phase1_oracle` は編集したシートを飛ばす。このターゲット自身が見ているのは次の 3 つだけである。

- 対象セルの読み戻し（直後のみ）
- 依存先に `v` がないこと
- 数式セルの集合

このため、F17（編集したシートの別のセルの `v` を消す）が 150 iters でも生き残る。xlsx_edit（#36）は `written` を再オープン後に読み戻すので、同じ変異を検出する。

**修正案**（どちらか、または両方）:

- (a) モデルはすでに全セルの `has_v`・`has_f`・`formula` を持っている。そこで編集のたびに、次の 3 種類以外のセルが前後で同じであることを assert する（数行）。
  - 対象セル
  - モデルの依存先と配列の結果
  - ライブラリの `dependents`（`pending`）が返したセル

  これで F17 と、編集したシートでの F1 が検出できる。
- (b) xlsx.rs と同じく、`written` を保持し、最後の再オープン後にすべて読み戻す。

### m3 [Minor] 共有数式の引き継ぎ（§4.2）の結果を検査していない

**場所**: `model()`、`check_xlsx_formula`

マスターを上書きすると、新マスターの式と `f/@ref` が書き換わる。F8（式を平行移動しない）は、モデルとグラフの不一致から偶然検出された。
一方、F12（`ref` が新マスターだけを覆う）は生き残った。これは、Excel がメンバーを見失うか修復を求める出力になる。

**修正案**: 編集の前後で、次の 2 つを assert する。

- 対象以外の数式セルの「実効的な参照」（モデルの `reads`）が変わらないこと。F8 型の誤りを、グラフと独立に検出できる
- 入力で成り立っていた「各共有グループのマスターの `ref` が全メンバーを含む」が、出力でも成り立つこと

### m4 [Minor] F-8 の回帰テストが、文書の提案する修正の 1 つで落ちる。影響範囲の記述も不足している

**場所**: `tests/fuzz_findings.rs` の `f8_formula_cell_in_another_row_is_not_duplicated`、`fuzz-findings-2026-11.md` の F-8

- テストは、「control」として最初に `assert!(wb.edit_refusal().is_none())` を置いている。
  文書の修正方法の 1 つは「census の走査中に `FormulaScanFailed`（またはそのシートの編集をすべて拒否）」である。
  `FormulaScanFailed` は census の block なので、`edit_refusal()` が `Some` になる。つまり、**正しく直すとこのテストが落ちる**。
  control は「`dependents(A1)` が B2 を含む」（census が B2 を数式セルと見ている）にする。
  `edit_refusal()` が `Some`、または `set_formula` が `Err` なら、修正済みとして return する。
- テストは `set_formula` しか見ていない。上表のとおり、`set_number` と `remove_formula` も `Ok` で行 1 の数式を残す。
  特に `remove_formula` は「数式を消したのに残る」。回帰テストに `remove_formula` の場合を足し、F-8 の文書の再現と影響にもこれを書くこと。
  逆向き（行 2 の中に `c r="B1"`）も同じ原因なので、推定原因の記述に含めるとよい。

### Nit

1. モジュールの doc と文書の「ここで見つかる辺はすべて本当の参照」は、「ライブラリも必ず報告する辺」に直す。`ROW(A1)` は値を読まないが、ライブラリは `DOC_REFINFO` で引数を残す。
   `[1]Sheet1!A1` をローカルの Sheet1!A1 と読むのも、ライブラリが `Everything` にするから無害である。
2. 引用符つきシート名を `name.push(b[i] as char)` でバイトごとに組み立てている。非 ASCII の名前が化け、化けた結果が別のシート名（例 `Ã©`）と一致すると、モデルが過大な辺を作る。
   ありえない組み合わせに近いが、`text[start..end]` で切り出し、`''` を置き換える方が素直である。
3. `Model::full_calc` は代入するだけで読んでいない。手動モードと同じく「すでに立っている」場合の扱いを書くなら使い、使わないなら消す。
   検査の後ろにある `let _ = &mut ws;` も不要。
4. 文書の「検査」2 の「workbook、registry、数式を持つシートと編集したシート以外は L1 一致」は、旧 oracle の記述である。
   同じ節の後ろにある `phase1_oracle` の説明と矛盾するので、`phase1_oracle` の規則に書き換える。

## 判定

**converged: no** — Blocker・Major はない。依存モデルの方向（過小報告のみ）、oracle の検出力（変異 17 個中 14 個検出、残り 2 個は等価）、F-8（Low）はいずれも確認できた。
m1（同名シートでの誤検出）と m4（修正後に壊れる回帰テスト）は数行で直るので、round 2 で確認する。
m2・m3 は検出力の穴で、(a) の前後比較を入れれば F17・F12 型も検出できる。
土台の #36（m12）と同時に直すこと。
