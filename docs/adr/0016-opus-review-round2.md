# ADR-0016 レビュー（Opus, round 2）

対象: PR #37 `docs/adr/0016-pptx-phase3-formatting-and-links.md`（Proposed, revision 2, commit `37ee24e`）。

照合先:

- round 1 の指摘（`0016-opus-review-round1.md`）。
- main に merge 済みの ADR-0012 rev 3（Accepted、`848b057`）。
- PR #38 `core/add-part`（HEAD `bdc133d`、`package/add.rs`）。
- PR #18 `core/detach-insert`（HEAD `cca838e`、`detach_as` と `dml.toml` の `[detach]`）。
- PR #44 `core/move-subtree`。

独立プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-44-37r2/scripts/r2.py`
（`python3 -I`、zip/XML を読むだけ）。

- コーパス: round 1 と同じ集合を SHA-256 で重複除去した 606 ファイル。
- ECMA-376 Part 1 の実物: scratchpad の `ecma1/`（`p1.zip`、`geo/`）。

## 結論（先に）

M1 は解決した。

- §1 原則 6、§7〜§10 は、ADR-0012 §2 の transaction の形（clone → `RegistryPlan` → `apply`）で書かれている。
- ヘッダの依存は PR #38 になった。
- `PartPlan`、`commit`、C3〜C5、Q1 への参照は残っていない。
- #38 の実際の API（`stage_part`、`add_part(PartNameRequest::Numbered)`、`add_relationship`、
  `remove_relationship`。`remove_relationship` は staged の内容に対する any-attribute 判定）とも一致する。

M2 は大部分が解決した。

- `RunFormat` を値で受け取り、分割前に検証する。
- 値が変わらなければ分割しない。
- 隣の run を結合しない。
- `smtId` は拒否する。

ただし、core に求める「same-part copy mode」の要件が不正確で、round 1 が名指しした `ahyp:hlinkClr` の
run をまだ救えない（N1）。コーパスの 14/14 件の `ahyp:hlinkClr` は、`xmlns:ahyp` を**自分の要素に**
宣言している。#18 の `detach_as` は名前空間宣言も拒否するので、rev 2 の要件（`r:` id と `a:extLst` を
保つ、ほかの `DetachRefusal` は維持）どおりに作ると、これらの run は引き続き `FormatNotCopyable` になる。

m1〜m14 は、いずれも反映を確認した（下の表）。新しい Minor が 6 件ある。

- `extLst` を保つ範囲。
- 一意属性の明示。
- 事前検証の範囲。
- `notesSz` と 4:3 variant。
- provenance の記録。
- `tableStyles` の検出と手順の順序。

## 判定

**converged: no / Accepted after Minor only: no.**

- 残る Major は N1 の 1 件だけで、§3 の 1 段落を書き直せば足りる。
- rev 3 で N1 と Minor を反映すれば、round 3 で収束すると見込む。

---

## round 1 指摘の確認

| 指摘 | rev 2 | 判定 |
|---|---|---|
| M1 transaction model | §1-6、§7〜§10、ヘッダ。Q1 は削除 | 解決 |
| M2 run split | §3。`RunFormat` を値で受け取る、分割前の検証、変わらなければ分割しない、`smtId` は拒否、mode ができるまでは `FormatNotCopyable` | **一部**（N1） |
| m1 ノートマスターの検出 | §8-1。関係で判定、リストを補う、`NotesWithoutMaster` | 解決 |
| m2 4:3 | §8-2。4:3 variant と、それ以外の比率の計算 | 解決。ただし N5 |
| m3 `app.xml` `Notes` | §8-4 | 解決 |
| m4 API の一本化 | §8 の冒頭 | 解決 |
| m5 graphicFrame とコネクタ | §6、Consequences | 解決 |
| m6 group の枠 | §6、§13（手動確認）、恒等 `xfrm` | 解決 |
| m7 preset geometry | §6。`custGeom` は拒否、旧 `avLst` を外す、`hf`/`vf` を doc に書く | 解決 |
| m8 定義ファイルの provenance | §6 | 解決。記録の細部は N4 |
| m9 テーブルスタイル | §9。`@def`、大文字の GUID、Layer 3 の誤りを訂正 | 解決 |
| m10 `hlinkMkLst` とジャンプ先 | §7、Consequences | 解決 |
| m11 箇条書き | §4。グループごとに `None` = 外す、`buBlip` の関係 | 解決 |
| m12 下線の線・塗り | §3 | 解決 |
| m13 リンクのスキーム | §7 | 解決 |
| m14 `themeOverride` | §10 | 解決 |
| Q1〜Q4 | Open questions | 解決 |

---

## Major

### N1. same-part copy mode の要件が、`ahyp:hlinkClr` の名前空間宣言を落としている

**根拠:**

- `ahyp:hlinkClr` は、コーパスの 14/14 件で `<ahyp:hlinkClr xmlns:ahyp="…" val="tx"/>` の形をしていた
  （宣言が要素自身にある）。ADR の 12 件は slides だけを数えた値で、14 件は layout と master を含めた値である。
- #18 の `detach_node` は、残す部分木の `xmlns:*` を `DetachRefusal::NamespaceDeclaration` で拒否する。
  そのうえで名前の prefix を消し、挿入先で選び直す。したがって、宣言を許したとしても、`ahyp` は挿入先で
  スコープに無く、`NamespaceNotInScope` になる。
- rev 2 §3 は、mode を「`r:` id を保つ、`a:extLst` を保つ、`smtId` とほかの `DetachRefusal` は引き続き拒否」と
  定義している。この定義どおりに実装すると、`hlinkClr` を持つ run は分割できない。§3 の「the only one in the
  corpus is `ahyp:hlinkClr`」が保たれる、という主張は成り立たない。
- さらに、§7 の `text_color: true` は、Phase 3 自身が `ahyp` の宣言を持つ `a:ext` を作る。つまり、
  **このライブラリが作ったリンクの run が、後で範囲書式を適用できなくなる**。Q3 の既定（「分割では保持される」）
  とも矛盾する。

**推奨:** same-part copy を、#18 の detach（prefix を消して選び直す）の変種ではなく、**span を保つクローン**として
定義する。

- `RawNode` を span ごと clone する。bytes はそのまま書かれる。部分木の宣言は一緒に移り、prefix も変わらない。
- 照合は PR #44 の移動と同じにする。部分木の自由なプレフィックス（M1 を直した後なら MC 属性値のものも含む）、
  `mc:Ignorable`、継承された `xml:space` が、挿入先で同じ意味であること。
- 分割で作る run は、同じ `a:p` の兄弟なので、祖先のスコープは同一になり、照合は常に通る。core の要件として
  書くなら、「#44 の照合を流用し、コピーを許す」で足りる。
- そのうえで、拒否として残すのは次の 2 つだけでよい。
  - `[detach] unique_attrs`（`smtId` と `a:CT_EffectContainer@name`。N3）
  - 許可していない `a:ext`（N2）
- §3、§11 の `FormatNotCopyable` の行、Q3 の文言を、これに合わせる。

---

## Minor

### N2. `a:extLst` を「丸ごと保つ」のは広すぎる

**根拠:**

- #18 の `dml.toml` には「`a:extLst` content is open (not covered by this review)」とある。
- `a:extLst` の中身は自由な拡張で、一意であるべき id や、1 回だけ参照されるべき値を含み得る。同じパートの中なら
  `r:` id は有効なままだが、一意性は保証されない。

**推奨:**

- 保つ `a:ext` を URI の許可リストで決める。現状は `{A12FA001-AC4F-418D-AE19-62706E023703}`
  （`ahyp:hlinkClr`）だけ。
- それ以外の `a:ext` を含む run の分割は `FormatNotCopyable` とする。

### N3. 残す拒否を列挙する

rev 2 の「the other `DetachRefusal`s」では、どれが外れて、どれが残るのかが読み取れない。

- 外れるもの: `RelationshipAttribute`、`ExtensionList`（許可リストの分）、`NamespaceDeclaration`。
- 残すもの:
  - `UniqueAttribute`。`smtId` の 0 以外の値と、`a:effectDag`/`a:cont` の `@name`（#18 R3-1、ECMA-376
    §20.1.8.20/25）。
  - MC。

`a:effectDag@name` は名前を挙げておく（コーパスでは `a:rPr` の中に 0 件）。

### N4. 分割前の事前検証の範囲

rev 2 で、値の検証は分割より前になった。同じく、次の検証も最初の分割より前に済ませると書く。

- **両端の run の copy 拒否**: N1〜N3 の拒否である。範囲の始まりと終わりで、2 回分割するため。
- **`RangeSplitsField`**。

あわせて次の 2 点を書く。

- 原則 4 の「every value of the `RunFormat` を既に持つ」は、**直接の** `a:rPr` の値で比べる。resolved の値で
  比べると、継承で bold の run には直接値が書かれず、原則 1 の provenance と食い違う。
- 同じ項目を set と clear の両方で指定した場合の扱い（`ValueOutOfRange` 相当で拒否するのか）。

§13 の T4 には、「2 つ目の境界の run が `FormatNotCopyable`」の場合を加える。

### N5. ノートマスターの `notesSz` と 4:3 variant

**根拠:**

- テンプレートのノートマスターは、テンプレートの `p:notesSz` を前提に配置されている。
- ノートマスターの関係を持たないデッキ 449 件のうち、392 件の `p:notesSz` は `6858000×9144000` だった。
  残りは違う値で、A4 の `7559675×10691812/3` が 35、レターの `7772400×10058400` が 15、横長の
  `9144000×6858000` が 2、ほか 5 だった。
- 同じ 449 件の `sldSz` の比率は、16:9 が 271、4:3 が 127、それ以外が 51 だった。
- 次の点が書かれていない。
  - テンプレートの配置をそのまま使うのか。
  - デッキの `notesSz` に合わせてプレースホルダの `a:xfrm` を拡大・縮小するのか。

**推奨:**

- プレースホルダの `a:xfrm` を、`notesSz` の比で拡大・縮小する規則を書く。`sldImg` は、その枠の中に
  `sldSz` の比率で合わせる。
- この規則をすべての比率に適用すれば、4:3 variant は不要になる（16:9 と 4:3 は計算の特別な場合にすぎない）。
- variant を残す場合は、次のことを書く。
  - 置き場所。`Presentation::new()` のパッケージに入れると孤立したパートになるので、テンプレート外の
    埋め込みファイルにする。
  - ADR-0012 §8 の規則（自作、`--check`）が適用されること。
  - ADR-0012 にも 1 行のノートを入れること。
- 「4:3」の判定を `cx*3 == cy*4` と明記する。

### N6. provenance の記録の細部（主張そのものは確認できた）

**確認できたこと:**

- `OfficeOpenXML-DrawingMLGeometries.zip` の SHA-256 は `a8517838…02abf6`、
  `presetShapeDefinitions.xml` は `2f7c868d…7b6939` で、どちらも ADR と一致した。
- Part 1 の zip には notice のファイルが無い。geometries zip の中身は、`presetShapeDefinitions.xml` と
  `presetTextWarpDefinitions.xml` の 2 ファイルだけ。
- PDF の 2 ページ目から抽出したテキストは空だった。本文全体を検索しても、「copyright」は W3C の
  スキーマ引用部にしか現れない。

**推奨:**

- Part 4 の `PROVENANCE.md` と同じく、**外側の download の SHA-256** も記録する。手元の
  `ECMA-376-1_5th_edition_december_2016.zip` は 43,501,721 bytes、`9d0bcad9…d4129` だった。
- ADR の本文でも、省略しない hash を `PROVENANCE.md` に書くと明記する。
- 版の日付をそろえる。PDF の表紙は「October 2016」、Ecma のファイル名は「december_2016」、Part 4 の
  `PROVENANCE.md` は「(December 2016)」と書いている。
- 生成する表は、notice の (iv)（「implementing … the functionality」）に当たる派生物で、notice を添える
  条件がある。ADR-0002 §2 の運用と同じく、その表を持つ crate の `THIRD-PARTY-NOTICES` に notice を
  入れると書く。
- 実物のルート要素名は `presetShapeDefinitons`（綴りの誤り）である。codegen はこの名前で照合する必要がある。

### N7. `tableStyles.xml` の検出と、リンク置換の手順の順序

- **`tableStyles.xml` の検出**: m1 と同じく、ファイル名ではなく、presentation → `tableStyles` の**関係**で
  判定する。作成は `add_part(PartNameRequest::Exact("ppt/tableStyles.xml"))` になる。dangling な関係や
  `Override` がその名前を使っている場合、core は `NameInUse` を返す。これは §11 の `Core(…)` で拒否する、
  と書く。
- **リンク置換の順序**: 次の順で行う、と 1 文書く。
  1. `add_relationship`
  2. 新しい id で clone を編集する
  3. `stage_part`
  4. 古い id を `remove_relationship`

  core の制約による順序である。`stage_part` は、先に削除した id を参照する内容を `ConflictsWithPlan` で
  拒否し、`remove_relationship` の走査は staged の内容を読む。

---

## Nit

- ヘッダの ADR-0012 は main に merge 済み（Accepted rev 3）。PR 番号ではなく、
  `0012-pptx-phase2-slides-and-shapes.md` へのリンクにする。
- §7 は「`ahyp` の宣言は作成した `a:ext` に置く」と書いているが、PowerPoint は `ahyp:hlinkClr` 自身に置く
  （14/14）。どちらも妥当だが、PowerPoint に合わせれば diff が小さくなる。
- コーパスの `ahyp:hlinkClr` は、slides だけなら 12 件、layout と master を含めると 14 件。どちらを数えたかを
  書く。
