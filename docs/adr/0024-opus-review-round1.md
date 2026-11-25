# ADR-0024 レビュー（Opus, round 1）

対象: `plan/xlsx-edit-journal` の未コミットの `docs/adr/0024-xlsx-edit-journal.md`（revision 1）。
PR はまだ無い。コードは `main` の `f88d02c` で照合した。ADR-0021/0022 は `plan/xlsx-js-compat` の未コミット版、ADR-0023 は `plan/xlsx-rpa-foundation` の未コミット版で照合した。ExcelJS 4.4.0 のソースとプローブも使った（プローブはコミットしていない）。

## 結論（先に）

Base / Journal / View / Commit という骨格と、§8 の未決 1〜3 を Q1〜Q3 として owner に聞く形はよい。ただし、規則どうしが食い違っており、そのまま実装すると正常系で黙って誤る経路が 3 つある。

- 1 つ目は、セッション中に書いた数式の参照がずれる経路（B1）。
- 2 つ目は、`truncate` でロールバックした書き込みが消える経路（B2）。
- 3 つ目は、セル書式の部分パッチが消えるか、Commit が `CommitInternal` で落ちる経路（B3）。

さらに次の 5 点が Major である。

- 「既存の仕組みにコンパイルされる」と書いた op の一部に、`main` 上のコンパイル先も逐次参照も無い（M1）。
- 追記時検証の土台が書かれていない（M2）。
- Test oracle 1 の等式が、畳み込みと戻し書きで設計上成り立たない（M3）。
- Commit の作業コピーが、実在しない仕組みを前提にしている（M4）。
- Commit をまたぐ `truncate` が未定義（M5）。

**converged: no**

## Findings

### B1（Blocker）: 数式エントリのテキストが、step map と ADR-0018 の書き換えで二重にずらされる

- 根拠:
  - N3(ii)（§4.4）により、座標テキストを持つ数式エントリは、参照を書き換える構造操作を越えて後ろへ移動しない。つまり生まれた epoch に残る。
  - §7 step 4 は、epoch `k` のエントリを epoch `k` の座標で適用し、その後で構造操作 `k+1` を ADR-0018 の 1 操作として適用する。ADR-0018 §2.1 step 3.2 はシート上のすべての数式テキストを書き換えるので、いま書いたセルの数式もここで書き換わる。
  - 一方で §7 step 2 と §8.1 は「Commit 時に、各参照トークンを生まれた epoch より後の step map に通す」とも書いている。
  - 両方を実装すると、次のようになる。epoch 0 で `A1 = B5*2` を書き、その後 `insert_rows(3, 1)` を行う。step map で `B6` になったテキストを epoch 0 で書き込み、それを ADR-0018 がさらに `B7` にする。結果は `Ok` で、参照が黙ってずれる。
  - 逆に、step map で写してから最後の epoch で適用する実装にすると、今度は N3(ii) と矛盾する。さらに、StepMap には ADR-0018 §3.2〜§3.6 の規則（`ShiftOffSheet`、sticky end、構造化参照、名前、`#REF!` の接頭辞形）が無い。そのため、逐次適用の結果とずれる。
- 修正案: Commit では、数式エントリを生まれた epoch の座標で書き、その後の書き換えは ADR-0018 の操作に任せる。§7 step 2 と §8.1 の「Commit 時の写像」は削る。StepMap は View の normalized 読みにだけ使い、その結果が ADR-0018 の書き換えと一致することを Test oracle 2 の「View = sequential state」に明記する。

### B2（Blocker）: epoch 内の畳み込み（§4.3）で、`truncate` に要る履歴が消える

- 根拠:
  - §2 の図と §4.3 では、epoch ごとに `{key → Entry}` の map を持ち、同じキーへの書き込みは「後勝ちで、payload と origin ごと」置き換わる。
  - 次の順に操作したとする。`set(k, 1)`（seq 1）、`set(k, 2)`（seq 2）、`truncate(1)`。seq 1 のエントリは seq 2 に置き換えられて既に無い。そのため `truncate` 後の `k` は Base の値を読む。seq 1 で書いた 1 が黙って失われる。
  - これは Test oracle 8 の「後の op を持たなかった Journal と同じ出力」に反する。ADR-0023 §11.2 の置き換えとして `with wb.transaction():` のロールバックに使う経路なので、正常系である。
- 修正案: 次の 2 つのどちらかにする。
  - 追記時には畳み込まず、ログに残す。N1 は `normalize` の中でだけ使う。
  - `truncate` が戻せる境界（transaction の開始点）より前のエントリだけを畳み込む。

  どちらにしても、Test oracle 8 の生成器に「同じキーへの複数回の書き込みをまたぐ truncate」を入れる。

### B3（Blocker）: 書式キーの「後勝ち」で部分パッチが消え、内容と書式を同じプランに入れると `DuplicateCellWrite` になる

- 根拠:
  - `main` の書式書き込みは部分パッチである。`set_fill` と `set_border` は `StyleRequest::FillOnly` / `BorderOnly`（`setters.rs:200-210`）で、現在の書式に重ねる。
  - §4.1 は書式に「set style (resolved `xf` or a style patch)」を 1 キー `(…, Style)` で持ち、§4.3 で後勝ちに畳む。ExcelJS で `cell.fill = …; cell.font = …` と書くと、同じキーに 2 つのパッチが入り、fill のほうが黙って失われる。
  - §4.1 は内容と書式を別のキーに分ける（facet-separated）。§7 step 4 は、epoch `k` のエントリをシートごとに 1 つの `CellWritePlan` にまとめる。ところが `plan_writes` は同じセルが 2 回出ると `Err(DuplicateCellWrite)` を返す（`edit.rs:373-376`）。分割規則は `SharedFormulaMaster` の場合しか書かれていない（§7）。「値を書いてから書式を付ける」という最も普通の Journal が、受理されたあと Commit で `CommitInternal` になり、§4.5 の不変条件が破れる。
  - `set_datetime` は内容の op が書式の副作用（`StyleRequest::EnsureDate`、`setters.rs:229-233`）を伴う。これも 2 つの facet に分けられない。
- 修正案:
  - 書式エントリは「パッチの合成」として畳む規則にする（後勝ちにしない）。合成の結果は、逐次適用と同じ解決済み `xf` になることにする。
  - Commit では、同じセルの内容と書式を 1 つの `CellOp`＋`Extra` にまとめる。まとめられない場合はプランを分ける規則を §7 に書く。
  - Test oracle 1 の生成器に「同じセルへの内容と書式」「書式パッチの連続」を入れる。

### M1（Major）: §4.1・§5.6 の op の一部に、`main` 上のコンパイル先も逐次参照も無い

- 根拠:
  - §1 は「`main` が今日提供する（Phase0–7）すべての編集操作」を範囲とし、§4.1 は各行を既存のプランにコンパイルすると書く。しかし `main` に次のものは無い。
    - `copy_sheet`（ADR-0018 G2、P7d）
    - テーブル名・テーブル列名の変更（ADR-0018 §5・§6）
    - 定義名の書き込み API
    - 計算結果を添えた数式の書き込み（ADR-0003 §3.1a は「A new formula has no cache」）
    - セルや行の移動・コピー、シートをまたぐ cut/paste（§5.6）
  - splice（§8.2）は「exactly one ADR-0009/0018 operation」にならない。ExcelJS の `spliceRows(start, count, ...inserts)` は、削除と挿入と書き込みの組み合わせである。Q2 (B) の「参照を書き換えずに移動する」操作は、Rust 側にまったく無い。
  - これらには Test oracle 1 の逐次参照（「既存の公開 Rust API を即時に呼ぶ」）が存在しない。そのため、この ADR の動機である欠陥 (d)（splice）が、等式でも「受理されたものはコンパイルできる」不変条件でも検証されない。
- 修正案: §4.1 に「`main` にある op」と「新たに Rust の操作を要する op」を分けて書く。後者は、どの ADR で定義する Rust 操作にコンパイルされるかと、その op の逐次参照を決める。決まるまでは範囲外とし、J2/J4 の前提として Delivery に書く。

### M2（Major）: 追記時検証と構造操作の dry run に、実装できる土台が書かれておらず、費用の主張にも根拠が無い

- 根拠:
  - §4.5 は、各 op を `View(t)` に対して逐次 API と同じガードで検証する。構造操作は `normalize(J + op)` に対して「ADR-0018 §2.1 step 1（read-only）」を実行するという。
  - しかし ADR-0018 の step 1 は「the tree as it is now」、つまり epoch ≤ k をすべて適用した後の木と、再構築した census・row index に対して動く（ADR-0018 §2.1、apply 後の rebuild）。Base は不変で Journal は未適用なので、構造操作を 1 つ追記するたびに、それより前のすべての epoch を作業コピーに適用する必要がある。read-only の dry run にはならない。
  - 費用は §4.5 が言う「the validation of the sequential operation costs」ではなく、Commit の前半ぶんになる。`insertRow` を繰り返すと O(n²) である。
  - §4.5 は「structural ops are rare」とも書くが、ExcelJS の `insertRow`/`insertRows`/`duplicateRow` はすべて `spliceRows` を呼ぶ（`lib/doc/worksheet.js` L423・L429・L469）ので、構造操作は稀ではない。
  - キー付き op のガードを View の上に作り直すと、それは §7 が否定した「第二の編集エンジン」の検証部分にあたる。逐次 API とずれれば、Test oracle 1 (i) が破れる。
- 修正案: 検証の土台を決める。たとえば「追記ごとに逐次 API で更新する影の `Workbook`（直列化しない）」と「View 上の再実装」から選ぶ。構造操作の追記費用は、spike で測った値として書く。

### M3（Major）: Test oracle 1 の等式が、畳み込みと戻し書きで設計上成り立たず、比較の単位がその穴を塞いでいない

- 根拠: 生成器は「Base の値への戻し書き」を含み（Test oracle 1 values）、同じキーへの複数回の書き込みも当然生成する。次のどの例でも、逐次参照と Journal で比較の単位 (b) が一致しない。
  - A1 に数式 `=1` を書き、次に定数 5 を書く。逐次参照では、最初の書き込みで `calcChain.xml` が削除され、`fullCalcOnLoad` が立つ（ADR-0003 §4.3、§3.3 項 2）。Journal では N1 で定数書き込みだけが残るので、calcChain が残る。part の集合が違う。
  - 依存先を持つセルに別の値を書き、Base の値に戻す。逐次参照では依存先のキャッシュが削除される。Journal では `noop_at_commit` によって何もしない（§9.2、Test oracle 5）。逐次参照の側はバイト同一にならないので、Test oracle 1 と Test oracle 5 が互いに矛盾する。
  - 同じセルに文字列を 2 回書く場合と、書式パッチを 2 回当てる場合。逐次参照では `sst` と `cellXfs` に未使用の項目が残る（Layer 3 は追記のみ）。単位 (b) が許すのは「shared-string index order」と「shared-group hand-off」の違いだけなので、項目数と `@count` の違いは許されない。
  - 単位 (a)（バイト等価）の条件「新しい shared string が無く、shared-group の member に書かない」では、上の calcChain と `cellXfs` の差を除けない。

  実装者はこの等式を通すために、その場その場で比較を緩めることになる。それが等式を骨抜きにする。
- 修正案: 等式の右辺を「逐次 API に `normalize(J)` の op を順に与えた結果」と定義し、左辺の「J をそのまま逐次に適用した結果」とは別の性質として扱う。後者との差は、許される差の一覧として ADR に列挙する。列挙するのは、calc 状態（calcChain の有無、`fullCalcOnLoad`、削除されたキャッシュの集合が逐次側の部分集合であること）と、未使用の `sst`/`xf` の項目である。それ以外の差は失敗とする。

### M4（Major）: Commit の作業コピー（§7 step 3）が、`main` に無い仕組みを前提にしており、ADR-0018 が退けたクローンの費用を扱っていない

- 根拠:
  - §7 step 3 の「a shadow of the Base, sharing every part by `Arc` until it is changed」に当たるものは `main` に無い。`Workbook`（`workbook.rs:177-`）は `Package` と、census・索引・registry を所有する可変の構造である。`Package` は `ZipArchive` を持ち、パース済みの part を `Arc` で共有していない。
  - ADR-0018 §2.1 は、200k 数式のシートを staged clone すると 289 ms かかり、木のメモリが 2 倍（1M セルで 1.44 GiB）になるとして、ワークシートのクローンを退けた。
  - 作業コピーを Base のバイト列から開き直す方式にすると、Commit ごとに census を全部やり直す（ADR-0003 §1）。それでも View 用のパース済み Base とあわせて、メモリは 2 つ分になる。どちらの方式でも、§3.8 の 2× 目標（ADR-0018 Q10）との関係が書かれていない。
  - 「Untouched parts are never parsed again」は、構造操作が全ワークシートをパースする（ADR-0018 §2.1 step 1）こととも合わない。
- 修正案: 作業コピーの実現方法（開き直し、または `ooxml-core` に新設する COW）を決める。新設するなら、その core の変更を Delivery に入れる。メモリ上限と、ADR-0018 §3.8 のベンチとの関係を書く。

### M5（Major）: Commit をまたぐ `truncate` と、トランザクション中の save が未定義

- 根拠: §10.1 では、Commit が成功するとキー付きエントリが Base に畳まれ、Journal から消える。その後に、Commit 前の seq を指定して `truncate(seq)` すると、Commit 済みの op はもう Journal に無いので戻らない。ADR-0023 の `with wb.transaction():` の中で save して例外が起きた場合、ロールバックしたつもりで、保存済みの値が View に残る。`Ok` のまま黙って誤る。
- 修正案: 次のどちらかを決め、エラー種別と Test oracle 8 のケースを足す。
  - 最後の Commit の seq より前への `truncate` は拒否する。
  - トランザクション中の save を拒否する。

### m1（Minor）: ExcelJS 4.4.0 の `spliceRows` は、セルの数式を書き換えない（確認済み）。Q2 (B) の説明を事実に合わせる

ExcelJS 4.4.0 の `lib/doc/worksheet.js` L483-548 では、行の `values`・`style`・`height` を移し替え、最後に `definedNames.spliceRows` を呼ぶ。プローブで確かめた結果は次のとおり。

- 準備: `B5=7`、`A1={formula:'B5*2'}`、`C6={formula:'SUM(B2:B5)'}`、名前 `nm = S!$B$5`、`B5` にノート。
- 操作: `spliceRows(2, 0, ['ins'])`。
- 結果:
  - `A1` は `B5*2` のままで、reload 後も同じ。値は `B6` に移った。
  - `C7` は `SUM(B2:B5)` のまま。
  - 名前は `S!$B$6` に書き換わった。
  - ノートは `B5` からも `B6` からも消えた。

Q2 (B) の「rewrites no reference elsewhere」は、定義名については事実と違う。§8.2 と Context の「未確認」は、この結果で置き換えられる。

### m2（Minor）: Q1 (A) の「supplied result を `<v>` に残す」は、既存の規則ではない

Q1 の既定案の理由は「ADR-0003 と ADR-0018 が既に適用している規則」だが、ADR-0003 §3.1a は「A new formula has no cache」と書いている。また `main` には、計算結果を添えて数式を書く API が無い。supplied result を残すかどうかは、ADR-0003 を変更する新しい判断として、Q1 の別の選択肢に分ける。

### m3（Minor）: id ベクトルの端と、コンパクション後の `Base(r)` の定義

- §3 では `Base(r)` を「Base ファイルでの行番号」と定義している。しかし §10.1 のコンパクション後は、ベクトルが写像になるので、この定義は成り立たない。
- insert では、末尾 n 個の id がシートの外に押し出される。その扱い（ADR-0018 §3.2 の `ShiftOffSheet` と clip）が §5.2 に無い。

### m4（Minor）: N3(iii) の read set が例示（「…」）で終わっている

Test oracle 9 の変異だけでは、read set が過不足なく定まらない。ADR-0018 §3.2 の挿入拒否（末尾 n 行にある `row` 要素や、内容を持つオブジェクト）、ガードとして §2.4 と §8 が読むもの、§2.1.1 が生成する見出しを、すべて列挙する。

### m5（Minor）: `r` の無い行やセルへのキー付き書き込み

§3 は「`r` の無い行やセルには stable id が無い」とするが、それらへのキー付き書き込みをどう扱うか（拒否するのか、暗黙の位置を id にするのか）を書いていない。逐次 API が受理する値書き込みを Journal が拒否すると、Test oracle 1 (i) が破れる。

### m6（Minor）: §11 で、バッファしたフラッシュが途中で拒否されたときの状態

i 番目の op が拒否されたとき、i より前の op が追記済みになるのかどうかが書かれていない。呼び出し側には成功を返した後なので、`CommitInternal` 級であっても、Journal の状態（全部か無しか）を決めておく。

### n1（nit）

Revision history は研究ノートの未決 7 を §5.4 に対応させているが、id の永続性を決めているのは §5.1 と §5.6 である。

## その他の確認結果（指摘なし）

- §8 の未決 1〜3 は、選択肢・既定案・理由のそろった Q1〜Q3 になっている。4〜7 は §4.5、§10、Test oracle 1、§5 で決まっている。8（docx/pptx）は §1 で範囲外にしている。Q2 の既定表は、ADR-0018 §3.2 と合っている。
- §13.3 が指す ADR-0021 の節（"Mutation and ownership"、"Rust changes and transaction choice"、"Cell and range projection"、"Acceptance criteria" 4・5）と ADR-0022 の節（"Decision" 1 段落目、"ExcelJS row splice …" ほか）は実在し、引用も合っている。68.8 / 750 ms と「Reopening the snapshot dominates」も、ADR-0022 の notes と一致する。
- `CellWritePlan`、`SharedFormulaMaster`、`RegistryPlan`、`ReferenceDeleted`、`KeepStaleCaches`、`WriteIndexMap`（ADR-0018 §2.1.1 の index map）は、`main` に実在する。`Workbook::fork()` は `main` に無く、ADR-0023 の提案である。本文の書き方はこの事実と合っている。
- 本文に、ローカルパス、ユーザー名、非公開プロジェクト名は無い。「67 entries, 58 verified」は、リポジトリの外にある書誌に依拠した数値である。

## 判定

converged: no（Blocker 3 / Major 5 / Minor 6 / nit 1）
