# ADR-0024 レビュー（Opus, round 2）

対象: `plan/xlsx-edit-journal` の未コミットの `docs/adr/0024-xlsx-edit-journal.md`（revision 2）。
PR はまだ無い。コードは `main` の `f88d02c` で照合した。プローブは `ooxml-xlsx` を path 依存にした使い捨てのバイナリで、コミットしていない。

## 結論（先に）

影の Workbook（shadow）を入れたことで、round 1 の B1・B2・M2・M4・M5 は骨格のレベルで解消した。「拒否は逐次 API と一致する」は、追記時には構成上成り立つ。

ただし、Commit 側の 3 つの変形（N3 の移動、N1 の畳み込み、§7.1 の同一セルの併合）が、shadow とは別の順序で逐次 API を呼ぶ。その結果、shadow が受理したものが Commit で拒否される場合と、`Ok` のまま結果が逐次と食い違う場合が残っている。いずれもプローブで再現した（M1〜M3）。

さらに、複数の呼び出しからなる splice の原子性（M4）と、フラッシュの途中拒否の規則（M5）が、`Err` ⇒ 無変更と矛盾する。

**converged: no**

## 前ラウンドの解消状況

- B1: 解消。数式は生まれた epoch の座標で書き、書き換えは ADR-0018 の操作だけが行う（§5.3, §7 step 4, §8.1）。step map は数式テキストに使わない。
- B2: 解消。ログは畳み込まず、key index は truncate で pop する（§4.3）。`set(k,1)`, `set(k,2)`, `truncate(1)` は 1 を読む。
- B3: 書式パッチの合成は解消。full / partial / conditional の分類は `registry/styles.rs` L489-511 と一致する（`Named` は `p.named(wb, name, current)` で current を読むので、partial で正しい）。ただし「内容の op は `s` を読み書きしない」という前提が、存在しないセルでは偽である（新しい M3）。
- M1: 解消。part A の 40 個の名前（`set_string_inline`, `remove_formula`, `merge_clearing`, `set_row_outline`, `move_sheet`, `delete_sheet_strict` ほか）は、すべて `main` に `pub fn` として実在する。`CellOp::Clear` も実在する（`edit.rs` L39）。
- M2: 解消（shadow 上での逐次呼び出し）。
- M3: おおむね解消。P の範囲に小さな穴がある（m2）。
- M4: 解消。`Workbook::from_bytes(impl Into<Arc<[u8]>>)` は実在する（`workbook.rs` L230）。木は同時に 1 つだけである。
- M5: 解消（`TruncateBeforeCommit` と Q5）。
- m1: 解消（§8.2）。
- m2: 解消（Q1 (D)）。
- m3: 解消（§3.1, §5.2）。
- m4: 規則は保守側に書き換えた。しかし「guard zone は ADR-0018 が読むものより広い」という主張が偽である（新しい M1）。
- m5: 書き換えたが、事実と違う（新しい m4）。
- m6: 規則は決めた。しかし `Err` ⇒ 無変更と矛盾する（新しい M5）。
- n1: 解消。

## Findings

### M1（Major）: N3 の guard zone は、ADR-0018/0009 が読むものより狭い。そのため、受理済みの Journal が Commit で拒否される

- 根拠:
  - §4.4 は「The guard zone of N3 (iii) is wider than what ADR-0018 reads」と書く。しかし guard zone に入るのは「on the edited sheet」の merge・table・array・shared group などだけである。
  - 一方、構造操作は**全シートの全数式**を書き換え、次の場合に拒否する（`sheet_ops/grid.rs` L250-L283、`rewrite_error` L302）。
    - 書き換えられない数式（3-D 参照 `Prefix::Sheets`、lex できない数式）: `UnrewritableReference`
    - 端からはみ出す参照: `ShiftOffSheet`
  - `delete_sheet_strict` は、他のシートからの参照が残っていると `SheetInUse` で拒否する（`sheet_ops/mod.rs` L1255-1260）。
  - このため、Base の数式セルを定数で上書きしてから構造操作をする、という普通の流れで次のことが起きる。逐次（shadow）では、数式が消えたあとに構造操作が走るので受理される。N3 は定数（座標テキストなし、編集対象シートの外）を次の epoch へ動かすので、Commit では構造操作が Base の数式を見て拒否する。結果は `JournalInternal` で、§4.5 の不変条件が破れる。
- プローブ: Base は `Sheet1` と `S2` の 2 シート。各行について、2 つの順序を比べた。
  - (a) 逐次: `S2!A1` に定数 5 を書いてから構造操作
  - (b) N3 の順序: 構造操作を先に行う

  | `S2!A1` の Base の数式 | 構造操作 | (a) 逐次 | (b) N3 の順序 |
  |---|---|---|---|
  | `SUM(Sheet1:S2!A5)` | `Sheet1.insert_rows(0,1)` | `Ok` | `Err(UnrewritableReference { "xl/worksheets/sheet2.xml", "A1 formula" })` |
  | `Sheet1!A1048576` | `Sheet1.insert_rows(0,1)` | `Ok` | `Err(ShiftOffSheet { "xl/worksheets/sheet2.xml A1 formula" })` |
  | `Sheet1!A5*2` | `delete_sheet_strict("Sheet1")` | `Ok` | `Err(SheetInUse { sheet: "Sheet1", count: 1 })` |

- Test oracle 1 の生成器の fixture（§Test oracles 1）には、3-D 参照、端の参照、`delete_sheet_strict` で参照されるシートが無い。そのため、oracle 1 も oracle 3 の不変条件もこの穴を見つけない。
- 修正案:
  - N3 (iii) に、次の条件を足す。「エントリが書く前の、そのセルの状態が数式である（シートを問わない）なら動かさない」。あわせて、「ADR-0018 より広い」という主張を、実際に読む集合（全シートの数式セル、全 registry place）に合わせて直す。
  - 生成器の fixture に、上の 3 つの形を入れる。
  - oracle 9 に、変異「数式セルを上書きするエントリを動かす」を足す。

### M2（Major）: `remove_formula` は条件付きの内容 op なのに、N1 の「後勝ち」で畳まれる。そのため、古い値が黙って生き返る

- 根拠:
  - `remove_formula` は「A cell without a formula is left alone」（`worksheet.rs` L760-763）。つまり、現在の内容を読む条件付きの op である。
  - N1 は、内容のキーについて、単純に後の op を勝たせる。§4.4 は、書式については conditional（`EnsureDate`）を区別した。しかし、内容の側には同じ区別が無い。
- プローブ: Base は `A1=3`。
  - 逐次: `set_formula("1+1")` → `remove_formula()`。結果は `<c r="A1"></c>`（空）。
  - N1 で畳んだ形（`remove_formula` だけを Base に適用）: 結果は `<c r="A1"><v>3</v></c>`。

  どちらも `Ok` で、値が食い違う。
- 修正案:
  - 内容 op を、full（値・数式・`Clear` で置き換える）と conditional（`remove_formula`）に分ける。conditional は直前の内容を消さない。
  - 例えば「conditional の直前に full があれば、2 つを 1 つの full に合成する（数式なら値なしのセルにする）」という規則を書く。
  - oracle 1 の固定ケースに上の列を入れる。

### M3（Major）: 存在しないセルへの「内容 → 書式パッチ」の併合（§7.1）は、逐次と別の `xf` に解決される

- 根拠:
  - §7.1 と §4.4 は「a content op never reads or writes `s`」を、併合と独立性の根拠にしている。
  - しかし、存在しないセルに内容を書くと、`s` の無い `c` ができる（実効 0）。その後のパッチは 0 を土台にする。
  - 併合した 1 回の書き込みでは、`registry_cell_state` が存在しないセルの current を `missing_cell_style`（行の `customFormat` の `s`、または `col/@style`）とする（`edit.rs` L187-193, L995-1009）。
- プローブ: `col min=2 max=2 style="1"` の列（xf 1 は太字）の `B1` に、2 つの順序で書いた。
  - 逐次: `set_number(5)` → `set_num_format("0.00")`。`set_number` の直後の `style_index` は 0 で、結果は `xf{numFmtId=2, fontId=0}`。
  - パッチを先に解決する順序（併合した書き込みと同じ current）: 結果は `xf{numFmtId=2, fontId=1}`（太字）。

  P の resolved style で差が出る。`Ok` のまま黙って誤る。
- 修正案:
  - §7.1 の併合条件に、次のどちらかを足す。
    - 「その時点の作業コピーでセルが存在する」
    - 「内容が先、パッチが後のときは、パッチを `s=0` を土台に解決する」
  - §4.4 の「never reads or writes `s`」を、「存在しないセルを作る内容 op は `s` を 0 にする」と直す。
  - oracle 1 の生成器に「書式つきの行・列にある、存在しないセル」を入れる。

### M4（Major）: splice（Q2 (A)）は 3 回の呼び出しなのに、shadow 上で原子的に扱う規則が無い

- 根拠:
  - §8.2 は、splice を `delete(p,d)`、`insert(p,i)`、`i` 行の書き込みの 3 段にし、逐次参照も同じ 3 回の呼び出しとする。
  - §4.1 の表は「one ADR-0009/0018 operation each」と書いている。§4.5 は「`Err` → Journal and shadow unchanged」を、1 回の呼び出しが原子的であることで保証している。
  - splice では、delete が成功したあとに、insert が拒否されることがある（M1 の `ShiftOffSheet`、端の数式）。挿入行への書き込みが拒否されることもある（長すぎる文字列、merge に入る書き込み）。
  - そのとき shadow には delete だけが適用され、op は追記されない。shadow とログが食い違い、以後の normalized 読みと Commit の出力が一致しなくなる。
  - 逆に、拒否を Journal の側だけで扱うと、oracle 1 の逐次参照（3 回の呼び出し）は途中の状態を残す。どちらにしても (i)〜(iii) が定まらない。
- 修正案: 次のどちらかを決め、oracle 1 の逐次参照もその定義に合わせ、oracle 3 にケースを足す。
  - splice の途中で拒否されたら、shadow をログから再構築する（§3.2 の replay）。
  - 3 段を 1 つの合成操作として先に検証する。

### M5（Major）: フラッシュの途中拒否で「前半を残す」規則は、`Err` ⇒ 無変更を破り、成功と伝えた op を黙って捨てる

- 根拠:
  - §11 では、op `i` が拒否されると、`i` より前は追記済みのまま残り、`i` 以降は捨てられ、`JournalInternal` が送出される。
  - バッファした op は、すべて呼び出し側に成功と返してある。したがって、`i` 以降も呼び出し側にとっては受理済みである。
  - フラッシュを起こした呼び出し（getter や `writeBuffer()`）は `Err` を返す。それなのに Journal は変わっている（前半が追記された）。これは §9.3 の表（`JournalInternal` の Commit では「Base and log unchanged」）とも、ADR-0001 の `Err` ⇒ 無変更とも合わない。
  - 次の `save` は前半だけを書く。呼び出し側が成功と思っている後半は黙って失われる。
  - 「never a normal refusal」の前提も弱い。受理は状態に依存する。たとえば `StyleTableFull`（`styles.rs` L516-526）や、前の op の結果に依存する guard は、JS 側の分類器では確かめられない。
- 修正案:
  - 途中で拒否されたら、フラッシュ前の `seq` まで truncate して、Journal を全部か無しかにする。
  - そのうえで Journal を「保存不可」とし、truncate か reload まで `save` を `JournalInternal` で拒否する（fail closed）。
  - oracle 3 に「フラッシュの i 番目の拒否」のケースを足す。

### m1（Minor）: View から読んだ `style_index` は、Commit をまたぐと意味が変わる

`CellRef::style_index()`（`worksheet.rs` L94）は shadow の index を返す。Commit では、full パッチが前のパッチを落とし、no-op を省くので、セッション中に作られた `xf` の index が shadow とずれる。

保存前に読んだ index を保存後に `set_style_index(i)` へ渡すと、`i` は新しい Base の別の `xf` を指し、`Ok` のまま別の書式になる。§4.4 の「by content」は Journal 内部だけを扱っており、外に渡した index は扱っていない。

修正案は次の 2 つ。
- 「registry の index は Commit までしか有効でない」を §6.1 と §12 に書く。
- reload 法則の対象から `s` の index を外す。

### m2（Minor）: P の比較モデルが不明確で、隠れた情報が含まれていない

P の include の一覧には、次のものが無い。
- Q3 の h1（`c`/`row`/`col` の未知の属性と拡張の子要素）
- `dimension`
- `docProps/app.xml` の中身
- sheet view と selection（barrier op）

一方で、本文は「Any other difference fails」とも書いている。インデックスの差を許す以上、比較はモデルの上で行うしかない。そのモデルの要素を列挙し、h1 を入れる。ADR-0001 が保持を約束し、Q3 の既定案が「keep」としているものが、等式の外に出ている。

### m3（Minor）: truncate が「echo の終了」を元に戻さない

§6.3 の「echo ended」は §6.1 の契機で立つだけで、§4.3 と §10.3 は key index しか pop しない。次の順で操作したとする。

1. `set(k, x)`
2. `set(k, y)`（seq 1 の echo が終わる）
3. `truncate(1)`

このあと、`k` は raw の `x` ではなく normalized の読みを返し、View 法則が rollback 後に破れる。また、oracle 8 は出力だけを比べている。

修正案は 2 つある。
- 終了フラグをログから導く（truncate 後に再計算する）。
- oracle 2 に rollback 後の echo を足す。

### m4（Minor）: round 1 の m5 への対応の説明が事実と違う

§3.1 と Revision history は、「`r` の無い行やセルにも暗黙の位置で id を与えるので、keyed writes work there as in the sequential API」と書いている。しかし `main` では、census が `EditBlock::MissingReference` でそのシートの編集を止める（`census.rs` L521、`index.rs` L182-190）。そのため、逐次 API はそこへの書き込みを拒否する。

shadow を通すので挙動は一致する。直すのは説明だけで、「逐次 API と同じく拒否される」とする。

### n1（nit）

§11 の `JournalInternal { seq }` と、§12 の `JournalInternal { seq, source }` で、フィールドが食い違っている。

## その他の確認結果（指摘なし）

- shadow と「Base は不変」は矛盾しない。shadow は Base から再構築できるキャッシュで、Base のバイト列には触れない（§3.2）。
- 「木は 1 つだけ」という主張も、§7 step 2 で shadow を落とすので成り立つ。Commit が失敗したときと truncate のときの shadow は、作り直し（replay）であり、元に戻す操作ではない。これは費用を J0 で測ると書いてあり、整合している。
- 列の幅（`<col>` の span）を insert_cols の前に書いても後に書いても、結果は同じだった（プローブで確認。挿入した列は両方とも span の外になる）。行・列の属性に N3 を当てることに、反例は見つからなかった。
- バッチ内の依存先による拒否（`IterativeCalculation`、`RichValueCell`、`MacroSheetDependent`）は、同じバッチで書いたセルを `others` から除いている（`edit.rs` L530-549）。そのため、併合で逐次より拒否が増えることはない。
- Q1〜Q5 は、選択肢・既定案・理由がそろっている。既定案は ADR-0003 §3.1a/§3.3 と ADR-0018 §3.2/§3.7 に合っている。Q2 (A) と ExcelJS の差（セルの数式を書き換える、ノートを残す）は、意図した差として書かれている。
- 数値（289 ms、1.44 GiB、0.29 s、15 sheets、68.8 / 750 ms）は、ADR-0018（L208, L384-385, L1306）と ADR-0022 の notes に出典がある。
- 本文に、ローカルパス（`/home`、`/tmp`）、ユーザー名、非公開プロジェクト名は無い。

## 判定

converged: no（Blocker 0 / Major 5 / Minor 4 / nit 1）
