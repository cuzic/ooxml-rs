# ADR-0024 レビュー（Opus, round 3）

対象: `plan/xlsx-edit-journal` の未コミットの `docs/adr/0024-xlsx-edit-journal.md`（revision 3）。
PR はまだ無い。コードは `main` の `f88d02c` で照合した。プローブは `ooxml-xlsx` を path 依存にした使い捨てのバイナリで、コミットしていない。

## 結論（先に）

round 2 の M1〜M5 は、規則としてはすべて解消した。N3 (iv) の開始状態は、構造操作が読む場所（全シートの数式セル、編集シートの guard zone、registry place は N7）を覆っている。反例は見つからなかった。

ただし、新しく入った規則どうしがぶつかる箇所が 2 つ残る。

- §7.1 の「存在しないセルでは内容を単独で先に書く」と「1 回の呼び出しの 2 つの半分は 1 回の書き込みを共有する」が、N1 の畳み込みで `set_datetime` の内容の半分が置き換わったときに衝突する。どちらを優先しても、片方の入力で `Ok` のまま別の `xf` になる（M1）。
- N4 が捨てる `set_dynamic_formula` は、逐次側に `metadata.xml` を残す。P は part の集合を比較するので、oracle 1 の等式が規則どおりの実装で破れる（M2）。

**converged: no**

## 前ラウンドの解消状況

- M1: 解消。N3 (iv) は「epoch の開始時に数式だったセル（どのシートでも）は動かさない」とし、構造操作が読む集合を本文に列挙した（`grid.rs` の `plan_formulas`・`rewrite`、`scan_grid_part`、`delete_sheet_strict` と照合）。fixture・固定ケース・oracle 9 の変異もそろった。
- M2: 解消。N1 の表で `remove_formula` と `write_cells` の `RemoveFormula` を conditional にした。conditional は何も消さず、後のバッチに置く。固定ケースもある。
- M3: `set_number` → `set_num_format` の形は解消した。ただし `set_datetime` の畳み込みで同じ穴が別の形で残る（新しい M1）。
- M4: 解消。呼び出し単位で全か無かにし、replay で戻す。戻す対象の列挙に漏れがある（m2）。
- M5: 解消。フラッシュは全か無かで、`JournalBroken` を導入した（Q6）。エラーの種類の記述が食い違う（m1）。
- m1: 解消（§6.1）。
- m2: P をモデルとして列挙した。ただし `dimension` を外す理由づけが事実と合わない（m3）。
- m3: 解消（終了 `seq` を保存し、truncate で消す。oracle 2 に rollback のケースがある）。
- m4: 解消（§3.1）。
- n1: 解消（`JournalInternal { seq, source }` に統一）。

## Findings

### M1（Major）: 存在しないセルで `set_datetime` の内容の半分が畳まれると、§7.1 の 2 つの規則が衝突し、どちらを優先しても `xf` が逐次と食い違う

- 根拠:
  - §7.1 には次の 3 つの規則がある。
    - 「a folded full entry takes the place of the key's **first** content op」
    - 「If the start state is `Missing` and the first op is content, the content is written alone first … every patch resolves on 0」
    - 「The two halves of one call (`*_with_format`, `set_datetime`) always share one write」
  - `*_with_format` のスタイルは `StyleRequest::Format`（full）なので、土台を読まない。土台を読むのは `set_datetime` の `EnsureDate`（conditional）だけである（`setters.rs` L230-236）。
  - N1 によって `set_datetime` の内容の半分が別の op に置き換わると、「同じ呼び出しの半分」はもう存在しない。そのとき、上の 2 番目と 3 番目のどちらの規則を当てるかを ADR は決めていない。そして、どちらを当てても片方の入力で誤る。
- プローブ: Base は `st1.xlsx`。B 列に `col style="1"`（xf 1 = 太字）があり、`B1` と `B2` は存在しない。

  | 列 | 逐次の結果 | 2 番目の規則（内容を単独で先に書き、`EnsureDate` は 0 の上） | 3 番目の規則（共有した 1 回の書き込み。土台は列の既定） |
  |---|---|---|---|
  | X: `B1.set_datetime(d)` → `B1.set_number(5)` | `xf{numFmtId=14, fontId=1}` | `xf{14, fontId=0}`（**誤り**） | `xf{14, fontId=1}` |
  | Y: `B2.set_number(5)` → `B2.set_datetime(d)` | `xf{14, fontId=0}` | `xf{14, fontId=0}` | `xf{14, fontId=1}`（**誤り**） |

  - X: N1 で `set_number` が `set_datetime` の内容の位置を取る。最初の op はスタイルの半分を持っていたので、逐次は列の既定の上に `EnsureDate` を解決した。
  - Y: N1 で `set_datetime` の内容が `set_number` の位置（最初の内容 op）を取る。逐次では、`EnsureDate` は `s=0` のセルの上で解決された。
  - どちらの場合も `Ok` のまま、P の resolved style が食い違う。§4.4 の「the result resolves to the same `xf` as the sequential chain」が成り立たない。
- 修正案:
  - 存在しないセルでの土台を、**畳む前の最初の op** で決めると書く。
    - 最初の op がスタイルの半分を持つ（`set_datetime`、`*_with_format`、パッチ）なら、最初のパッチは列・行の既定の上で解決する（内容と 1 回の書き込みにしてよい）。
    - 最初の op が内容だけなら、内容を単独で先に書き、パッチはすべて 0 の上で解決する。
  - あるいは、開始状態が `Missing` のキーでは、最初の op を N1 の畳み込みの対象から外す。
  - oracle 1 の固定ケースに X と Y を足し、oracle 9 に「畳んだ内容を最初の op の土台規則ごと置き換える」変異を足す。

### M2（Major）: N4 で捨てた `set_dynamic_formula` は、逐次側に `metadata.xml` を残す。そのため oracle 1 の P が規則どおりの実装で一致しない

- 根拠:
  - `set_dynamic_formula` は内容 op（§4.1）だが、workbook 単位の副作用を持つ。metadata の part が無ければ `xl/metadata.xml` を作り、relationship と content type を足す。part があれば、そこに `XLDAPR` の型・ブロックを足す（`dynamic.rs` の `ensure_block`、L200-250）。
  - そのセルに後から書き込むと `ArrayAnchor` で拒否されるので、N1 では消えない。しかし、行・列・シートの削除で id が死ぬと、N4 がこのエントリを捨てる。Commit は Base から作り直すので、`metadata.xml` が生まれない。
  - P は「the part set, relationships and content types except `calcChain.xml`」を比較し、それ以外の part をバイト単位で比べる。除外表には metadata の行が無い。
- プローブ: Base は `st1.xlsx`（metadata の part なし）。逐次で `A5.set_dynamic_formula("1+1")` → `delete_rows(4, 1)` とすると、どちらも `Ok` で、出力には `xl/metadata.xml` が残る。Journal の経路では、N4 がエントリを捨てるので part が生まれない。part の集合が食い違い、oracle 1 (iii) が落ちる。
  - 生成器は「every part A row」と「cells earlier ops wrote」への偏りと、構造操作を 5 回に 1 回引く。そのため、この形はすぐに出る。
  - 同じ形は、`delete_sheet` による N4 でも起きる。
- 修正案: 次のどちらかを決めて書く。
  - P の除外表に「どの `c/@cm` からも参照されない metadata のブロック・part・関係」を足し、述語を「参照される `cm` はすべて範囲内」とする。
  - あるいは、`set_dynamic_formula` の metadata の半分を、N4 で消えない barrier として扱う。

  どちらの場合も、oracle 1 の固定ケースに上の列を入れる。
  あわせて、内容 op の副作用（sst・style・metadata）を一覧にし、除外表と突き合わせたことを §4.4 に書く。

### m1（Minor）: フラッシュの失敗を呼び出し側がどう見るかの記述が 3 か所で食い違う

- §4.5 は、多段の呼び出しが拒否されたら「the call returns that error」（拒否のエラーそのもの、たとえば `StyleTableFull`）と書く。一方、§11 は、フラッシュを起こした呼び出しが `JournalInternal { seq, source }` を投げると書く。
- oracle 3 は、フラッシュの失敗のあと「View reads … as before the call」を確かめる一方で、「every call but `truncate`/reload returns `JournalBroken`」とも書く。読み出しも呼び出しなので、読み出しを比較できない。
- 修正案:
  - §4.5 に「フラッシュは例外で、`JournalInternal` を返す（`source` に拒否を入れる）」と書く。
  - oracle 3 の比較は、内部状態（log、カウンタ、shadow の `to_bytes`）にする。

### m2（Minor）: 呼び出し単位のロールバックで戻すものの列挙に漏れがある。また、id のカウンタを巻き戻す規則が §5.1 と oracle 6 の言い方とぶつかる

- §4.5 と §9.3 は「log, the `seq` and id counters and the shadow」を戻すとしか書いていない。splice の delete が成功したあとで insert が拒否されたときは、次のものも戻さないと、id → 位置の対応がずれる。
  - §5.2 の vectors
  - §5.5 の tombstone の集合
  - §6.1 の echo の終了（「its id dies」で立つ）

  oracle 3 の「View reads」で検出はできるので、Minor とした。
- 呼び出しの中で割り当てた id を、カウンタを戻して再発行する規則がある。これが §5.1 の「never reused」と、oracle 6 の「no id is ever issued twice across … replays」に反するように読める。
- 修正案:
  - 戻すものに vectors、tombstone、echo の終了を足す。
  - 「issued は呼び出しが `Ok` で返ったときを指す」と定義する。あるいは、カウンタを戻さず、oracle 3 の比較からカウンタを外す。

### m3（Minor）: `dimension` を P から外す理由「consumer が読むものはすべて P にある」が事実と違う

- SheetJS は `<dimension ref>` を `!ref` として読む。ADR-0021 の SheetJS adapter も `!ref` を射影する。
- `dimension` は、書き込みで広がり（`worksheet.rs` L306）、構造操作で動く（`locations.rs` L400）。そのため、2 つの経路で値が食い違う。たとえば Base が `A1:C5` のとき、逐次で `D10` に書いてから行 10 を削除すると `A1:D9` になる。Journal の経路では、N4 がその書き込みを捨てるので `A1:C5` のままになる。
- 述語（「すべての `c` を覆う」）はこの差を通す。ところが View の `!ref` は shadow のもの（`A1:D9`）なので、保存して読み直すと `!ref` が変わる。
- もう 1 点ある。数式セルの「value, value type」は P に入っているが、`<v>` と `t` は calc state として除外されている。数式セルの value がどちらに属するかが書かれていない。
- 修正案:
  - `dimension` の差を、意図した差として、§6.1 の Reload 法則の例外と differential harness に書く。または、Commit で `dimension` を shadow の値に合わせる。
  - 数式セルの value と `t` は、cache の除外に属すると明記する。

### n1（nit）

oracle 9 の項目（L883-886）の字下げが崩れていて、リストの続きに見えない。

## その他の確認結果（指摘なし）

- N3 (iv) を次の観点で崩そうとしたが、反例は無かった。
  - 定義名、CF・DV、テーブル: barrier か part B なので、N7 で動かない。
  - 他シートの数式: (iv) が覆う。
  - `move_sheet`・`add_sheet` の 3-D 範囲の検査（`check_three_d_order`）: 拒否は shadow が先に行う。Commit では、数式が少ない状態で同じ操作を走らせることになり、受理が増える方向にしか変わらない。また、書かれるテキストは同じである。
  - `scan_grid_part`: `<v>` と `<is><t>` は許可されている。
  - shared group の hand-off: 数式のまま残る。
  - `merge_clearing` のあとの開始状態: shadow から取るので、barrier の効果を含む。
  - 複数 epoch にまたがる移動: 移動したキーのセルは、Commit の順序では開始状態のまま。
- N1 の conditional を後のバッチに回す規則は、`plan_writes` が `RemoveFormula` を対象の数式の有無で判定すること（`edit.rs` L420）と整合する。バッチの分け方で結果は変わらない。配列とデータテーブルのセルへの書き込みは `ArrayAnchor`・`ArrayResultCell` で常に拒否されるので、バッチ内の依存で拒否が増えることもない。
- 存在しないセルで「パッチが先、内容が後」の場合（1 回の書き込みにまとめ、土台は既定）と、「内容が先、パッチが後」の場合（内容を単独で書き、パッチは 0 の上）は、`*_with_format` を含めて逐次と一致する。`*_with_format` は full なので土台を読まない。食い違うのは M1 の `set_datetime` の形だけである。
- `Err` ⇒ byte-identical と「受理されたものは必ずコンパイルできる」は両立する。バッファした op は追記されていないので、不変条件の対象外である。
- 受け入れ条件について。proptest の等式（i〜iv、冪等性）、View 法則・Reload 法則・rollback、`Err` ⇒ byte-identical（拒否・故障注入・途中拒否）は、それぞれ検証できる形で書かれている。M2 と m1 の点を除けば、矛盾は無い。
- Q1〜Q6 には、選択肢・既定案・理由がそろっている。
- 本文に、ローカルパス、ユーザー名、非公開プロジェクト名は無い。数値はすべて round 2 で出典を確認したものである。

## 判定

converged: no（Blocker 0 / Major 2 / Minor 3 / nit 1）
