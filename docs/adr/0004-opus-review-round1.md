# ADR-0004 レビュー（Opus, round 1）

対象: `docs/adr/0004-docx-phase0.md`（Status: Proposed, 2026-11-16, branch `main`）

方針: 読み取り専用の敵対的レビュー。ADR-0001、ADR-0002（Accepted rev 5）の primitives・allowlist・overrides、roadmap `docs/roadmap/docx.md`、`spikes/docx/` を突き合わせて検証した。

- 数値は spike を再実行して確かめた。`phase0_gate.py` と `readback_compare.py` を `python3 -I` で、fixtures 25 件、POI 129 件、SDK の非 Strict 444 件に対して実行した。ADR-0002 の `slottable.py` は SDK `431ab05c` と ECMA Transitional XSD に対して実行した。
- 未信頼の入力は zipfile と ElementTree で読んだ。DTD を含む part は読み飛ばした。検証用スクリプトは fixture の外、scratchpad の `review-0004/` に置いた。
- 重大度は3段階。
  - **[Blocker]**: 実装前に ADR の修正が必要。
  - **[Major]**: 安全性か設計の整合性に関わる。ADR 本文の修正が必要。
  - **[Minor]**: 記述や数値の修正、テストの追加で済む。
- 判断が分かれる指摘には「判断」と明記した。

## 結論（先に）

Phase0 の骨格は妥当で、ADR-0002 の上で実装できる。

- 骨格: 既存 run のテキスト置換と、`sectPr` の前への段落追加。ID と part を作らない。文書単位と run 単位の型付き拒否。
- 確認できた点:
  - allowlist の SDK `Name` 31 件はすべて実在する。
  - write の6件は ADR-0002 §4.1 の slot に曖昧さなく落ちる。`w:body` = [35 QName, `sectPr`]、`w:p` = [`pPr`, 61 QName]、`w:r` = [`rPr`, 33 QName]。
  - `CT_OnOff` の XSD には `default` が無い（`wml.xsd` L23-25）。したがって `set_default` override は確かに必要。
  - §6 の数値はすべて再現した。SDK 404/444、拒否の内訳 30/6/2/2、23,162/24,558、POI 106/129、27,407/27,682、fixture の 3,110。docx-rs の被覆率（57,615/57,616 など）も再現した。

ただし、安全性と整合性の点で ADR 本文の修正が要るものが3つある。

1. **M1:** 「権威ある実装」とされる `phase0_gate.py` のフィールド走査は、入れ子のフィールドで**外側のコードを捨てる**。そのため Word の差し込み印刷の定型 `{ IF { MERGEFIELD … } … }` が「安全」として通る。ADR 本文も入れ子の扱いを規定していない。
2. **M2:** §3 の API に必要なナビゲーションを、ADR-0002 の primitives では書けない。
   - 書き込み: 表の中や hyperlink の中の run、`append_paragraph` に至る経路は、`[read]` に置いた `document`/`tbl`/`tr`/`tc`/`hyperlink` を可変で降りる必要がある。
   - 読み取り: `sdt`/`smartTag`/`customXml` を経由する読み取りには、型なしの走査と QName を確かめる型変換が要る。ADR-0002 にはどちらも定義が無い。
3. **M3（判断）:** `w:documentProtection`（readOnly / forms / comments が enforcement 付き）の文書を Phase0 は黙って書き換える。POI では4件が Editable になる。roadmap は保護を Phase4 としているが、Phase0 はそれを拒否していない。

Blocker は無い。M1〜M3 は記述の追加と小さなゲートの追加で直せる。

---

## M1 [Major] W2 の入れ子フィールド分類: spike が外側のコードを捨てる。ADR も入れ子を規定していない

### 根拠
- ADR L296: W2 は「after joining split `instrText`」でコードを判定するとしか書いていない。L311 は分類の正を `phase0_gate.py` の表とし、「to be ported as a table」としている。
- `phase0_gate.py` の走査（`gate()` 内）は、`begin` のたびに `cur = ''` で上書きする。`separate`/`end` で `cur` を確定して `None` にするので、入れ子では**最も内側のコードだけ**が数えられる。コメントの「outermost-first is enough to classify nested codes too」は事実と逆。
- 再現（`review-0004/nested.py`）: Word の差し込み印刷の「ルール → If…Then…Else」が出力する形 `{ IF { MERGEFIELD Gender } = "F" "Ms." "Mr." }` を作って試した。
  - spike のアルゴリズムは `{'MERGEFIELD': 1}` を数え、bad は `[]`。つまり Editable になる。
  - スタックで数えると `{'MERGEFIELD': 1, 'IF': 1}` で、bad は `['IF']`。
- コーパスでは、外側のコードの取りこぼしが2件で起きている。`complex2010.docx` では外側の `TC` を、`complex1_NOR.docx` では外側の `tc` を落としている。どちらも内側の `SEQ`/`=` で拒否されるため、ファイル単位の判定（404/444 など）は変わらなかった。

### なぜ問題か
- Phase0 の主な用途は template filling で、`IF`+`MERGEFIELD` はまさにその文書に現れる。外側の `IF` は内容に依存する（ADR 自身が CONTENT_DEPENDENT に入れている）のに、内側が安全なら W2 を素通りする。`{ = { PAGE } - 1 }` や `{ HYPERLINK "{ REF url }" }` も同じ構造をしている。
- 表だけを移植すると、実装者は spike のループも一緒に写しやすい。fixtures にも T4 にも入れ子の反例が無いので、この誤りは検出されない。

### 提案
- §6 W2 に次を明記する。
  - フィールドは文書順のスタックで追う。
  - 各フィールドのコードは、自分の `begin` から `separate`（無ければ `end`）までの、**自分の階層の** `instrText` だけを連結して得る。
  - **入れ子のすべての階層**を分類する。
  - Word は命令部の中にある入れ子フィールドの結果を `w:instrText` として書く（`complex1_NOR` で確認した）。このため外側のコードには混ざらない。
- `phase0_gate.py` を直して数値を取り直す。今回の再計算ではファイル単位の数値は変わらない。
- T4 に unit test を足す。`IF{MERGEFIELD}` は `StaleFieldRisk { codes: ["IF"] }`、`{ = {PAGE} }` は `["="]` になること。
- 判断: `IF` を「入れ子がすべて safe なら safe」とするかどうかは、owner への質問1に加える。差し込み印刷のテンプレートが丸ごと拒否されるかどうかがこれで決まる。

---

## M2 [Major] §3 の API に必要なナビゲーションが ADR-0002 の primitives に無い（`[read]` の意味が未定義）

### 根拠
- ADR-0002 §3 と §4.2 で子 view を得る方法は、生成される `*ViewMut` のメソッドで親から借用し直す経路だけ。
  - 「child views are reborrowed from the parent view」とある。
  - 「primitives return the new child's view instead of handing out paths」とあり、path から view を作る公開 API は無い。
  - 型なしの子の走査や、QName を確かめる型変換（cast）も定義されていない。
- ADR-0004 L207 では、`[read]` の view は「writes through these views return Err」とされている。その `[read]` に `w:CT_Document/w:document`、`w:CT_Tbl/w:tbl`、`w:CT_Row/w:tr`、`w:CT_Tc/w:tc`、`w:CT_Hyperlink/w:hyperlink` が入っている。
- 書き込み経路で必要なもの:
  - `append_paragraph` は `body.insert::<P>()`（L183）。`BodyViewMut` は `document` の可変 view から得るしかない。
  - 表の中の段落の `set_run_text` には `tbl`→`tr`→`tc`→`p` の可変の降下が要る。
  - `Hyperlink` コンテキストの run（L141 で settable）には `hyperlink`→`r` の可変の降下が要る。
- 読み取り経路で必要なもの:
  - `paragraphs()` は `w:sdt/w:sdtContent` の中まで降りる（L110-112）。
  - `text()`/`runs()` は `w:smartTag`、`w:customXml`、`w:sdt` を通る（L123-127）。
  - ところが L236-238 は、これらを「recognized by QName only and need no generated views」としている。`w:smartTag` は SDK の `CT_P` に無い（確認済み: `CT_P` の 61 QName に含まれない）ので unlisted 子になる。
  - したがって型なしで降りて、その先の `w:p`/`w:r` を `PView`/`RView` に変換する手段が要る。
- L227-229 の「`ChildOf` impls exist exactly for Body→P, P→R, R→T, R→Tab and R→Br among write entries」について。ADR-0002 §3 では、`ChildOf` は「両方が allowlist にあれば」生成される。したがって `ChildOf<R> for FldChar`、`ChildOf<Ins> for R`、`ChildOf<Tc> for P` なども生成される。`[read]` の書き込み禁止はコンパイル時には効かない。

### なぜ問題か
- 書かれた通りに `[read]` を「可変 view 無し」と実装すると、次の3つが書けない。
  - `append_paragraph`
  - 表の中の run の編集（`poi-table-alignment` は 66 run すべてが表の中）
  - hyperlink の中の run の編集（fixture で 308 run、POI で 365、SDK で 286）
- 逆に「可変 view はあるが primitives が Err」と読むなら、その判定を実行時のどこに置くのかが ADR-0002 に無い。
- 読み取りの型変換が無いと、`sdt`/`smartTag` 経由の `text()` が書けない。

### 提案
- `[read]` の意味を「構造・属性・テキストを変更する primitives は `Err(ReadOnlyEntry)`。ただし `*ViewMut` による可変の降下は生成する」と定義する。owner への質問6にこの定義を添える。
- ooxml-core に、次の2つを foundation への要求として §3 か §4 に書く。
  - 型なしの子の列挙（`children_raw()`: QName、index）。
  - 検査つきの型変換（`Lens::<E>::try_from_child(raw)`。QName が一致すれば view、違えば Err）。
- `sdt`/`sdtContent`/`smartTag`/`customXml` は、この型なしの経路でだけ通ると明記する。
- L227 の `ChildOf` に関する主張は「コンパイル時の保証ではない」と書き直す。

---

## M3 [Major, 判断] 文書保護（`w:documentProtection`）にゲートが無い

### 根拠
- POI のコーパスに、`enforcement="1"` の保護がかかった文書が8件ある（`review-0004/prot.py`）。
  - `edit="trackedChanges"` の2件（`bug56076`, `documentProtection_trackedChanges_no_password`）は、どちらも `<w:trackRevisions/>` を持っているので W1 で止まる。
  - `edit="readOnly"` の2件（`documentProtection_readonly_no_password`, `protected_sample`）は Editable になる。
  - `edit="forms"` の1件（`documentProtection_forms_no_password`）は Editable になる。
  - `edit="comments"` の1件（`documentProtection_comments_no_password`）は Editable になる。
  - `w:formatting="1"` だけの2件（書式ロック）も Editable になる。こちらは編集が許される保護なので問題ない。
- roadmap row 21 は保護を Phase4 に回している。ADR-0004 §6 にも §5 にも保護の扱いが無い。

### なぜ問題か
- ADR-0001 以来の「扱えないものは型付きで正直に拒否する」方針と食い違う。作成者が「読み取り専用」「フォームのみ」とした文書の本文を、Phase0 は何も報告せずに書き換える。Word は保護をかけたまま開くので、利用者には改変がもともとの内容に見える。
- 保護はセキュリティ機構ではない（ハッシュは編集 UI の鍵にすぎない）。この意味で判断の分かれる点ではある。ただ、ライブラリが既定で黙って迂回するのは、`trackRevisions` を W1 で拒否する理由（作成者の意図を裏切らない）と同じ理由で避けるべきだと考える。
- `edit="trackedChanges"` の保護で `trackRevisions` が無い文書は、コーパスには無かった。それでも W1 が `trackRevisions` 要素だけを見ている限り、そうした文書は通ってしまう。

### 提案
- W3 `DocumentProtected { edit }` を加える。発火条件は `settings/w:documentProtection` の `enforcement` が on で、かつ `edit` が `none` 以外であること。fixture は POI の `documentProtection_readonly_no_password.docx`（追加候補）。
- 将来 `edit="trackedChanges"` を W1 と同じ扱いにすることも書いておく。
- `CT_DocProtect` を `[read]` に追加する。`enforcement` は `ST_OnOff` なので、M2 の override がそのまま効く。
- 保護を無視して書くオプトインは Phase1 以降とする（`updateFields` と同じ扱い）。

---

## m1 [Minor] `set_run_text` のテキスト対応表: `\r`、整形用の空白ノード、自分自身のテキストの書き戻し

### 根拠と問題
- L163-168 は `\t` と `\n` しか対応させていない。U+000D は XML 1.0 で許される文字なので `w:t` に入る。
  - リテラルの CR は、再パースのとき XML の行末正規化（XML 1.0 §2.11）で LF になる。`&#xD;` にエスケープするかどうかも ADR-0001/0002 に規定が無い。
  - Windows 由来の `"a\r\nb"` は `a`、`\r`（`w:t` の中）、`<w:br/>`、`b` になる。書いたものを読み直すと内容が変わり、T1b の構造比較とも食い違う。
- 整形済みの文書では、`w:r` の中に空白だけのテキストノードがある。`poi-table-alignment` は `<w:r>\n   <w:t>Amet</w:t>\n </w:r>`、`sdk-HelloO14` も同様。
  - L169 の「children other than `w:rPr` is exactly one `w:t`」でテキストノードを子に数えると、fast path に入らない。
  - slow path の「remove every content child」が空白ノードを消すのかどうかも決まっていない。
  - XML コメントや PI が `w:r` の中にある場合の扱い（R4 で拒否するのか）も同じく未定義。
- 自分自身のテキストを書き戻しても恒等にならない。
  - `text()` は `w:noBreakHyphen` を U+2011 と読むが、書き戻すと `w:t` の中のリテラル U+2011 になる。
  - `w:cr` は `<w:br/>` になる。
  - `w:softHyphen` は `text()` に含まれないので、書き戻すと消える。
  - T2 の「own text」は差分が出る前提で書かれていない。

### 提案
- 対応表に次を加える。
  - `\r\n` と単独の `\r` は `<w:br/>` に対応させる（または `Err(InvalidXmlChar)`）。どちらかに決める。
  - 空白だけのテキストノードは content とみなさない。fast path の判定でも slow path の削除でも無視する。
  - `w:r` の中のコメントと PI は R4 で拒否する。
- 写像した結果が現在の内容と同じなら何もしない（差分 0）と規定する。そのうえで、T2 の「own text」が期待する結果を書く。

## m2 [Minor] R1（InsideField）は文書順の状態が必要。spike は段落内の直下の run しか見ていない

### 根拠と問題
- ADR L318 の定義（begin から end の間）は正しい。ただ、spike の `runs_stats` には2つの制限がある。
  - `in_field` を段落ごとに 0 に戻している。
  - `for r in p` で直下の `w:r` だけを見ている。`w:hyperlink` や `w:ins` の中の `fldChar` は数えない。
- roadmap Q2 によると、段落をまたぐフィールドは SDK に 21 件あり、すべて TOC。現在は W2 で文書ごと拒否されるので顕在化しない。しかし Phase1 で `updateFields` を入れて W2 を外すと、TOC の結果の2段落目以降の run が R1 をすり抜ける。
- `begin` に対応する `end` が無い壊れたフィールドの扱いも書かれていない。

### 提案
- R1 に次を明記する。
  - 状態は story part 全体の文書順で持つ。段落・表のセル・`hyperlink`/`ins` の境界をまたぐ。
  - 対応のとれない `begin` があれば、その story の残りをすべて InsideField とみなす（fail closed）。
- unit test に「TOC の結果の2段落目の run が `InsideField` になること」を加える。

## m3 [Minor] 数値の分母: hyperlink 内の run が数えられていない。POI の内訳が合わない

### 根拠
`review-0004/gate2.py` で、到達できるすべての run（入れ子のフィールドはスタックで判定）を数え直した。

| 範囲 | ADR | 到達できるすべての run |
|---|---|---|
| fixtures の settable（T2） | 3,110 | 3,418（うち hyperlink 内 308） |
| SDK の settable / text run | 23,162 / 24,558 (94%) | 23,448 / 25,889 (90.6%)。context による拒否は 1,347 ではなく 2,392 |
| POI の settable / text run | 27,407 / 27,682 | 27,770 / 28,103 |

- L330「Of the rest, 15 are not zips or are corrupt」は、実測では NotZip 13 と CorruptPackage 1 で計 14。CFB の2件を含めるなら 16。roadmap L23 の「15 = 2 CFB + 13」とも一致しない。

### 提案
- T2 の母集団は「`runs()` が返し、かつ `check_settable() == Ok` の run」と定義し、数を 3,418 前後に直す。T2 がこの数を固定値として使うなら、確定値は実装の段階で出す。
- §6 の割合と L330 の内訳を直す。

## m4 [Minor] `runs()` が到達できる範囲と、`RunContext`/R2/R3 の fixture が噛み合わない

### 根拠と問題
- L123-127 の `text()`/`runs()` の到達先には、次のものが入っていない。
  - `w:dir`/`w:bdo`
  - `w:del`/`w:moveFrom`（意図的に除外）
  - テキストボックスと AC
- その結果、到達できない `RunContext` が生じる。
  - `RunContext::Bidi`、`Deleted`、`MovedFrom` は返りえない。
  - R3 の fixture `poi-shapes-with-text`（テキストボックス）は、`paragraphs()` がそもそも降りないので `UnsupportedContext` を観測できない。この点で T4 の「every row … returns exactly that variant」は成立しない。
- 読み取りにも漏れがある。
  - ブロックレベルの `w:customXml`（SDK の `CT_Body` の 35 QName に含まれる）の中の段落。
  - 行レベル・セルレベルの `w:sdt`/`w:customXml`。
  - `w:ruby` の親文字（日本語のルビ。`w:r/w:ruby/w:rubyBase/w:r/w:t`）。`text()` から落ちる。

### 提案
- `runs()` の到達範囲を表にする。
- `RunContext` の各変種が返る条件と、返らない変種を書く。
- R3 の fixture は、`poi-smarttag-snippet` と `sdk-coverpage1` のように実際に観測できるものに限る。
- `w:dir`/`w:bdo`/`w:ruby` の扱いを決める。`text()` には含め、`set_run_text` では拒否するのが自然。

## m5 [Minor, 判断] W2 の「safe」の基準の言い方と、操作ごとの判定

- L298-299 は safe の基準を「an edit elsewhere cannot make wrong in a way a reader could detect」としている。しかし `NUMPAGES`/`SECTIONPAGES`/`PAGE` の結果は、append や長い置換でページ数が変われば古くなる。キャッシュを読む利用者（docx-rs、python-docx、検索インデクサ）はその古い値を表示する。実際の基準は「Word と LibreOffice がレイアウト時や表示時に必ず再計算するフィールド」だろう。そう書き直すべき（判断）。
- 逆に、Phase0 の操作（既存 run のテキスト置換、末尾への追加、フィールドの追加や削除はしない）で結果が変わりえないコードもある。
  - `SEQ`/`TC`/`XE`/`LISTNUM`/`NOTEREF`/`RD`: フィールドの個数・順序・見出しスタイルが変わらないので、結果も変わらない。
  - TOC/PAGEREF/INDEX: 末尾への追加では変わらない。
- SDK で `['PAGEREF', 'TOC']` だけを理由に拒否されている 15 件は、`append_paragraph` に限れば許せる。拒否は過剰だが安全側であり、Phase0 としては許容できる。owner への質問1に「W2 を操作ごとに判定するか」を加えるのが良い。

## m6 [Minor] W1 は override に依存する。fail closed とテストを明記する

- L249-253 の通り、W1 の正しさは `w:CT_OnOff/` の `set_default` override に依存する。
  - ADR-0002 §1.2 では、型に合わない既定値は採用されない。`"true"` は SDK の `OnOffValue` の字句空間に入るので、採用される。
  - ADR-0002 §1.3 では、継承の鍵は型の部分（`w:CT_OnOff`）。`w:CT_OnOff/w:trackRevisions` はこの型を持つことを確認した。
- 足りないのは次の2点。
  - `val` が `ST_OnOff` の字句空間の外にあるとき（ADR-0002 では `AttrError` になる）、W1 を on とみなす（fail closed）規定。
  - override が外れたことを検出するテスト。
- T4 に `val` = `on`/`off`/`0`/`1`/`true`/`false`/不正値/省略の8通りを unit test として加える。override が消えたら `poi-delins` の T4 が落ちることも明記する。

## m7 [Minor] T5（docx-rs による読み戻し）の手順の細部

- docx-rs の `json()` では、`document.sectionProperty` の下に header/footer の本文が入る。`poi-Headers` では、文書全体の最後の `"text"` はヘッダーの `"Section 3"` になる（`rb_out` で確認）。したがって L364 の「appended text is the last body text」は、走査を `document.children` に限らないと成立しない。
- docx-rs は `xml:space` を見ない（`xml_parser.rs` は `trim_text(false)` で、`preserve` を参照しない）。したがって、`preserve` が付いていないことを T5 では検出できない。T2 または T6 に「先頭・末尾の空白か連続した空白を持つ新しい `w:t` には `xml:space="preserve"` がある」という構造の検査を足す。
- 被覆率の測り方（`readback_compare.py`）は、空白を除いた文字列が出力全体のどこかに含まれるかを見ているだけで弱い。順序を保つ照合（`review-0004/strict_rb.py`）でも、469 件中 459 件は全一致した。残りは主にテキストボックス（txbx は 94/124）で、Phase0 の対象外。docx-rs を選んだ判断は変わらない。測り方を ADR に一行書いておくと良い。

## m8 [Minor, 判断] `lastRenderedPageBreak` を消す理由と、他に古くなるキャッシュの一覧

- L173-175 は、編集した run の `w:lastRenderedPageBreak` を落とすことを `<dimension>` の再計算になぞらえている。しかし同じ文書の他の run にある LRPB（POI では 19 ファイル 202 個、SDK では 101 ファイル 820 個）も、改ページの位置がずれれば同じように古くなる。1つだけ落としても文書は一貫しない。
  - 代案1: 最初の content 子として置かれた LRPB は残す。位置の意味が変わらないので、slow path の差分も小さくなる。
  - 代案2: 落とすことを「一貫性のため」とは言わない。
- Consequences に「Phase0 が古いまま残すキャッシュ」の一覧を置く。
  - `docProps/app.xml` の統計（`Pages`/`Words`/`Characters`/`Lines`/`Paragraphs`）
  - `settings/w:proofState`（`clean`）*(u)*
  - `w14:textId`、rsid、他の run の LRPB

## m9 [Minor] O2 の DTD 検出は、パーサのイベントで行うと書く

- spike は先頭 4,096 バイトに `<!DOCTYPE` があるかを見ている。DOCTYPE の前には任意の長さのコメントや PI を置ける。
- L286 の「contains `<!DOCTYPE`」を、実装では quick-xml の `DocType` イベント（つまり prolog の全体）で判定する、と明記する。テストに「長いコメントの後に DOCTYPE がある」合成ケースを加える。

---

## 推奨する修正の優先順位

1. **M1:** W2 をスタック方式にし、全階層を分類する。spike を直して数値を取り直す。IF/= の入れ子の unit test を加える。
2. **M2:** `[read]` の意味（可変の降下は許す）と、型なしの走査・型変換を foundation への要求として書く。`ChildOf` に関する主張を直す。
3. **M3:** W3 `DocumentProtected` を加える（または owner の判断を Open questions に加える）。
4. **m1、m2:** テキストの対応表と R1 の状態の範囲。どちらも実装者が最初に踏む箇所。
5. **m4、m6、m7:** API の到達範囲、W1 のテスト、T5 の手順。
6. **m3、m5、m8、m9:** 数値、基準の言い方、キャッシュの一覧、DTD 検出。

## 判定

**ready for Accepted after Minor only: no.**

M1〜M3 は、それぞれゲートの規定かナビゲーションの規定を ADR 本文で直す必要がある。どれも局所的な修正で済み、設計の方向を変えるものではない。次のラウンドで確認するのは M1〜M3 の修正だけで足りる見込み。
