# ADR-0003 レビュー（Opus, round 2）

対象: `docs/adr/0003-xlsx-phase1-formula-and-calc-state.md` revision 2（Status: Proposed、main @ `ff86ab1`）

方針: 読み取り専用の敵対的レビュー。round 1（`0003-opus-review-round1.md`）の B1・B2・M1–M3・m1–m9 が実際に解消されたかを判定し、revision 2 が新たに持ち込んだ問題を探した。根拠は次のとおり。

- **ADR-0001 との突き合わせ**: Layer 3 の append-only 不変条件と、post-acceptance note（first visible worksheet、QName による式検出）。
- **ADR-0002 との突き合わせ**: §4.2 の child removal / close-tag の規則、§5/§6。
- **roadmap**: `docs/roadmap/xlsx.md` の Phase1 節。
- **マージ済みの Phase0 実装**: `crates/ooxml-xlsx/`（`workbook.rs`、`worksheet.rs`、`scan.rs`、`error.rs`）と、その下の `ooxml-core`（`package.rs`、`xml/mod.rs`、`lens.rs`）。
- **fixture と spike**: `reference/fixtures/xlsx/README.md`、`spikes/xlsx/`。
- **ソースで確認した参照実装**:
  - LibreOffice/core master `2e885d0b` の `sc/source/core/tool/interpr1.cxx`（`ScInterpreter::ScLookup`）。
  - openpyxl 3.1.5 の `formula/translate.py`、`tokenizer.py`。scratchpad に単体で取り出し、`python3 -I` で実行した。
  - calamine 0.38.0 の `src/xlsx/mod.rs`。
- **実測**:
  - POI コーパスの手元コピー（350 ファイル）を `python3 -I` で追加集計した。
  - scratchpad で rustc 1.99.1 の小さなベンチマークを書き、`ooxml-core::XmlDocument` の `clone` を計測した。

重大度は round 1 と同じ3段階。

- **[Blocker]**: 中心の主張（「stale な値をどの reader にも見せない」）が成り立たない構造的な欠陥。
- **[Major]**: 規則が誤っているか欠けていて、黙った stale 値・試験の素通り・実用にならない性能につながるもの。
- **[Minor]**: 記述の修正、範囲の明記で済むもの。

判断の分かれる指摘には「判断」、ソースや実機で確かめていない推論には「要検証」と書いた。

## 結論（先に）

round 1 の Blocker 2件と Major 3件は、構造としては解消された。

- 依存規則は allowlist の向き（「証明できなければ `Everything`」）に書き直された。round 1 の反例 (a)〜(f) はすべて規則になった。
- 試験には、実装の依存グラフを参照しない oracle が3つ（O1 手書き期待値、O2 摂動、O3 再計算）入った。
- `remove_part` は ADR-0001 の不変条件に対する明示的な例外として書かれた。
- 「計画 → 検証 → 適用」が導入された。
- master の昇格は、外接矩形・`Err` の条件・範囲外の扱いまで定義された。

Minor 9件も実質的に取り込まれている。

ただし、allowlist の**中身と名前解決**には、まだ具体的な穴が4つある。どれも「黙って stale な値が残る」種類で、round 1 B1 と同類である（R2-M1）。

1. **動的配列の spill 領域が広がる場合**、広がった先のセルを読む式が依存先にならない。
2. **`LOOKUP` の result_vector は、引数の範囲の外を読む。** LibreOffice のソースで確認した。ADR の「文書化されたリサイズだけを除外する」という基準では除外できない。
3. **`SUMIF` の `sum_range` が `INDEX(…)` のような計算された参照の場合**、「precise rectangle」の読み方次第で健全でなくなる。
4. **シート修飾された名前の解決規則が無い。** `Defines!NR_To_A1` の形で、POI の `56737.xlsx` に実例がある。大文字小文字の違う・存在しないシート名の扱いも書かれていない。

さらに、3つの oracle はいずれもこれら4つの穴を検出しない（R2-M2）。

- O1 と O2 は、Excel の意味論について著者自身のモデルを使う。
- 実エンジン（LibreOffice）と照合する O3 は、実 fixture にしか適用されない。

このほか、§3.1 の staged clone は、9 MB・20 万セルのシートで編集1回あたり約 320 ms かかる。一括編集では実用にならない（R2-M3、判断）。

修正はどれも、規則の文言、O3 の適用範囲、§3.1 の実装方針という局所的なもので、アーキテクチャの変更は要らない。

**判定: Minor の修正だけで Accepted にできるか → No。** R2-M1 と R2-M2 は規則と試験の修正を要する。分量は数段落なので、round 3 で収束する見込みが高い。

---

## round-1 指摘の解消状況

| ID | 内容 | 判定 | 根拠 / 残り |
| --- | --- | --- | --- |
| B1 | 「参照トークン＝前件」が健全でない | **概ね解消（新しい穴あり）** | §2 L137-141 で allowlist の向きにした。(a)〜(f) は §2.3 L191-214 で規則になった。`_xlfn.` の参照を返す関数と `LET`/`LAMBDA` は除外された（L175-179）。ただし allowlist の選び方と名前解決に、具体的な反例が残る → R2-M1 |
| B2 | 試験が取りこぼしを検出できない | **概ね解消（独立性に限界あり）** | §7.1 L427-465 に O1/O2/O3 とミューテーション検査がある。O2 の評価器は tokenizer とは独立している。しかし意味論のモデルは実装と同じ著者のもので、O3 は実 fixture にしか使われない → R2-M2。O3 の運用の細部 → R2-m6 |
| M1 | `remove_part` と append-only 不変条件 | **解消** | §4.3 L355-388。ADR-0001 に足す note の文面、前提条件、走査範囲、空白の扱いが書かれた。在 memory の `ContentTypes` キャッシュの更新と、§3.1 の原子性との関係が残る → R2-M3 |
| M2 | guard と適用の順序 | **解消（設計上の代償あり）** | §3.1 L222-245 の plan → validate → apply と、`Err` の後の L1 試験（L245、§7.3）。staged clone のコストと、clone で覆えない操作 → R2-M3 |
| M3 | master 昇格の `ref`・`Err`・範囲外・oracle | **解消** | §4.2 L319-353。外接矩形（`51710.xlsx` を根拠に引用）、`Err` は字句解析の失敗と範囲外だけ、`INDIRECT` 等はそのままコピー、oracle は openpyxl `Translator` と calamine と手書きの3つ。step 4 の「平行移動は合成できる」という主張は、後述のとおり正しい。「next remaining member」の曖昧さ → R2-m3 |
| m1 | `v` を失った result cell の dirty | **解消** | §3.3 L270-273。ADR-0002 §4.2（L591-598: 元々 close tag を持つ要素は、最後の子を失っても close tag を保つ）と整合する |
| m2 | `t` は値によらず消す | **解消** | L265-267 |
| m3 | LibreOffice は `fullCalcOnLoad` を読まない | **解消** | Context L84-88、`KeepStaleCaches` L296-299 |
| m4 | Context の事実 | **解消** | L50-52（producer の内訳）、L72-73（openpyxl）。`formula_stats.py` L73-79 が `cm`/`ca` を数えるようになった |
| m5 | manual モード、`true` の綴り、保存の確認 | **解消** | L279-283、L293-294。Open question 4 に残っているのは XL での確認だけ |
| m6 | ADR-0002 §5/§6 の範囲拡張 | **概ね解消** | §6 L405-420。ただし「Table-part reads already exist」（L420）は、Phase0 では first visible worksheet の table しか読んでいない（`workbook.rs` `from_package` → `load_tables(&sheet_part …)`）。Phase1 では全シートの table 部品の読み取りが要る → R2-m2 |
| m7 | tokenizer の入力がコミットされない | **解消（roadmap は未更新）** | L150-156。roadmap Phase1 は今も「fetched, not committed」のまま → R2-m5 |
| m8 | `cm` の解釈 | **解消** | L121-128 |
| m9 | Alternative 1 の記述 | **解消** | L526-527 |

---

## R2-M1 [Major] allowlist と名前解決に残る4つの健全性の穴

§2 の原則（L137-141）は正しい。しかし、健全かどうかは「allowlist に載ったものの意味が本当にテキストで尽きているか」にかかっている。以下の4つは、その前提が崩れる具体例である。

### (1) 動的配列の spill 領域が広がる場合

**根拠**: §3.2 step 3（L255-257）は「a dependent array, data-table or spill anchor makes all of its result cells dependents and seeds」としている。result cell は、§1 L119-125 のとおり**現在の** `ref` の中のセルである。

**反例**:

- `D1 = _xlfn.SEQUENCE(A1)`（`A1 = 5`、spill は `D1:D5`、`ref="D1:D5"`）。
- `E1 = D6*2`（キャッシュは `0`）。
- `A1` を `10` に書き換える。

`D1` は依存先になり、`D1:D5` が seed になる。しかし `D6` は seed にならないので、`E1` はキャッシュ `0` を保ったまま残る。Excel が再計算すると `D6 = 6`、`E1 = 12` になる。calamine と LibreOffice は `0` を返し続ける（LibreOffice は `v` のあるセルを再計算しない。Context L85-88）。

- これは allowlist とは無関係で、`D1` の前件が `Everything` でも同じことが起きる。問題は前件の側ではなく、出力領域が広がる側にある。
- §2.3 (e) の、塞がれた spill のアンカーも同じ構造をしている。`Everything` にはなるが、塞がれていた領域を読む式は seed にならない。

**提案**:

- 依存先になった spill アンカー（XLDAPR `fDynamic`）の seed を、「アンカーから右下方向の、アンカーが spill しうる最大の矩形」とする。最も単純には、アンカーの行以降・列以降の全体。
- または「spill アンカーが依存先になったら、同じシートでアンカーより右下を読む式をすべて依存先にする」。
- どちらも O1 に1ケース足す。

(e) の判定は「`cm` アンカーで、キャッシュが `#SPILL!`」とされている（L206-208）。Excel が塞がれたアンカーにも必ず `cm` を付けるかは手元で確かめられなかった（要検証）。`cm` の有無を問わず「式セルのキャッシュが `#SPILL!` なら」とするほうが安全である（判断）。

### (2) `LOOKUP` の result_vector は、引数の範囲の外を読む

**根拠**:

- L172-174: allowlist の種は「openpyxl の `FORMULAE` から、参照を返す関数・引数の外を読む関数・揮発性の関数を除いたもの」。
- L196-197: 「Any other function **documented** to resize an argument is kept off the allowlist」。
- Microsoft のドキュメントは、LOOKUP の result_vector は「lookup_vector と同じサイズでなければならない」としている。リサイズは**文書化されていない**。
- 一方、LibreOffice/core master `2e885d0b` の `sc/source/core/tool/interpr1.cxx` `ScInterpreter::ScLookup` では、結果の位置を求める処理が result_vector の終端を見ない。
  - 結果のアドレスを `nResRow1 + nDelta`（縦）または `nResCol1 + nDelta`（横）で計算し、シートの上限だけを確かめている。
  - 単一セル（`svSingleRef`）の result_vector も受け付け、`nResRow2 - nResRow1 == 0` なので横方向に伸ばす。
- LibreOffice のこの関数には「undocumented Excel behaviour」に合わせたというコメントが他に2箇所あり、Excel 互換を狙った実装と読める。ただし Excel 自体の挙動は手元で確かめていない（要検証。§7.4 XL に1件足せば確かめられる）。

**反例**: `=LOOKUP(5, A1:A10, B1)`。前件は `{A1:A10, B1}` で precise になる。実際には `C1`、`D1`… のどれかを読む（LibreOffice では横方向）。その読む先を書き換えても、キャッシュが残る。

**なぜ問題か**: 除外の基準が「文書化されたリサイズ」なので、文書化されていない互換挙動は種の段階で allowlist に入ってしまう。O2 の評価器も、著者が知らない挙動は実装しないので検出できない（R2-M2）。

**提案**:

- `LOOKUP`（3引数形）を allowlist から外すか、SUMIF と同じく「result_vector の先頭から lookup_vector の長さ」を前件に足す規則を書く。後者の向きは LibreOffice ソースと XL で確定させる。
- 除外の基準を「文書化された」から「**引数として渡された参照の矩形の外を読むことがない、と根拠（ドキュメント、LibreOffice ソース、または XL）で示せる**」に変える。「証明できなければ外す」という、§2 の原則と同じ向きにする。
- POI コーパスでは `LOOKUP` は 1 ファイル（`FormulaEvalTestData_Copy.xlsx`、2式、どちらも同じサイズ）しか使っていないので、外したときの精度の損失は小さい。

### (3) `SUMIF`/`AVERAGEIF` の `sum_range` が計算された参照の場合

**根拠**: (a) の規則は「If `range` or `sum_range` is not a precise rectangle, the result is `Everything`」（L195）。一方、§2.2 の allowlist では `INDEX`/`CHOOSE`/`IF` が「value position」で precise とされている（L185-187）。

**反例**: `=SUMIF(A1:A10, ">0", INDEX(B1:B10, 5))`。

- `INDEX(B1:B10, 5)` の前件は `B1:B10` で、これは「precise な矩形」と読める。
- この読み方では、`sum_range` の左上 `B1` から `range` の形（10行）に伸ばした `B1:B10` が前件になる。
- 実際の `sum_range` は `B5` で、そこから伸びた `B5:B14` を読む。`B11:B14` を取りこぼす。

**提案**: `sum_range` と `range` は**リテラルの参照トークン**（または、名前を展開した結果がリテラルの矩形になる定義名）に限る、と書く。参照を返しうる関数（`INDEX`/`CHOOSE`/`IF`/`IFS`/`SWITCH`）が、位置に依存する引数（SUMIF の `sum_range`、(2) を採るなら LOOKUP の result_vector）に入ったら `Everything` にする。L185-187 の「Their reference results are handled by the `:` rule below」は `:` しか扱っていないので、この一般規則を併記する。

### (4) 名前とシート名の解決規則が無い

§2.2 の名前の行（L165）が書いているのは「Local names (`localSheetId`) shadow global ones on their sheet」だけである。これでは足りない。

- **シート修飾された名前。** POI `56737.xlsx` に、`Defines!NR_To_A1`（`NR_To_A1` は `localSheetId="1"` のローカル名）と `[0]!NR_Global_B2` が実在する。前者を「使う側のシートのスコープ → 無ければグローバル」で解決する実装は、同名のグローバル名があればそれを選び、前件を誤る（精密だが誤った集合になる）。
  - 規則としては次のようにする。`Sheet!Name` は、そのシートのローカル名に解決する。無ければ `Everything`。Excel がグローバル名にフォールバックするかは要検証なので、安全側に倒す。
  - `[0]!Name` は自ブックを指す。L183-184 で `[n]` は `Everything` になるので健全だが、`[0]` が自ブックであることは書いておく（精度の改善余地として）。
- **シート名の照合。**
  - Excel のシート名の比較は大文字小文字を区別しない。Excel 自身は正規の綴りで保存するが、openpyxl などの利用者が書いた式は綴りが揃っているとは限らない。
  - 区別して照合する実装では、`sheet1!A1` が「存在しないシート」になる。それを「前件なし（`#REF!`）」と扱うと取りこぼす。
  - 「解決できないシート名、3-D 参照の端点 → `Everything`」と「照合は Excel と同じく大文字小文字を区別しない」を明記する。
  - 3-D 参照の端点が workbook 順で逆の場合（`Sheet3:Sheet1!A1`）の扱いも同じ節に書く。
- **名前とセル参照の衝突。** `TAX2024` は有効な A1 参照である（列 `TAX` = 13,570 ≤ 16,384）。
  - Excel 2007 以降はこの名前を作らせない。しかし旧形式からの変換や、第三者の writer が書いたファイルには現れうる。
  - openpyxl の `Translator` は `TAX2024` を参照として平行移動する（`=TAX2024+A1` を B2→C3 に移すと `=TAY2025+B2`。scratchpad で実行して確認した）。
  - 「A1 参照として妥当なトークンが定義名とも一致したら `Everything`、平行移動は `Err`」と書く（判断）。
- **値を名前に代入する式。** `f/@bx="1"`（ECMA §18.3.1.40: 名前に値を代入する式。主に XLM）がある式セルは、その名前を使う式の前件になる。`MacroSheetDependent` は「依存先がマクロシートにある」場合しか扱わないので、「`bx="1"` があれば、その名前を使う式は `Everything`」を足す。コーパスでは 0 件。

### なぜ Major か

4つとも、ADR の存在理由である「黙った stale 値」を作る。ただしアーキテクチャの問題ではなく、規則の文言で閉じられる。round 1 の B1（規則の向きが逆）とは性質が違うので Major とした。

---

## R2-M2 [Major] 3つの oracle は、R2-M1 のような「著者の意味論の誤り」を検出しない

### 根拠

- **O1**（L428-439）は、期待値を手で書く。手で書くのは規則を書いたのと同じ著者なので、Excel の意味論を誤解していれば期待値も同じように誤る。
- **O2**（L440-451）の評価器は tokenizer から独立しており、これは round 1 B2 の要求を満たす。しかし評価器が実装する `SUMIF` のリサイズ、`INDEX`、3-D 参照のシート順は、§2.3 の規則と同じ著者の理解である。評価器と規則が同じ誤解を共有していれば、O2 は通る。さらに、生成の対象（L441-444）に次のものが無い。
  - 動的配列
  - `LOOKUP`
  - ローカル名とシート修飾名
  - 大文字小文字の違うシート名
  - 計算された `sum_range`
- **O3**（L452-460）は唯一、実エンジンと照合する oracle である。しかし対象は「each acceptance edit on a real fixture」に限られる。
  - round 1 B2 で確かめたとおり、実 fixture にはシートをまたぐ精密な依存、定義名、3-D 参照、推移的な連鎖が無い。
  - `xlookup.xlsx` の式はすべて `_xlfn.XLOOKUP`/`XMATCH` で、`Everything` になる。
- **ミューテーション検査**（L461-463）は「ある規則を外すと試験が落ちる」ことを確かめる。「規則が足りない」ことは原理的に検出できない。

したがって、R2-M1 の4つの穴は、O1・O2・O3・ミューテーションのどれにも引っかからない。

### 提案

- **O3 を合成 workbook にも適用する。**
  - O1 の合成 workbook すべてと、O2 が生成した workbook の固定シードの標本（たとえば 200 個）を、LibreOffice の hard recalc にかけて期待値をコミットする。
  - 検査は O3 と同じ「`v` を残した式セルは再計算後の値と一致する」でよい。
  - これで、著者の意味論が誤っていれば実エンジンとの不一致として現れる。R2-M1 (2) の `LOOKUP` は LibreOffice が Excel 互換で実装しているので、この経路で検出できる。
- **O2 の生成対象を広げる。** 範囲を伸ばす関数（`SUMIF`、`LOOKUP` を残すならそれも）、名前（ローカル、グローバル、シート修飾）、シート名の綴りの揺れ（大文字小文字、不要な引用符、`''` を含む名前）を加える。生成した式は `$` の有無や空白をランダムに変えて描画する。
- O1 の必須ケース（L432-439）に、R2-M1 の (1)〜(4) と `56737.xlsx` の形を足す。

---

## R2-M3 [Major・判断] §3.1 の staged clone: 1編集ごとにシート全体を複製するコストと、clone で覆えない操作

### 根拠

- L239-243: 「Changes are made on staged clones of the touched `XmlDocument`s … The cost is one clone per touched part per call. Phase6 (large sheets) may replace this」。
- `ooxml-core` の `XmlDocument` は `#[derive(Clone)] struct XmlDocument { source: Box<str>, root: RawNode }`（`xml/mod.rs` L285-289）。`clone` は、ソース文字列とノード木の全体を深く複製する。
- **計測**（scratchpad `review-0003r2/bench`、rustc 1.99.1、`--release`）: 20,000 行 × 10 列、全セルが式のシート（9.1 MB）で、`parse` 368 ms、`clone` は平均 **320 ms**。
- Phase1 の主な用途は、式を含む workbook への値の書き込みである。そこでは**すべての** `set_number` が、少なくとも編集されたシートを clone する（L224-225: 「Every editing call」）。1,000 セルを書くループは、clone だけで約 5 分かかる。Phase6 まで待てる規模ではない。
- clone で覆えない操作がある。
  - `remove_part`（§4.3）は `XmlDocument` の変更ではない。`Package` の `parts` から要素を外し、在 memory の `content_types` と `package_rels`（`package.rs` L70-76）を変える操作である。
  - §1 の `FormulaIndex` と依存グラフのキャッシュ（L132、L259）も `Workbook` の状態だが、staging の対象として書かれていない。
  - どちらも、途中で失敗したときに「何も変わっていない」を保証する手順が無い。

### 提案（判断）

- **validate で apply が失敗しないことを証明し、apply は直接その場で行う**ことを基本にする。
  - validate で、依存先のセルを Layer 2 で特定でき、`calcPr` を挿入でき、`remove_part` の前提条件を満たすことを確かめる。そうすれば、apply で残る失敗は不変条件違反（バグ）だけになる。
  - clone は `workbook.xml`（小さい）に限るか、debug ビルドの検査用にする。
  - 代わりに、変更ごとの undo log（「`v` を外した位置と元のノード」）を持つ方法もある。
- apply の順序を書く。(i) fallible な木の変更 → (ii) `remove_part`（それ自体が all-or-nothing で、在 memory の `ContentTypes` と `Relationships` を同時に更新する） → (iii) `FormulaIndex` の更新（成功後に1回）。
- §7.3 の「apply 段階に失敗を注入する」試験は、`remove_part` の途中と index 更新の手前にも注入点を置く。

---

## R2-m1 [Minor] census が失敗する場合と、依存先を Layer 2 で特定できない場合が未定義

- Phase0 には `Error::FormulaScanFailed`（UTF-16、整形式でない、DTD）がある。§1 はスキャンの失敗に触れていない。式の位置が分からなければ `v` を消せず、`Everything` でも救えない。Phase0 と同じく「value edit は workbook 全体で `Err`」を維持すると書く。
- `r` の無い `row`/`c`（`59746_NoRowNums.xlsx`）が、編集対象以外のシートにある場合も同じ。§1 の census はセル参照で式を索引するので、位置で暗に決まるセルの式は索引できない。そのシートに式があれば、`Err(MissingReference)` を workbook 全体に適用する。
- §3.1 の validate（L231-237）に、次の項目を足す。
  - 依存先が載っているシートをすべて Layer 2 で開ける（Strict、`MissingReference`、`OutOfOrder` にならない）。
  - census（quick-xml のイベント）で見つけた依存先のセルが、Layer 2 の `locate` で同じセルとして見つかる。
  
  二つの解析器が、重複した `r` などで食い違う場合に備える。

## R2-m2 [Minor] Phase0 の API の形は、複数部品にまたがる編集を表せない

- Phase0 の `Worksheet<'wb>` が持っているのは `&'wb mut XmlDocument`（そのシートだけ）と `&'wb SheetContext` だけである（`worksheet.rs` L133）。`CellMut::set_number(self)` からは、他のシート、`workbook.xml`、`Package::remove_part` に届かない。`SheetContext` は「Phase0 edits never change any of it」（`workbook.rs` L63-64）という前提で作られている。
- Phase1 では、`Worksheet` が `&mut Workbook`（または、package と index を分けて借用する構造）を持つ形に変わる。§6 に、公開 API の変化として書く。対象は、`Worksheet`/`CellMut` の借用の形と、`edit_refusal()` の意味の変化（`FormulaPresent` が消える）。
- `worksheet_by_index`（L411）の index は `<sheets>` の順で、chartsheet、macro sheet、dialog sheet も数に入る。それらを指したときの `Err` を決める。
- Table 部品は、Phase0 では first visible worksheet の分しか読んでいない（R2-m6 の表を参照）。Phase1 は全シートを編集するので、`TableHeaderCell` の判定に要る table 部品の読み取りを全シートに広げると §6 に書く。
- `UnknownCellMetadata` は「value edits」にだけ適用される（L127-128、L396）。formula の書き込みと削除にも適用するかどうかを明記する。

## R2-m3 [Minor] master 昇格の細部

- L340 の「the **next** remaining member in row-major order」は、旧 master より前にも member があるときに曖昧になる。round 1 の集計では 0 件だが、規則としては「残った member のうち row-major で**最初の**もの」と書く。
- step 4 の主張（L347-350）は正しい。理由は次のとおり。
  - 新 master N の平行移動 T(N−O) は、N 自身の元の意味そのものである。
  - そのうえで、各 member M について、成分ごとに c+(N−O) と c+(M−O) がどちらも範囲内なら、T(M−N)∘T(N−O) = T(M−O) になる。
  - よって中間で範囲外になるのは、N 自身の元の式が範囲外だった場合に限られ、step 2 の `Err` と一致する。
  
  ADR にこの一行の理由を添えると、試験の意図が明確になる。
- 引用符付きのシート名を「1単位として飛ばす」（L324）は、`''` のエスケープ（`'It''s'!A1`）と、`!` を含むシート名（`'a!b'!A1`）を扱う必要がある。openpyxl の `Translator` は `'It''s'!A1` を正しく扱った（`='It''s'!B2+C2`）。§7.2 item 7 の合成ケースにこの2つを足す。
- 関数名がセル参照の形をしている場合（`LOG10(` は列 `LOG` = 8,509 で有効な参照になる）は、`(` が続くことで関数として扱う。openpyxl（`FUNC`）も calamine（`next_is_paren`）もそうしている。合成ケースに足す。

## R2-m4 [Minor・判断] 反復計算と `autoNoTable`

- `calcPr iterate="1"` は POI コーパスに 2 件ある（`47889.xlsx`、`57535.xlsx`）。反復計算では、循環参照の結果が**キャッシュされた値を出発点にして**決まる。依存先の `v` を消すと出発点が 0 に戻り、Excel が再計算した値は「Excel で同じ編集をしたときの値」と変わりうる。たとえば、`A1 = A1 + B1` を `iterateCount=1` で使うカウンターでは、編集のたびに累積した値が失われる。
  - 黙った値の変化なので、「`iterate` が真で、依存集合が循環を含むなら `Err`」とするか、制約として Consequences に書く（判断）。
- `calcMode="autoNoTable"`（データテーブル以外は自動計算）の workbook で、data table の result cell の `v` を消した場合に、Excel が `fullCalcOnLoad` で data table も再計算するかは確かめられていない（要検証）。manual モードと同じく §7.4 の XL 項目にする。

## R2-m5 [Minor] roadmap Phase1 節が revision 2 と食い違っている

`docs/roadmap/xlsx.md` の Phase1 節は、revision 1 の内容のままである。

- 「shared-formula master promotion (as POI)」。§4.2 は、残りが矩形でない場合に POI と違う結果になる、と明記している。
- 「every `f` text is tokenized … (fetched, not committed)」。L150-156 ではコミットすることになった。
- 「calamine's shared formula expansion agrees before/after」。M3 の解消方針で、calamine を唯一の oracle にしないことになった。
- Oracles 欄に O1/O2/O3 が無い。

ADR を Accepted にするときに、roadmap の該当節を ADR §7 に合わせる。

## R2-m6 [Minor] O3 と O1 の運用の細部

- **揮発性の関数。** §2.3（L212-214）は `NOW`/`RAND` などを、前件なしで allowlist に載せている。そのため、それらのセルは `v` を残す。LibreOffice の hard recalc は、それらに新しい値を入れるので、O3 は必ず不一致になる。揮発性の関数を含む式セル（と、その推移的な依存先）を O3 の比較から自動的に除外する、と書く。いまの 5 fixture には揮発性の関数は無いが、§7.2 item 5 で足す `INDIRECT` の fixture や合成 workbook には現れる。
- **ベースライン。** 入力ファイルのキャッシュが、もともと LibreOffice の再計算と一致していない場合がある。原因は、LibreOffice と Excel の差、POI が書いた未計算の値、manual モードなど。そうしたセルは、編集と無関係に O3 で落ちる。編集前の入力も同じ手順で再計算する。そのうえで、「入力では一致していたのに、出力で `v` を残したセルが一致しない」セルだけを失敗とする差分の形にする。これで「LibreOffice と Excel の差の一覧」（L457）も小さく保てる。
- **再現性。** LibreOffice のバージョン（と `OOXMLRecalcMode` の設定方法）を、期待値と一緒に記録する。値は、再保存した xlsx の `v` をセル単位で読み出して取り出す（shared formula の形が変わっても比較できるように）。
- **ライセンス（判断）。** 期待値は、Apache-2.0 の POI fixture から計算した事実のデータである。LibreOffice（MPL-2.0）の出力には MPL は及ばないと考える。fixture と同じ `NOTICE` の下に置き、生成元のファイルとコミットを記録すれば足りる。
- **「依存先の結果を必ず変える値を書く」（L464-465）について。** 依存先の集合が分からない（取りこぼしがあるかもしれない）状態で、それを保証する手段は無い（`=IF(A1>100,1,0)` は 5→6 では変わらない）。各編集で、桁の違う値を2〜3個書き、それぞれについて O3 を実行する、に言い換える。
- **O1 の生成器。** O1 の「committed generator script」（L429）が何で workbook を書くのかが書かれていない。O1 は「キャッシュされた `v` がバイト単位で残る」ことを確かめるので、生成器はキャッシュ値を書けなければならない。openpyxl は式のキャッシュ値を書かず、rust_xlsxwriter は書ける。生成済みのファイルをコミットし、試験時に Python を走らせないことも明記する。

## R2-m7 [Minor・判断] 「どの reader にも stale な値を見せない」の範囲

Consequences（L554-555）は「no reader ever sees a stale value」としている。しかし、Phase1 が扱うのはセルの式のキャッシュだけで、ほかのキャッシュは対象外である。

- **chart の `c:numCache`/`c:strCache`。** fixture `57362.xlsx` がちょうどこの例で、README にも「goes stale when they are edited」とある。
- **pivot cache。** `pivotCacheRecords` と、シート上の pivot の出力セルは、どちらもただの値である。Excel も、`refreshOnLoad` が無ければ自動では更新しない。`GETPIVOTDATA` は `Everything` になって `v` を失うが、Excel は再計算でも古い pivot cache から同じ値を出す。
- **`externalLink` の `sheetDataSet`**（自ブックへのリンクの場合）。

これらは Phase0 の頃からある制約で、Phase1 が悪化させるものではない。しかし主張の範囲を「cell formula の cached value について」と明記し、ほかのキャッシュは roadmap §4 の該当箇所を参照する、と書くのが正確である。

---

## 推奨する修正の優先順位

1. **R2-M1**: spill アンカーの seed を「spill しうる領域」に広げる。`LOOKUP` を除外するか規則を書き、allowlist の除外基準を「根拠で示せる」に変える。SUMIF の `sum_range` をリテラルに限る。名前とシートの解決規則（シート修飾、大文字小文字、解決できない場合は `Everything`、名前と参照の衝突、`bx`）を書く。
2. **R2-M2**: O3 を O1 と O2 の合成 workbook にも適用する。O2 の生成対象を広げ、O1 のケースを足す。
3. **R2-M3**: apply は validate で証明したうえでその場で行う（または undo log）。`remove_part` と index 更新の順序と原子性を書く。
4. Minor（R2-m1〜m7）。特に R2-m1（census の失敗）と R2-m6（O3 のベースラインと揮発性）は、実装を始めてすぐに当たる。

## 判定

**Minor の修正だけで Accepted にできるか: no.**

round 1 の Blocker は構造としては解消されており、方向性と設計の骨格はこのままでよい。しかし R2-M1 は、ADR の中心的な主張に対する具体的な反例（spill の拡大、`LOOKUP`、計算された `sum_range`、シート修飾名）である。R2-M2 は、それらを試験で検出する経路が無いことを示している。どちらも規則と試験の文言の修正で済むので、round 3 で収束する見込みが高い。R2-M3 は判断の項目だが、Phase1 の主な用途での性能に直結するので、Accepted の前に方針を決めておくことを勧める。

## 出典

- ECMA-376 Part 1, 5th ed.: §18.3.1.40 `f`（`bx`、`ca`、`t="dataTable"`）、§18.17 formulas。
- Microsoft Support, "LOOKUP function"（result_vector は lookup_vector と同じサイズ）: <https://support.microsoft.com/en-us/office/lookup-function-446d94af-663b-451d-8251-369d5e3864cb>。"SUMIF function": <https://support.microsoft.com/en-us/office/sumif-function-169b8c99-c05c-4483-a712-1697a653039b>。
- LibreOffice/core master `2e885d0bda9bc4ee9f3bde51905c4a7820b01f06`: `sc/source/core/tool/interpr1.cxx` `ScInterpreter::ScLookup`（result_vector を `svSingleRef` で受け付ける。結果のアドレスは `nResRow1 + nDelta` / `nResCol1 + nDelta` で、シートの上限だけを確かめる）。<https://github.com/LibreOffice/core/blob/master/sc/source/core/tool/interpr1.cxx>
- openpyxl 3.1.5: `openpyxl/formula/translate.py`、`openpyxl/formula/tokenizer.py`（scratchpad `review-0003r2/tr.py` で `LOG10`、`TAX2024`、`'It''s'!A1`、3-D、`A:A` を平行移動した）。
- calamine 0.38.0: `src/xlsx/mod.rs` L3476-3570（`next_is_paren`、範囲外はそのまま）。
- ooxml-rs main `ff86ab1`:
  - `crates/ooxml-core/src/xml/mod.rs` L284-289（`XmlDocument` は `Clone` で、`source: Box<str>` を持つ）。
  - `crates/ooxml-core/src/package.rs` L36-76、L156-181（状態と `read_part`）。
  - `crates/ooxml-xlsx/src/workbook.rs`（`SheetContext`、`formula_census`、`load_tables`）。
  - `crates/ooxml-xlsx/src/worksheet.rs` L133、L235-258（`preflight`）。
  - `crates/ooxml-xlsx/src/error.rs`（`FormulaScanFailed`、`MissingReference`）。
- コーパス: POI `test-data/spreadsheet` @ `47dcff76` の手元コピー（350 ファイル）。`python3 -I` の `review-0003r2/lk.py` で集計した。
  - `iterate="1"`: 2 件（`47889`、`57535`）。
  - `LOOKUP`: 1 ファイル。
  - シート修飾名: `56737.xlsx`。
- ベンチマーク: scratchpad `review-0003r2/bench`。20 万セル・9.1 MB のシートで、`XmlDocument::parse` 368 ms、`clone` 平均 320 ms（20 回）。
