# Word / PowerPoint への ADR-0002 の適用可能性（調査メモ, 2026-11-16）

ADR-0002（スキーマ駆動のコード生成）の仕組みは、Excel の Phase0 を前提に設計・レビューした。このメモは、同じ仕組みが Word（WordprocessingML）と PowerPoint（PresentationML + DrawingML）にもそのまま通用するかを、データと実ファイルで確かめた結果である。

## 結論

- 生成の仕組み（入力、生成器、定数表 + view、スロットへの挿入、変更済みフラグ）は Word と PowerPoint にもそのまま使える。
- ただし調査の途中で、**ADR-0002 のスロット規則の誤りを1件見つけた**。「繰り返さない Choice の選択肢が繰り返す場合は排他的」と判定する際に、外側のグループが繰り返すかどうかを見ていなかった。このため Word の中心要素（`w:p`、`w:tbl`、`w:tr`、`w:tc`、`w:hyperlink` など）がすべて挿入不可と判定されていた。規則を3点直した結果、挿入不可は全体で 66 件から **12 件**になり、Word の中心要素はすべて挿入できるようになった。修正内容は ADR-0002 revision 5 に反映した。
- 実ファイル 197 個（Apache POI のテストデータに含まれる docx と pptx）で、主要な親要素 18 種、合計 171,537 個の子要素の並び順を、直したスロット規則と照合した。規則に反する並びは **0 件**だった。
- Word と PowerPoint で ADR-0002 の生成部分に追加で必要になるのは、WML の on/off 型の既定値を補正ファイルに1件足すことだけである。むずかしいのは生成ではなく、ドメイン層（パートをまたぐ ID の整合、フィールド、変更履歴、プレースホルダーの継承）。これは Excel の Phase0 と同じく、フォーマットごとの ADR で扱う。

## 1. スロット規則の修正（ADR-0002 §4.1 revision 5）

| 修正 | 内容 | 影響 |
|---|---|---|
| 繰り返しは外側まで含めて判定する | 繰り返すグループの内側にある Choice は、それ自体に `maxOccurs` が無くても、実質的に繰り返す Choice として扱う。 | 修正前は `w:p` の中身（`Group* > Choice > …`）が「排他的な Choice」と誤判定されていた。 |
| `(a? b? …)*` ≡ `(a｜b｜…)*` | 繰り返す並びの各要素がすべて省略可能なら、並び全体を1つの自由なスロットとみなす（どちらも同じ集合を受理する）。 | SDK が w14 の拡張を `Sequence[w14:conflictIns?, w14:conflictDel?]` として繰り返す Choice の中に入れており、これが「多スロットの選択肢」と判定されていた。トップレベルの `x:CT_RPrElt/x:rPr` も同じ理由で、ECMA と同じ「順不同」として扱えるようになった。 |
| 同じ QName が2つのスロットにある場合 | モデルを挿入不可にはしない。その QName を挿入するときだけ、呼び出し側が明示的にスロットを指定する（指定しなければ `Err(AmbiguousSlot)`）。 | `w:tbl` では、範囲マーカー（`bookmarkStart` など 20 種）が `tblPr` の前と行の間の両方に現れる。行（`w:tr`）の挿入は曖昧にならない。 |

修正後の内訳（`spikes/adr-0002/py/slottable.py`、`Occurs` の読み方はヒューリスティック）:

| 名前空間グループ | 全体 | 単一 QName のスロットだけ | 複数 QName のスロットを含む | 挿入不可 |
|---|---:|---:|---:|---:|
| SML（基本） | 297 | 261 | 34 | 2 |
| SML（拡張） | 237 | 221 | 16 | 0 |
| **WML（基本）** | 165 | 100 | 65 | **0** |
| WML（拡張） | 84 | 58 | 26 | 0 |
| **PML（基本）** | 172 | 125 | 47 | **0** |
| PML（拡張） | 84 | 75 | 8 | 1（`p188:cm`） |
| DML（基本） | 233 | 122 | 111 | 0 |
| Chart | 367 | 273 | 87 | 7 |
| SmartArt | 79 | 58 | 21 | 0 |
| Math | 58 | 44 | 13 | 1（`m:rPr`） |
| その他 | 346 | 236 | 109 | 1 |
| **合計** | **2,122** | **1,573** | **537** | **12** |

ECMA の XSD と順序が食い違うもの（SDK のスロットでは許されるが ECMA では禁じられる順序がある）は 8 件で、修正前と変わらない。WML では次の 6 件が該当する。

- `tcBorders`、`tcMar`、`tblBorders`、`tblCellMar` の4つの `left`/`start`
- `sdtPr`
- `fieldMapData`

これらは `set_content` で直すまで挿入不可のままにする。

## 2. 主要な要素のスロット構造

| 要素 | スロット | 備考 |
|---|---|---|
| `w:body` | [ブロック要素 35 種（自由）, `sectPr`] | `sectPr` は最後のスロットなので、段落を追加しても常にその前に入る。 |
| `w:p` | [`pPr`, 段落の内容 60 種（自由）] | ラン、ハイパーリンク、フィールド、範囲マーカー、変更履歴、数式などが文書順で混在する。 |
| `w:r` | [`rPr`, ランの内容 32 種（自由）] | `t`、`br`、`tab`、`fldChar`、`drawing` など。 |
| `w:tbl` | [範囲マーカー, `tblPr`, `tblGrid`, 行の内容（自由）] | 範囲マーカーは2つのスロットに現れる（曖昧スロット）。 |
| `w:pPr` / `w:rPr` | 36 / 51 スロット、すべて単一 QName | Excel の `worksheet` と同じく、決まった順序の列。 |
| `p:sld` | [`cSld`, `clrMapOvr`, `transition`, `timing`, `extLst`] | |
| `p:spTree` | [`nvGrpSpPr`, `grpSpPr`, 図形 6 種（自由）, `extLst`] | 自由スロットの中の順序が重なり順（z-order）になる。 |
| `p:txBody` / `a:p` / `a:r` | 3 / 3 / 2 スロット | `a:p` の中身（`r`、`br`、`fld`）は自由スロット。 |

「自由スロット」の中の順序は、Excel の行・列と違ってキーで決まらず、文書順や重なり順として呼び出し側が決める。これは `insert_by(before)` の想定どおりである（例: 「この段落の後ろ」「この図形の前面」）。

## 3. 実ファイルでの検証（Apache POI テストデータ）

対象は `apache/poi` の `test-data/document`（docx 129 個）と `test-data/slideshow`（pptx 94 個）。開けた 197 個を使った。読んだパートは `word/document.xml`、ヘッダーとフッター、`ppt/slides/*.xml`、`ppt/presentation.xml`。スクリプトは `spikes/adr-0002/py/corpus_check.py`。ファイル自体はリポジトリに入れていない（Apache-2.0 の外部データ）。

- **順序:** 18 種の親要素、合計 171,537 個について、子要素の並びを修正後のスロット規則と照合した。違反は 0 件。
- **`mc:AlternateContent` が直接の子として現れた場所:**

  | 親要素 | 出現数 | ファイル数 | 主な中身 |
  |---|---:|---:|---|
  | `w:r` | 18 | 5 | 図形やテキストボックス（`wps`）のフォールバック |
  | `a:graphicData` | 12 | 4 | |
  | `p:sld` | 10 | 5 | `p14` のトランジションやタイミング（`ooxmlsdk` の補正データでも `transition`/`timing` が AC で包まれる例として扱われている） |
  | `wp:anchor` | 4 | 1 | |
  | `pic:pic` | 1 | 1 | |

  `w:body`、`w:p`、`p:spTree`、`a:p`、`p:txBody` には一度も現れなかった。ADR-0002 の「AC がある親への挿入は `Err`」は、テキスト編集や段落・ランの追加には実質的に影響しない。影響するのは、図形を含むランに子を足す場合と、トランジションを持つスライドに `timing` などを足す場合だけである。
- **未知の子（スロットに無い子）:**
  - `w:p` の下の `w:smartTag`（24 回）。ECMA Transitional の要素だが、SDK の `CT_P` には無い。ADR の規則どおり、直前の兄弟に付いているものとして保持される。
  - `w:rPr` の下の `w:ins`/`w:del`。段落記号の変更履歴で、実際の型は `CT_ParaRPr`。親の位置によって型が変わる例で、ADR の「(型, 要素) 単位」の前提を裏付ける。
- **テキストの分割:** テキストを持つ段落 21,857 個のうち、ランが2つ以上に分かれているのは 6%（平均 1.28 ラン）。POI のテストデータは手で作られたものが多く、Word で編集を重ねた実文書ではもっと多いと考えられる（推測）。`w:t` の 14% に `xml:space="preserve"` が付いていた。

## 4. 属性の既定値

| 接頭辞 | SDK 上の属性 | XSD から既定値が付く |
|---|---:|---:|
| `x`（SML） | 1,729 | 866 |
| `w`（WML） | 606 | **24** |
| `p`（PML） | 302 | 137 |
| `a`（DML） | 723 | 292 |
| `c`（Chart） | 123 | 51 |

- WML の既定値が少ないのは、WML の on/off 型（`<w:b/>` と書けば「オン」）の意味が XSD の `default=` ではなく仕様の本文で決まっているためである。`CT_OnOff` の `val` には XSD 上の既定値が無い。SDK では 186 個の要素がこの型（基底クラス `OnOffType`）を使う。基底のエントリ `w:CT_OnOff/` の `val` に `set_default`（値は `true`）を1件置けば、186 個すべてに効く。このため ADR-0002 §1.3 で、基底エントリに置いた override が派生エントリに継承されることを明記した。
- 型に合わない既定値 39 件の内訳は `a` 25、`c` 9、`p` 5 で、WML には無い。DrawingML のパーセント値（`ST_Percentage`）が主な原因なので、PowerPoint を対象に入れる前に `set_type` か union 型の扱いが必要になる（ADR-0002 R3-m6 で記録済み）。

## 5. 生成の外側で必要になること（将来のフォーマット別 ADR 向け）

Excel の Phase0 で、共有文字列表とスタイル表を「追記のみの登録簿」（ADR-0001 Layer 3）にしたのと同じく、Word と PowerPoint にもパートをまたぐ整合性がある。ここでは列挙だけして、設計は後続の ADR に回す。

**Word**
- `styles.xml` のスタイル ID
- `numbering.xml`（`numId` → `abstractNumId`）
- コメント、脚注、文末脚注の `w:id`
- ブックマークの `w:id` の一意性
- `w14:paraId` の一意性
- `settings.xml` の `rsids`
- フィールド（目次、ページ番号など）の結果はキャッシュである。Excel の数式キャッシュと同じ問題で、`updateFields` を使うか、フィールド内の編集を `Err` にする。
- 変更履歴（`w:ins`/`w:del`）の中の編集をどう扱うか。
- ランをまたぐテキストの検索と置換。

**PowerPoint**
- `presentation.xml` の `sldIdLst`（id は 256 以上で一意）
- スライド → レイアウト → マスターのリレーションの連鎖
- スライドごとの図形 ID（`cNvPr/@id`）の一意性
- ノート
- グラフに埋め込まれた xlsx
- プレースホルダーの継承（書式の実効値）

**Excel の Phase0 にあたる最初の範囲の候補**（判断）
- Word: 既存のランのテキストの置き換え、`w:body` 末尾（`sectPr` の前）への段落の追加。新しいスタイル、番号、ID は作らない。フィールドの中と変更履歴の中への書き込みは `Err` にする。
- PowerPoint: 既存の図形の `a:t` の置き換え、`a:p`/`a:r` の追加。スライドと図形の追加は ID の登録簿が要るので後回しにする。

## 6. ECMA の XSD のライセンス

- ECMA-376 Part 4（第5版、2016年12月）を Ecma の公式サイトから取得した。配布 zip（`ECMA-376-4_5th_edition_december_2016.zip`、SHA-256 `bd25da11…34ecd`）の中にある `OfficeOpenXML-XMLSchema-Transitional.zip`（SHA-256 `d3418752…c04fc`）が XSD の原本である。`ooxmlsdk` の同梱物とは改行コード（CRLF）以外は同一で、ADR-0002 の数値はそのまま有効。
- この版の PDF には著作権表示のページが無い（2ページ目は空白）。XSD にもライセンスのヘッダーは無い。
- Ecma の text copyright policy（Version 3、2025年12月承認）の FAQ は、既定の著作権表示を2009年以降の標準すべてに入れてきたこと、それ以前の標準には遡って入れていないが、実際には同じ原則が適用されることを述べている。既定の表示は次のとおり。
  - 著作権表示とこのライセンス・免責の文言を添えるかぎり、複製、公開、配布ができる。
  - 許される派生物の (iv) は「標準に適合する製品の中で、この仕様を実装すること（全部または一部のコピー＆ペーストを含む）」。
  - 文書の内容そのものは改変できない。
- したがって、次の3つを守れば同梱してよいと判断した。
  1. XSD は**無改変**で同梱する（改行コードも原本のまま）。補正は別ファイル（`overrides.toml`）で行い、XSD 自体には手を入れない。これは ADR の方針どおり。
  2. `reference/ecma-376-transitional-xsd/` に Ecma の既定の著作権表示の全文と出典（URL、版、SHA-256）を置く。
  3. 生成コードに既定値を取り込むことは (iv) にあたる。`ooxml-schema` の `THIRD-PARTY-NOTICES` にも Ecma の表示を載せる。

  なお、BSD ライセンス（Ecma の標準に含まれるソフトウェア部分を対象とするもの）は、その旨が著作権ページに書かれた標準にだけ適用される。ECMA-376 の 2016 年版にはその記載が無いので、BSD ライセンスは当てにしない。法的な助言ではなく、公開されている条項を読んだ結果である。

## 出典

- Apache POI テストデータ: https://github.com/apache/poi/tree/trunk/test-data （`document/`、`slideshow/`、2026-11-16 取得）
- ECMA-376 の各版: https://ecma-international.org/publications-and-standards/standards/ecma-376/
- Ecma text copyright policy Version 3: https://ecma-international.org/wp-content/uploads/Ecma_text_copyright_policy_version_3.pdf
- Ecma text copyright policy（Web版）: https://ecma-international.org/policies/by-ipr/ecma-text-copyright-policy/
- スクリプト: `spikes/adr-0002/py/`（`slottable.py`、`bynamespace.py`、`corpus_check.py`、`join3.py`、`typecheck.py`）
