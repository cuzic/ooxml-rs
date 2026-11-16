# ADR-0017 レビュー（Opus, round 2）

対象: PR #46 `docs/adr/0017-xlsx-phase5-objects.md` revision 2（HEAD `322756d`）+ `spikes/xlsx-phase5/`。

照合したもの:

- round 1（`0017-opus-review-round1.md`）。
- **ADR-0003 rev 5**（Accepted、main）。
- **ADR-0009 rev 3**（`plan-xlsx-phase3` `47ff161`）。
- **ADR-0014 rev 3**（Accepted、main。PR #35 はマージ済み）。
- PR #38 `RegistryPlan`（`core-add-part` `68a02c1`）と、その round 2 の N1/N2。
- main の `ooxml-core` `lens.rs`（`check_insert_at` は `LensMut` にしかない）。

スパイクは POI `test-data/spreadsheet` @ `47dcff76`（366 ファイル）と calamine の fixture で再実行した。
結果は次のとおりで、rev 2 の数値と一致した。

- rev 1 の規則で衝突するシート: **3**。
- VML の番号: 464（非負）+ 2（負）= **466**。
- `control`/`oleObject` の `shapeId`: **264**。
- 描画と VML が番号を共有するシート: **6 シート / 5 ファイル**。
- ブロックの範囲外にある番号: 0。

## 結論（先に）

round 1 の Blocker 2 件と Major 4 件は、**すべて設計として解消されている**。

- **B1**: すべての拒否を step 1 で判定する構成になった。
  - Phase1 のセル書き込みは `CellWritePlan` でまとめて検証し、census を計画の中で累積する。
  - calcChain の削除と `calcPr` の変更は `RegistryPlan` に入れた。
  - `xml_part_mut` を使う検査は、すべて `RegistryPlan::new` の前に行う。
  - apply の後は、検査済みの in-place 挿入だけが残る。
  - poisoning と fault injection を ADR-0003 rev 5 §3.1 と同じ形で定めた。
  - 操作ごとの表もある。
  - ADR-0003 rev 5 は今も「batch API は不要」と書いている。この点は、ADR-0017 が P5-pre（ADR-0003 の追記）を
    前提として明示し、P5b と P5d の前に置くことで正しく扱っている。
- **B2**: シートごとに 1 つの shape 番号の割り当て器を置いた。
  - 対象は `cNvPr`（全分岐）、VML の `id` と `o:spid`、`control`/`oleObject` の `shapeId`、セッションの high-water。
  - この割り当て器は P5a に入れる。
  - mv-calculator（1133）と 45540（2139）の衝突、`ExcelWithAttachments` の連番は、この規則で正しく扱える。
- **M1–M4、m1–m11** も反映されている。
- **Q1–Q7** はオーナーの決定（2026-11-16）として記録されている。Q1 は `oneCellAnchor` + 自然サイズ、
  Q7 は拒否 + opt-in ヘルパである。

新しく見つかったのは **Minor 8 件** で、Blocker と Major はない。

- 注意が要るのは r2-m1 である。`ColumnInUse` の照合が大文字小文字を区別するか規定がなく、Phase1 の lexer は
  構造化参照を不透明なトークンとして扱う。このため、一致を見逃すと列名の変更が通ってしまう方向（fail-open）に穴が残る。
- r2-m2 は、構造化参照で特殊文字を含む列名をどう書くかの規則が不完全という指摘である。
- どちらも ADR の文言を数行足せば閉じる。

**判定: converged（Major 0）。Accepted after Minor only: yes。**

---

## round 1 の指摘の検証

| 指摘 | 判定 | 確認内容 |
|---|---|---|
| B1 | **解消** | §2.1 の 3 段構成と操作ごとの表を確認した。`CellWritePlan` が全セルの §3.1 ガードを検証し、census を累積する。calcChain は `remove_parts(&[calcChain], &[REL_CALC_CHAIN])` で、ADR-0009 §2 と同じ形である。workbook の `stage_part` と、`xml_part_mut` を使う検査を `RegistryPlan::new` より前に置く規則も確認した。PR #38 の `xml_part_mut` が `touch()` で generation を上げること（`package.rs` L384-392）とも合っている。apply の順序（registry → 要素の挿入 → セル書き込み → census）は ADR-0003 rev 5 の apply 順（i)–(vi) を包含している。`vm`/`cm`/共有マスタについて、apply 前に拒否してバイトが不変であることの回帰試験が試験オラクル 3 にある |
| B2 | **解消** | §2.2 の割り当て器を確認した。値は 1 + max(`cNvPr` の全分岐、VML の `id`/`o:spid`、`shapeId`、high-water) で、解析できない番号は無視するが既存の文字列とは一致させない。この規則を `ExcelWithAttachments`（描画 1025–1031、VML 1032–1038）に当てると、次はどちらも 1039 になる。mv-calculator では 1133 を避ける。オラクル 2 にシート内の一意性の検査、オラクル 3 に明示ケースがある。境界の扱いは r2-m3 |
| M1 | 解消 | §6.4 で、自己参照、`tableType` ≠ `worksheet`、自分の `.rels` を持つ表を拒否する |
| M2 | 解消（細部は r2-m1, r2-m2） | 列指定子を作る関数を 1 つにし、`'` エスケープと縁の空白の二重括弧を扱う。lexer での往復を試験する |
| M3 | 解消 | `comment` 要素に `xmlns:mc`、`xmlns:xr`、`mc:Ignorable="xr"` を局所的に宣言する。`xr` が別の URI に結ばれていれば拒否する。Excel 2010 相当で開けるかを XL 項目にした |
| M4 | 解消 | 空でないセルは拒否し、`overwrite(true)` のときだけ上書きする。`vm`/`cm`/共有マスタは範囲の拒否理由に入った |
| m1 | 解消 | 表の削除だけがシートを stage し、それを最後の段にする（理由の記述は r2-m7） |
| m2 | 解消 | `x15:tableSlicerCache/@tableId` を表 id の走査に含める |
| m3 | 解消 | スキーマのスロットに挿入する。`xda` は `ext` の上で宣言する。calamine の fixture を試験ケースにする |
| m4 | 解消 | DPI、SOF、BMP、EMF、WMF、EXIF、`ext` の上限、fuzz 対象を規定した |
| m5 | ほぼ解消（r2-m6） | allowed-diff 表に削除系の行と CT の行が入った |
| m6 | 解消 | query/xml の表、ピボットやスライサーのソースになっている表、空文字列への変更は拒否する |
| m7 | 解消 | `[@Col]` をトークン単位で変換する。`#`/`@` 演算子を拒否する。Q7 と合わせて ADR-0003 の追記に入れる |
| m8 | 解消 | 拒否条件 5 つと、カテゴリ軸の選択規則を加えた |
| m9 | 解消 | POI の引用を直した。一致する Note shape が複数あれば拒否する |
| m10 | 解消 | GUID の衝突検査を入れた。`dT` を XL 項目にした |
| m11 | 解消 | ext の順序を XL 項目にした |
| Q1–Q7 | 記録済み | 「Open questions (decided)」節と各節に、Owner decision 2026-11-16 として記録されている |

---

## Minor（新規）

### r2-m1. `ColumnInUse` の照合が規定不足で、見逃すと変更が通ってしまう（fail-open）

§6.3 は「lexer トークン上で、列指定子関数のデコードを使って照合する」としている。しかし次の点が欠けている。

- **Phase1 の lexer は構造化参照を 1 つの不透明なトークンとして扱う**（ADR-0003 rev 5 §2.1:
  「Structured references … are one opaque token」）。トークンの中身（表名、`[#This Row]`/`[#Totals]`/`[#All]`/
  `[#Data]`/`[#Headers]`、`,`、列範囲 `[[Q1]:[Q3]]`、`'` エスケープ）を解析する副文法が、どこにも定義されていない。
- **大文字小文字**: Excel の構造化参照で、列名は大文字小文字を区別しない（`T[sales]` は列 `Sales` を指す）。
  §6.3 は列名の一意性については case-insensitive と書いているが、照合については書いていない。
  `Sales` を `Revenue` に変える見出し同期で、`T[sales]` を見逃すと、参照がダングリングしたまま保存される。
- 列挙されていない綴りがある: `T[@Col]`、`[@[Col with space]]`、`T[[#Totals],[Col]]`、列範囲の端点。

**推奨。** 安全側に倒した規則を規定する。

- 対象は、構造化参照トークンのうち、表名が対象の表である（ADR-0003 §2.4 の fold で比較）もの、
  または表名がなく対象の表の範囲内にあるもの。
- その内部で、角括弧に囲まれた要素を `'` エスケープを解いたうえで fold 比較する。
  対象の列名に一致すれば `ColumnInUse` とする。
- 副文法で解析できない構造化参照トークンが 1 つでもあれば、`ColumnInUse` として拒否する。
- 試験に、大文字小文字の違い、列範囲、`[@[…]]` を加える。

### r2-m2. 列指定子の二重括弧の規則が、縁の空白だけでは足りない

Microsoft の説明（"Using structured references with Excel tables"）では、次の特殊文字を含む列見出しは、
列指定子を二重括弧で囲む必要がある: tab、LF、CR、`,`、`:`、`.`、`[`、`]`、`#`、`'`、`"`、`{`、`}`、`$`、
`^`、`&`、`*`、`+`、`=`、`-`、`>`、`<`、`/`（空白も挙げられているが、Excel 自身は `T[Sales Amount]` と単括弧でも書く）。

rev 2 の規則では、`SUBTOTAL(109,T[Q1, est])` や `T[a:b]` のような、Excel が別の意味に読む単括弧の形を生成しうる。
lexer での往復試験は、Excel の解釈までは保証しない。

**推奨。**

- 単一の列指定子（集計の `Table[Col]`）は、列名に上の文字か縁の空白が 1 つでもあれば `Table[[Col]]` にする。
  あるいは常に `Table[[Col]]` にする（常に有効な形）。
- `'` エスケープの対象（`'`、`#`、`[`、`]`）はそのままでよい。
- XL 項目に「`,`、`:`、`$` を含む列名の集計」を加える。

### r2-m3. 割り当て器の境界条件が未規定

- **割り当て器の値が、シートの最後のブロックを超える場合。**
  - §2.2 は「ブロックの範囲内の最初の空き番号まで引き上げる」「ブロックが満杯なら新しいブロックを追加する」
    としている。
  - しかし、`cNvPr` の最大値がブロックの範囲より上にある場合（描画側の番号が大きい場合）、「引き上げる」では
    範囲に入らない。新しいブロック（1 + ブック全体の最大ブロック）の範囲が値より下になることもある。
  - 一意性が要件で、単調性は要件ではないので、次のように規定するのがよい。
    「シートのブロック範囲の中で、どの既存番号とも一致しない最小の番号 ≥ 割り当て器の値。それがなければ、
    範囲内で未使用の最小番号。それもなければ新しいブロック」。
- **XML として読めない VML（`BrNotClosed.xlsx`）を持つシートでの画像や図表の挿入。**
  - ブロックはバイト列から読むと規定されているが、shape 番号については規定がない。
  - スパイクと同じ正規表現で、`_s(\d+)` をバイト列から読むと規定するのがよい。それもできなければ
    `PartNotEditable` にする。
- `control`/`oleObject` の `shapeId` は、ふつう `mc:AlternateContent` の中にある（mv-calculator）。
  スパイクは全要素を走査しているが、ADR の本文には `cNvPr` と同じく「全分岐を含む」と書く。
- `legacyDrawingHF`（コーパスにはない）の VML 番号を、割り当て器に含めるのか拒否するのかを 1 行書く。

### r2-m4. `CellWritePlan` の出力の扱いが、§2.1 の中で食い違っている。ADR-0009 rev 3 との違いも明記されていない

- 「Prerequisite」段落は「The plan returns the strip set, the hand-offs, the `calcPr` change and the calcChain
  decision. **These go into the `RegistryPlan` as staged parts** and removals」と書いている。
- step 3.2 は「the `CellWritePlan`'s cell writes and **cache strips** … in place after apply」と書いている。
- ADR-0009 rev 3（R2-M2）は、キャッシュの除去と `sst/@count` を **stage した clone** に入れる方式である。

どちらの方式も成り立つ。ただし P5-pre の API は両方を満たす必要がある。in-place 用に位置を返すことと、
clone への適用である。

**推奨。**

- 「`calcPr` と calcChain だけが plan に入る。除去、hand-off、sst への追記は、step 1 で検証した位置に対して
  apply の後に in-place で行う（シートを clone する 289 ms を避けるため）」と書き直す。
- ADR-0009 との違いと、その理由を 1 文で書く。
- sst への追記の検査（Phase2 の slot と exact-form reuse）が `CellWritePlan` の検証に含まれることを明記する。
  計画の中で同じ文字列を 2 回追記する場合の重複排除も含める。

### r2-m5. fault injection の範囲

- ADR-0003 rev 5 R4-m6 と ADR-0009 rev 3 §2 は、**panic の注入**（`catch_unwind`）も試験している。
  ADR-0017 は「panic でも poisoned になる」と書いているが、試験の対象は Err だけである。panic の注入を加える。
- 試験の対象が、表、動的配列、見出し同期に限られている。画像、図表、ノートにも、apply の後の in-place 段
  （`<drawing>` と `<legacyDrawing>` の挿入）がある。これらにも 1 点ずつ加える（低コスト）。
- `Package::apply` が `StalePlan` を返す場合、何も変わっていないのに、「apply の前に poisoned にする」規則のため
  ブックが poisoned のまま残る。`StalePlan` は計画の手順のバグとして poisoned にするのか、apply の前に検査して
  ふつうの Err にするのかを決めて書く。

### r2-m6. allowed-diff 表の残り

- 見出し同期の行に `sst`（Phase2 の追記と `count`）がない。
- 表、見出し同期、動的配列の「dependents' caches」は、**他のシートのパート** にも及ぶ。しかし表では
  「Sheet part」列にしか書かれていない。「Other parts」列に「依存先のある他のシート（`v`/`is` の除去だけ）」を加える。
- 表の行の `sst per Phase2` は「Other parts」列に移す。

### r2-m7. 古い参照と、事実と異なる記述

- Depends-on の「ADR-0003 (revision 4 on PR #20)」を「revision 5, Accepted, main」に直す。
  あわせて「P5-pre が §3.1 末尾の "A batch API is therefore not needed" を改訂する」と書く。
- 「ADR-0009 revision 2 (PR #27)」を「revision 3」に直す。
- 「ADR-0014 (PR #35, in revision)」を「Accepted, revision 3, main」に直す。
- §2.1 の Removals 段落「PR #38 clones the plan at each step」: 現在の PR #38 は stage した木を `Arc` で共有している
  （`add.rs` L433、L583「the copy is shallow」）。ADR-0009 rev 3 もそう書いている。stage を最後の段にする順序は
  そのままでよいが、理由の記述を直す。
- §10 の「Phase5 adds to ADR-0009's table-id high-water」: ADR-0009 rev 3 に表 id の high-water はない
  （あるのは `sheetId` と `rId`）。「§2.2 の表 id の high-water（ADR-0017 で導入）を ADR-0009 の表の削除にも適用する」
  と書き直す。

### r2-m8. PR #38 round 2 の N1 への依存

PR #38 の N1（「編集していない clone」の判定が、現在の内容と比べているのに、保存するのは元のエントリである）は、
**dirty でない clone を stage したとき** にだけ効く。

ADR-0017 は「content changes」のときだけ stage するので、通常は効かない。ただし次のケースでは効く:
`fullCalcOnLoad` がすでに立っている `workbook.xml` で `calcPr` が変わらないのに clone を stage すると、
Phase1 や Phase2 がセッション中に `xml_part_mut` で行った編集が落ちる。

**推奨。** 「stage するのは変更を加えた木だけで、木は現在の状態（`xml_part`）から作る」を §2.1 に 1 行足す。
あるいは、N1 の修正が #38 に入ったことを前提条件として書く。N2（`set_relationship_target` と `remove_parts`）は、
ADR-0017 が retarget を使わないので関係しない。

---

## 判定

- converged: **yes**（Blocker 0、Major 0）。
- Accepted after Minor only: **yes**。
- r2-m1–m8 を反映して Accepted（rev 3）にしてよい。r2-m1 と r2-m2 は実装時の破損に直結するので、
  Accepted にする前に必ず本文へ入れる。
- P5b と P5d は、P5-pre（ADR-0003 の追記）が Accepted になるまで着手しない。これは §Delivery に書かれているとおりである。
