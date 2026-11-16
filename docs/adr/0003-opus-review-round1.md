# ADR-0003 レビュー（Opus, round 1）

対象: `docs/adr/0003-xlsx-phase1-formula-and-calc-state.md`（Status: Proposed, 2026-11-16, main）

方針: 読み取り専用の敵対的レビュー。ADR-0001（Layer 2 の dirty 規則、Layer 3 の不変条件、R2-M3/R3-M1）、ADR-0002（§4.2 のプリミティブ、§4.3、§5 の allowlist、§6）、roadmap `docs/roadmap/xlsx.md`（§4、§5、Phase1、§7）との整合を確認した。

ADR が挙げる数値は、POI `test-data/spreadsheet` @ `47dcff76` の手元コピー（367 ファイル）に `spikes/xlsx/formula_stats.py` をそのまま再実行して確かめた。そのうえで、レビュー用のスクリプト（scratchpad の `review-0003/`。すべて `python3 -I` で実行し、fixture ディレクトリの外に置いた）で追加集計をした。

参照実装は手元のソースで確認した。calamine 0.38.0、openpyxl 3.1.5、rust_xlsxwriter 0.102.0、umya-spreadsheet 3.1.1、POI の `XSSFSheet.java` と `CalculationChain.java`。LibreOffice の挙動は LibreOffice/core master のソース（`sc/source/filter/oox/` 以下）で確認した。

重大度は3段階。
- **[Blocker]**: ADR の中心的な主張（「どの reader にも stale な値を見せない」）が成り立たない、または受け入れ試験でそれを確かめられない。
- **[Major]**: 設計の一部が誤っているか、未定義である。
- **[Minor]**: 記述の修正や、範囲の明記で済む。

判断の分かれる指摘には「判断」と書き、推論の段階にあるものには「推論」「要検証」と書いた。

## 結論（先に）

方向性は妥当。依存先のキャッシュだけを消し、`fullCalcOnLoad` を立て、式セルの集合が変わったときだけ `calcChain.xml` を部品ごと削除する、という三段構えは筋が通っている。数値も再現できた（126/341、115/11、shared 48、array 6、defined name 13、structured 10、cross-sheet 23、external 6、`INDIRECT`/`OFFSET` 2）。ADR が「未検証」としていた LibreOffice の挙動も、ソースで確認したところ ADR に有利な結果だった（m3）。

ただし、このままでは Accepted にできない点が2つある。

1. **依存規則が健全でない（B1）。** 「式のテキストに書かれた参照トークン＝前件」という規則は、テキストに現れない範囲を読む式を取りこぼす。具体的には次の6種類で、いずれも `Everything` にならず、キャッシュが stale のまま残る。
   - `SUMIF`/`AVERAGEIF` の `sum_range` が暗黙に伸び縮みする場合
   - 範囲演算子 `:` の片側が関数や名前である場合（`A1:INDEX(…)`、`A1:CHOOSE(…)`）
   - VBA UDF やアドイン関数
   - data table（`f t="dataTable"` にはテキストが無い）
   - `#SPILL!` で止まっている動的配列
   - 相対参照を含む定義名

   これは ADR の中心的な主張（L125-127, L230-231）に反する。
2. **受け入れ試験が取りこぼし（偽陰性）を検出できない（B2）。** §7 の判定（「oracle が予測するセルだけが `v` を失う」「依存先は空、それ以外は不変」）は、実装自身の依存集合と照合しているだけ。どの fixture にも、シートをまたぐ精密な依存、定義名、3-D 参照、推移的な連鎖が無い。ADR-0001 R2-M3 の動機になった `Sheet2!B1 = Sheet1!A1*2` すら試験されない。

Major は3件。
- `remove_part` が ADR-0001 Layer 3 の「registry からは削除しない」という不変条件と矛盾しているのに、改定が書かれていない（M1）。
- §3 の手順と §5 の guard の順序が規定されていない。途中で `Err` になると「値は書いたが依存先は消していない」状態が残りうる（M2）。
- shared formula の master 昇格で、`ref` の定義、`Err` の条件、範囲外への平行移動、試験の oracle（calamine の展開）が未定義または不適切（M3）。

---

## B1 [Blocker] 「参照トークン＝前件」は健全でない。テキストに現れない範囲を読む式が `Everything` にならない

### 根拠

ADR L81-95 は前件を `Cell`/`Range`/`Name`/`Everything` とし、「strings, numbers, operators, function names: ignored for dependency purposes」（L95）としている。`Everything` を返すのは列挙された関数と構文（L89-94）だけ。L125-127 は「a dependent formula reads as *no cached value* … never as a stale one」と主張している。

反例は次の6つ。

(a) **`SUMIF`/`AVERAGEIF` の暗黙のリサイズ。** Microsoft のドキュメント（SUMIF function）には、`sum_range` が `range` と同じ形でないときは「`sum_range` の左上セルから始まり、`range` と同じ大きさの範囲」を合計する、とある。例として range `A1:A5`、sum_range `B1:K5` のとき、実際に足されるのは `B1:B5`。`=SUMIF(A1:A10,">0",B1)` は B2:B10 を読むが、トークンは `B1` しか出てこない。B5 を書き換えても、この式は依存先にならない。POI コーパスでは 3 ファイル・7 式が `SUMIF`/`AVERAGEIF` を使っている（`54206.xlsx` など。見た範囲では同じ大きさの範囲だった）。

(b) **範囲演算子 `:` の片側がリテラルでない場合。** `=SUM(A1:INDEX(C1:C10,5))` は A1:C5 の外接矩形を読む。B 列はどのトークンにも現れない。ADR は「`INDEX` used with a reference result」（L90）を `Everything` にするとしているが、「参照結果として使われているか」を構文だけで判定する方法は書かれていない。また、同じことが `CHOOSE`、`IF`、`XLOOKUP`（参照を返す）、定義名どうし（`Name1:Name2`）でも起きる。列挙は不完全。

(c) **UDF とアドイン関数。** L95 は関数名を無視する。VBA UDF は `Range(...)` で任意のセルを読めるが、`myFunc(A1)` の前件は `A1` だけになる。実例として `testNames.xlsm` に `myFunc("_")` がある（引数に参照が無いので、前件は空になり、永久に invalidation されない）。`56502.xlsx`（`[1]!LUCANET(…)`）や `HsGetVal.xlsx` は external 経由で `Everything` になるので救われるが、ブック内の UDF は救われない。

(d) **data table。** ECMA XSD の `CT_CellFormula` は `dt2D`/`dtr`/`r1`/`r2` を属性で持つ（`sml.xsd` L2757-2762）。`f t="dataTable"` は普通テキストを持たない。data table の結果は、表の縁にある式と、その推移的な前件すべてに依存する。§1（L70）は dataTable の result range をモデル化しているのに、§2/§3 の規則では前件が空になり、何を編集しても invalidation されない。コーパスには 0 件なので、仕様にもとづく指摘になる。

(e) **`#SPILL!` で止まった動的配列のアンカー（推論）。** spill 先が塞がれているアンカーの値は「塞いでいるセルが空かどうか」に依存する。§4 で formula → value の上書きや clear によって blocker が消えると、アンカーの `#SPILL!` は stale になる。しかし blocker は前件にも `ref` にも現れない。Excel がこのアンカーをどう保存するか（`ref` が1セルになるか、など）は手元の fixture では確認できなかった。

(f) **相対参照を含む定義名（要検証）。** Excel の名前の中の相対参照は、その名前を使うセルの位置に対して相対に解釈される。openpyxl issue #501 には、XML 上 `Depreciation!$A1` の名前が Excel では `$A7` と表示される報告がある。L87-88 の「a name whose text is itself only references resolves to them」は、相対成分を平行移動しないまま解決するので、別のセルを前件にしてしまう。コーパスで `!` の後に `$` の無い成分を持つ名前を数えたところ 0 件だったので、頻度は低い。

付記: spill 参照 `A1#` はファイル上 `_xlfn.ANCHORARRAY(A1)` と書かれる（rust_xlsxwriter の future-function 表にも載っている）。Open question 2 の allowlist に `ANCHORARRAY`、`XLOOKUP` など参照を返す `_xlfn.` 関数を入れると、(b) と同じ穴が開く。

### なぜ問題か

ADR の存在理由は「stale な値を黙って読ませない」こと（Context L12-16、ADR-0001 R2-M3）にある。(a)〜(f) はどれも、calamine、pandas、openpyxl `data_only` に誤った値を**エラー無しで**返す。偽陽性（必要以上に消す）は、見えるだけで正直な失敗だが、偽陰性はまさに禁止したい失敗である。ADR は「解析できないものは `Everything`」を原則に掲げている（L100-101）。しかし実際の規則は「明示的に危険と列挙したものだけ `Everything`」になっていて、原則と逆の向き（denylist）で書かれている。

### 提案

- 原則を **allowlist の向き**に書き直す。前件の計算に使ってよい関数は「組み込み関数で、参照を返さず、引数の範囲を暗黙に変えないもの」の表に限る。表に無い関数名（UDF、`_xll.`、未知の名前、`_xlfn.`/`_xlws.` の大半）は `Everything` にする。openpyxl の `FORMULAE`（組み込み関数 300 余）が表の出発点として使える。
- `SUMIF`/`AVERAGEIF` は、`sum_range` を「左上セルから `range` と同じ形」に伸ばしたものを前件にする。または `Everything` にする。
- 範囲演算子 `:` の両辺がリテラルのセル参照でない場合は、各辺の前件の外接矩形を前件にする（INDEX/CHOOSE/IF の結果は引数の範囲の内側にあるので、これで健全になる）。または `Everything` にする。
- `t="dataTable"` のアンカーとその result cell は `Everything` にする。
- `cm` アンカーのキャッシュが `#SPILL!` なら `Everything` にする。
- 定義名の中に `$` の無い成分があれば、平行移動の規則が決まるまで `Everything` にする。
- 補助の手がかりとして `f/@ca="1"`（XSD L2763）も `Everything` にする（判断）。Excel は揮発性の式にこれを付ける。
- 参考までに、本レビューのヒューリスティック集計では、式を含む 127 ブックのうち約 20 ブックで、現行の規則でも少なくとも1つの式が `Everything` になる。上の規則を足しても、その割合は大きくは変わらない見込み（推論）。

---

## B2 [Blocker] 受け入れ試験が取りこぼし（stale cache）を検出できない

### 根拠

- §7-1（L183-186）: 「the `v`/`t` of exactly the cells the oracle predicts」。この oracle が何かは定義されていない。実装の依存グラフを使うなら、自分自身との照合になり循環する。
- §7-1 の CR 判定「dependents read empty, others unchanged」（roadmap Phase1 L335-337 も同じ）が確かめているのは、「実装が依存先と判定したセルは空」「判定しなかったセルは元の値」ということだけ。依存先を1つ取りこぼした実装もこの試験に通る。
- fixture の実態（`python3 -I` で確認）:
  - `shared_formulas.xlsx`: 1シート。`A2` に `B2` を参照する shared formula がある。
  - `MatrixFormulaEvalTestData.xlsx`: 1シート。同じ行の配列演算。
  - `70005-countifs.xlsx`: 1シート、4式。
  - `NewStyleConditionalFormattings.xlsx`: 1シート。`$A2` の 2-D shared group。
  - `xlookup.xlsx`: 3シートあるが、Sheet2/Sheet3 の式はすべて `_xlfn.XLOOKUP`/`_xlfn.XMATCH` で、自シートだけを参照している。しかも Open question 2 の「allowlist は空から始める」なら `Everything` になる。
  - したがって、**シートをまたぐ参照、定義名、3-D 参照、列全体の参照、2段以上の推移的な連鎖のどれも試験されない**。
- §7-5 の「every formula cell becomes a dependent」は、`Everything` の経路が働くことだけを確かめている。
- §7-8 の openpyxl 差分試験（L102-104, L202-203）は字句解析の一致を見るだけで、前件の意味（B1 の a〜f）は検証しない。

### なぜ問題か

この ADR が防ぐべき失敗は偽陰性だけである。それを検出する oracle が1つも無いので、B1 のような穴は試験を通ったまま出荷される。ADR-0001 が T1/T1b/T2 を作ったのと同じ理由（自明に通る oracle は oracle ではない。ADR-0001 R2-M1）がここにも当てはまる。

### 提案

- **再計算 oracle を足す。** 各編集の出力を、オフラインで LibreOffice headless の hard recalc（`OOXMLRecalcMode`=always）にかけ、「`v` を残した式セルの値は、再計算後の値と一致する」ことを確かめる。期待値はコミットしておき、試験の実行時に LibreOffice は不要にする。生成手順は、spike と同じ扱いで `spikes/` に置く。
- 編集で書く値は、依存する式の結果が必ず変わるものを選ぶ（同じ値を書き戻すと、取りこぼしが見えない）。
- **小さな合成 fixture を足し、期待する依存集合を手で書く。** 対象は、シートをまたぐ参照（R2-M3 の例そのもの）、ローカル名とグローバル名、3-D 参照、`A:A`、3段の推移、B1 の (a)(b)(c)(d)、それに依存しないことを確かめる式（キャッシュが残ることを確認する用）。
- 「どれか1つの規則を外すと、どれか1つの試験が落ちる」ことを、ミューテーション試験で確認する（判断）。

---

## M1 [Major] `remove_part` は ADR-0001 Layer 3 の不変条件と矛盾するが、改定が書かれていない

### 根拠

- ADR-0001 L337-339: 「**Existing indices are append-only and are never renumbered or compacted.** A registry may add new entries at the end; it may never remove or renumber an existing one, even if it becomes unreferenced.」。L347 は `[Content_Types].xml` と `_rels` を、`ooxml-core::Package` が持つ registry と定めている。
- ADR-0003 L149-159 は、`.rels` から `Relationship` を、`[Content_Types].xml` から `Override` を削除する。L240-242 はこれを「registry 部品が Layer 2 の書き込み範囲に入る」としか書いておらず、不変条件との関係に触れていない。

### なぜ問題か

ADR-0001 の不変条件は「Layer 1 で素通しした部品の中の参照が無効にならない」ことを保証する根拠になっている。今回の削除は実害が無い。`calcChain` の `rId` を参照する部品は無く、番号の振り直しも無いからである。しかし、例外であることを書いておかないと、後の Phase3（シート削除）で同じ API が無条件に使われる。

`remove_part` の仕様にも穴がある。
- 削除する部品自身が `.rels` を持つ場合の扱い。
- source part 以外の `.rels` からその部品を指す relationship の扱い。
- 整形された（改行やインデントの入った）`.rels` や `[Content_Types].xml` で、前後の空白 text ノードをどう扱うか。§7-6 の「byte diff confined to it」の定義に直結する（ADR-0002 R1 B3 の空白の論点と同じ）。

### 提案

- ADR-0001 に post-acceptance note を足す。内容は「削除は、(1) 対象部品も同時に削除され、(2) 削除される entry の id が package のどこからも参照されない場合に限って許す。番号の振り直しはしない」。
- `remove_part` の前提条件（対象部品が `.rels` を持たない。持つなら `Err` か連鎖削除）と、走査の範囲（全 `.rels`）を書く。
- 空白 text ノードの扱いを決め、T2 で許す差分を「要素の span と、その直前の空白 text 1つ」のように定義する。

---

## M2 [Major] §3 の手順と §5 の guard の実行順が規定されていない。途中の `Err` で部分適用の状態が残る

### 根拠

- §3 は 1. 依存先の計算 → 2. `v`/`t` の削除 → 3. `calcPr` の設定や挿入、の順に書かれている（L111-121）。
- 2 の途中で `vm` の依存先が見つかったら refuse する（L116-117）。§5 の `MacroSheetDependent` も、依存先がすべて分かってからでないと判定できない。
- 3 の `calcPr` 挿入は、ADR-0002 §4.3 の Phase0 規則で `Err(UnresolvedSibling)` になりうる（`workbook` が `mc:AlternateContent` の子を持つ場合）。コーパスで数えると、`workbook.xml` が AC を持つのは 78/333。`calcPr` が無い式入りブックは 2 件（`bug60858`, `link-external-workbook-b`）で、どちらも AC を持たないので、いまは到達しない。しかし規則上は到達しうる。

### なぜ問題か

ADR に書かれた順にそのまま実装すると、次のどちらかが起きる。
- 値を書いた → 依存先の `v` を一部消した → `vm` の依存先や `calcPr` の挿入で `Err` になった。
- 値を書いた → 後続の処理が `Err` になった。

呼び出し側がその `Err` を無視して `save` すると、「値は変わったが、依存先のキャッシュが残っている」か「`fullCalcOnLoad` が無い」ファイルができる。これは B1 と同じ種類の、黙った stale である。

### 提案

§3 の冒頭に「計画 → 検証 → 適用」を明記する。
1. 依存集合を計算する。
2. すべての guard（`ArrayResultCell`/`ArrayAnchor`/`TableHeaderCell`/`RichValueCell`/`MacroSheetDependent`）と、`calcPr` を挿入できるかを確かめる。
3. ここまでで1つでも `Err` なら、`Workbook` を一切変更せずに返す。
4. そうでなければ、値の書き込み、依存先の削除、`calcPr` の変更を適用する。

§4 の formula 書き込み、master 昇格、`remove_part` にも同じ規則を適用する。試験に「`Err` の後に `save` しても全エントリが L1」を加える（ADR-0001 R2-m1 と同じ形）。

---

## M3 [Major] shared formula の master 昇格: `ref` の定義、`Err` の条件、範囲外への平行移動、oracle

### 根拠

- L144-148: 「promotes the next member in row-major order inside `ref` … a `ref` covering the remaining group … If the master's text yields `Everything` during translation (unlexable), the write returns `Err(SharedFormulaMaster)`」。
- POI `XSSFSheet.onDeleteFormula`（手元ソース L5006-5029）の新しい `ref` は `(i, lastRow, i == lastRow ? j : ref.firstColumn, lastColumn)` で、昇格した master が左上にない矩形になる。
- POI コーパスで、master を除いた残りが矩形でない group は 72 group / 12 ファイルあった。Excel が書いたファイル（`51710.xlsx`、app.xml は Microsoft Excel）では、master が `ref` の左上に無い（`E60` で `ref="C60:M83"` など 44 group）。master より row-major で前にある member や、`ref` の外にある member は 0 件だった。
- calamine 0.38.0 `expand_shared_formula_with_offset_into`（`src/xlsx/mod.rs` L3522-3570）について。
  - 引用符として扱うのは `"` だけで、`'` は引用符として扱わない。そのため `'Q1 2024'!A1` の `Q1` も平行移動される（字句上の推論。`Q1` は有効なセル参照）。
  - コメント（L3524）に「references that would move out of XLSX bounds are preserved」とあり、範囲外になる参照は元のテキストのまま残る。

### なぜ問題か

1. 「the remaining group を覆う `ref`」が、矩形でない残りに対して何を指すのか定義されていない。外接矩形なら `51710.xlsx` と同じ形（master が左上に無い）になる。その形を Excel が自分で書いている、という根拠は示せる（`51710.xlsx`）が、ADR には書かれていない。
2. `Err` の条件が「`Everything`」と「字句解析できない」を混同している。`INDIRECT` や構造化参照を含む master は、前件は `Everything` でも、平行移動は問題なくできる。書かれたとおりに実装すると、そうした group の master を書き換えるたびに `Err` になる。
3. 平行移動で参照が範囲外に出る場合の規則が無い。旧 master が `ref` の左上に無い場合（`51710`）、新しい master への列のオフセットは負になりうる。calamine の挙動（元のテキストを残す）を真似ると、誤った式を黙って書く。これはキャッシュが stale になるより悪く、式そのものが壊れる。
4. §7-7 の oracle「calamine の展開と一致」は、上のような calamine の字句上の近似をそのまま正解として扱ってしまう。

### 提案

- 新しい `ref` を「残った member の外接矩形」と定義し、`51710.xlsx` を根拠として引用する（POI の式と同値かどうかも書く）。
- `Err(SharedFormulaMaster)` は「字句解析に失敗した」場合と「平行移動の結果が範囲外になる」場合に限る。
- §7-7 の試験に次を加える。2-D の group（`NewStyleConditionalFormattings.xlsx` の `D2:U17`）で昇格した後、`ref` が `D2:U17` で master が `E2` になること。master が左上に無い group（`51710.xlsx` を fixture に追加）。引用符付きのシート名を含む合成ケース。
- oracle は calamine だけにしない。openpyxl の `Translator` とも照合し、両者が食い違うケースは手で期待値を書く。B2 の再計算 oracle も昇格後の出力にかける。

---

## m1 [Minor] result cell の `v` を消すと最後の子が無くなる。その場合は `own_dirty`

L117-119 は「it is a child removal, so only that `c` is `subtree_dirty`」としている。しかし配列の result cell（`<c r="I2"><v>0</v></c>`）から `v` を消すと子が無くなる。ADR-0001 L229-232 の規則（「gains its first child (or loses its last one) is always `own_dirty`」）により、この `c` は `own_dirty` になり、self-closing で出力される。ADR の記述をこの規則に合わせ、T2 で許す差分にも含める。

## m2 [Minor] `t` は値にかかわらず消すべき

L115-116 は `str`/`b`/`e` だけを消すとしている。コーパスには、式セルで `t="n"` のもの（106 セル / 7 ファイル）と `t="inlineStr"` のもの（`52575_main.xlsx`、LibreOffice 4.1、`<f>…</f><v t="inlineStr">…</v>`）がある。式セルの `t` は常にキャッシュされた値の型を表すので、規則は「`v` を消すときは `t` も消す」で足り、そのほうが漏れが無い。

## m3 [Minor] LibreOffice についての「未検証」（L243-245）はソースで解消できる

- LibreOffice/core master `sc/source/filter/oox/formulabuffer.cxx` L254-260: キャッシュ値が空の式セルは `SetDirty()` され、さらに `AddRecalcMode(ScRecalcMode::ONLOAD_ONCE)` が付く（コメント「Recalc even if AutoCalc is disabled」）。既定の「Never recalculate」でも再計算される。
- `workbookfragment.cxx` `recalcFormulaCells()` には、`OOXMLRecalcMode` による分岐と「0.0 以外の結果が1つも無ければ hard recalc」という分岐しかない。`workbooksettings.cxx` には `fullCalcOnLoad` が一度も出てこない。つまり **LibreOffice は `fullCalcOnLoad` を読まない**。

結論として、既定の経路（`v` を消す）は LibreOffice でも正しく、`KeepStaleCaches` は LibreOffice では stale な値を表示する。Consequences の未検証項目を、この根拠つきの記述に置き換える。`KeepStaleCaches` の説明（L129-130）にも「Excel 専用。LibreOffice は `fullCalcOnLoad` を無視する」と書く。手動での確認（XL と同様のもの）は、残すとしても補助の位置づけでよい。

## m4 [Minor] Context の事実関係の修正

- L22-23「11 do not (LibreOffice- and POI-written files)」: `app.xml` で見た内訳は、LibreOffice 4、Apache POI 2、Microsoft Excel 2（`clone_sheet`, `CVLKRA-KYC_…`）、ONLYOFFICE 1、記録なし 2。
- L23「1 a dynamic array」: `formula_stats.py` は `cm` を数えていない（数えているのは `_xlfn` の 1 件）。本レビューで `c/@cm` を数えた結果も 1 件（`xlookup.xlsx` の `C2`）で、数値は正しい。ただし、出典として示しているスクリプトでは再現できないので、`cm` の集計をスクリプトに足す。
- L33-36 openpyxl: `CalcProperties.__init__` の `fullCalcOnLoad=True` が既定値（`openpyxl/workbook/properties.py` L103）。読み込んだ `calcPr` にこの属性が無いときにも既定値が使われるので、openpyxl は保存のたびに実質的に `fullCalcOnLoad` を立てる。ADR の比較表にこの点を足すと、§3 step 3 の根拠が強くなる。
- rust_xlsxwriter の記述（L30-32）と umya の記述（L37）は、ソースで確認したとおり正しい。

## m5 [Minor] `calcMode="manual"` と `fullCalcOnLoad` の綴り

- コーパスでは、式入りブックのうち 2 件が `calcMode="manual"`。rust_xlsxwriter は manual モードでは `fullCalcOnLoad` を書かない（`workbook.rs` L3679-3682）。manual のブックで依存先の `v` を消し、`fullCalcOnLoad` を立てたときに Excel がどう振る舞うかは未検証。手動計算を選んだ利用者の意図を壊す可能性もある。方針を決めて書く（判断）。
- `fullCalcOnLoad="true"` という綴りも 1 件ある。「既に立っているなら変更しない」判定は `1` と `true` の両方を受け付ける。
- `fullCalcOnLoad` が立ったファイルは、Excel で開いて閉じるだけで「変更を保存しますか」と聞かれる（rust_xlsxwriter のファイルで知られている挙動）。Consequences に一言書く。

## m6 [Minor] ADR-0002 §5/§6 の範囲の拡張が書かれていない

ADR-0003 が足す allowlist は `calcPr`（L120）だけである。しかし実際には、次の拡張がすべて必要になる。
- 先頭以外のワークシートへの Layer 2 書き込み（ADR-0002 §5 は「edits are still confined to the worksheet part」で、§6 は先頭シートだけを扱う）。
- `workbook.xml` への書き込み。
- `definedName` の読み取り。
- `metadata.xml` の読み取り（`cm` が XLDAPR の `fDynamic` を指しているかの確認。m8）。
- table 部品の読み取り（`TableHeaderCell`）。
- `.rels` と `[Content_Types].xml` への書き込み。

roadmap の Phase1 scope 1（「Read and edit any worksheet」）も ADR 本文には無い。Decision に allowlist の差分として列挙する。

## m7 [Minor] tokenizer の差分試験の入力が、試験の実行時に手に入らない

roadmap L342 では POI コーパスを「fetched, not committed」としている。一方 L102-104 は「expected token streams … committed」としている。期待値だけをコミットしても、入力の式テキストが無ければ試験は動かない。抽出した式テキストも期待値と一緒にコミットする（Apache-2.0 の帰属表示を付ける）と明記する。

## m8 [Minor] `cm` の解釈

L71-72 は「A cell with `cm` is treated as a dynamic-array anchor」としている。`cm` は `cellMetadata` の汎用の索引である。`xlookup.xlsx` では XLDAPR / `fDynamic="1"` を指していることを確認したが、ほかの種類の metadata が付く可能性もある。規則は「`cm` があり、`f t="array"` で、metadata が XLDAPR を指す場合はアンカー。それ以外の `cm` は `Err`」とするのが安全（判断）。

## m9 [Minor] Alternative 1 の「degrades to」の記述が §3 と合わない

L211-213 は「Kept as the behaviour that §3 degrades to when the analysis says `Everything`」としている。しかし §3 で `Everything` になるのは、その式とその推移的な依存先だけで、ブック全体のキャッシュを消すわけではない。記述を §3 に合わせる。

---

## 推奨する修正の優先順位

1. **B1**: 前件の規則を allowlist の向きに書き直す。(a)〜(f) を規則として明記する。`_xlfn.` の allowlist から参照を返す関数を除く。
2. **B2**: 再計算 oracle（LibreOffice の hard recalc で作った期待値をコミット）と、依存集合を手で書いた合成 fixture（シートまたぎ、名前、3-D、列全体、推移、B1 の各ケース）を受け入れ試験に足す。
3. **M2**: 「計画 → 検証 → 適用」を明記し、`Err` の後に `save` しても L1 になる試験を足す。
4. **M3**: 昇格後の `ref` の定義、`Err` の条件、範囲外の扱いを決める。oracle を calamine だけにしない。2-D の group と `51710` の形を試験に入れる。
5. **M1**: ADR-0001 Layer 3 の例外を note として足し、`remove_part` の前提条件と空白の扱いを決める。
6. Minor（m1〜m9）。特に m3 は、Consequences の未検証項目をそのまま解消できる。

## 判定

**Minor の修正だけで Accepted にできるか: no.**

B1 は、ADR の中心的な主張（stale な値を黙って読ませない）に対する具体的な反例である。B2 は、その種の欠陥を受け入れ試験で検出する手段が無いことを示している。どちらも §2 と §7 の書き直しが必要で、M1〜M3 も設計上の決定を要する。

## 出典

- ECMA-376 Part 1 Transitional XSD `sml.xsd` L2751-2768（`CT_CellFormula`）。
- Microsoft Support, "SUMIF function": <https://support.microsoft.com/en-us/office/sumif-function-169b8c99-c05c-4483-a712-1697a653039b>（`sum_range` のリサイズ）。
- LibreOffice/core master（2026-08-25 時点の最新コミット `38fb197b`）: `sc/source/filter/oox/formulabuffer.cxx`、`workbookfragment.cxx`（`recalcFormulaCells`）、`workbooksettings.cxx`。
- openpyxl issue #501（名前の中の相対参照）: <https://foss.heptapod.net/openpyxl/openpyxl/-/work_items/501>。
- calamine 0.38.0 `src/xlsx/mod.rs` L3476-3570。openpyxl 3.1.5 `workbook/properties.py`、`utils/formulas.py`。rust_xlsxwriter 0.102.0 `src/workbook.rs` L3671-3688。umya-spreadsheet 3.1.1 `src/writer/xlsx/workbook.rs` L156。POI `XSSFSheet.java` L4997-5036、`CalculationChain.java` L87-104。
- コーパス: POI `test-data/spreadsheet` @ `47dcff76`（367 ファイル）。`formula_stats.py` の再実行と、レビュー用の追加集計（`python3 -I`、scratchpad `review-0003/`）。
