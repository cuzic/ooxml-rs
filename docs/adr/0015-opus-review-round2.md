# ADR-0015 レビュー（Opus, round 2）

- 対象: PR #39 `docs/adr/0015-docx-phase3-review-features.md`（Proposed, revision 2）と `spikes/docx-phase3/`
- 対象コミット: `b32a8bf`（worktree `plan-docx-phase3`）
- 照合したもの:
  - round 1 のレビュー `0015-opus-review-round1.md`、PR 本文の finding→fix 表
  - PR #38 `core/add-part`（`bdc133d`）: `crates/ooxml-core/src/package/add.rs`、round 1 のレビュー `docs/reviews/pr38-round1.md`（M1〜M7）
  - PR #45 `core/insert-with-ns`（`2a76520`）: `lens.rs` の `*_with_ns`、`NsDecl`、`set_attr`、`set_text`
  - PR #18 `detach_as`（`core-detach-insert`）、ADR-0004 の Alternative 9、ADR-0011 rev 2 の §2 と §9.1
  - [MS-OI29500] 2.1.229、2.1.230、2.1.258（Microsoft Learn で原文と目次の番号を確認した）
- 追加の測定:
  - spike の再実行: `scratchpad/review-pr39-r2/{fx,poi,sdk,d4j,tpl}.json` と `tables.md`
  - プローブ: `probes/p1_revkinds.py`、`p2_fx.py`、`p3_rpr_ext.py`
  - どれも `python3 -I` で実行し、パースだけを行い、エンティティは解決しない。

## 結論（先に）

**round 1 の指摘はすべて反映されていて、数値も再現する。ただし、修正の過程で新しい Major が 2 件見つかったので、このままでは Accepted にできない（判定: no）。**

- **B1 は解消した。**
  - spike を 5 コーパスで再実行し、`out/tables.md` と**完全に一致する**ことを確かめた（diff は 0 行）。
  - `commentEx` が最終段落を指す 1,089/1,090 件、返信 469 件（入れ子 52 件）、`done` が親と返信で異なる 36 + 12 = 48 件、プレリリース名前空間 54/59 件、`people` の一致 430/503 件。どれも ADR の本文と一致する。
  - 名前空間ごとの書き込み規則（2012 名前空間にだけ書き、プレリリース名前空間なら `LegacyCommentsExtended`）と、fixture の入れ替えも妥当である。`poi-testComment` は 2012 名前空間の `commentsExtended`、`commentsIds`、`commentsExtensible` を持ち、全パーツのルートが `w14 w15 w16se w16cid w16 w16cex` を Ignorable にしている。受け入れテストに向いている。
- **M2〜M5 は解消した。** M4 で引用している [MS-OI29500] 2.1.258 は、番号（目次で 2.1.257 と 2.1.259 の間）も引用文も正しい。
- **新しい Major 2 件:**
  - **N1:** M1 の修正で「新しい段落が P の元の段落記号を引き継ぐ」とした。しかし、段落記号が持つ `pPr`（特に `sectPr`）がどちらの段落に残るのかを決めていない。ADR-0011 §9.1 の既定（`sectPr` を除いたコピー、またはスタイル ID）と組み合わせると、却下したときにセクション区切りや P の書式が失われる。
  - **N2:** §6.2 は、custom XML と `core.xml` の編集を「ADR-0002 がすでに許している untyped primitive」で行うとしている。しかし、そのような書き込み API は core にない。ADR-0004 の Alternative 9 は untyped `LensMut` を明示的に退けている。したがって、§1 の「PR #38 と PR #45 以外に core API は要らない」は成り立たない。

## round 1 指摘の解消状況

| 指摘 | 状況 | 確認内容 |
|---|---|---|
| **B1** | 解消 | spike を再実行し、すべての表が一致した。コメント表の該当行: `commentEx paraId = last paragraph` 1089 (59)、`no commentEx entry` 1 (1)、`replies` 469 (43)、`depth > 1` 52 (16)、`done` 0/1 が 36、1/0 が 12、名前空間 2010/11 が 54、2012 が 5、`people` 430/73。§3.2 の表、§11 の受け入れ/拒否テスト、Alternatives 8、Consequences の「54 of 59」がこれと合う。`delete_comment` の扱いは n1 を参照。 |
| **M1** | 一部解消 → N1 | `ins` を `CT_ParaRPr` の先頭の子に置くこと、末尾の段落記号を挿入にしないこと、T-REV に末尾追加のケースを入れたことは正しい。コーパスでも、行の変更履歴以外で末尾段落の段落記号に `ins`/`del` がつく例は 0 件である（`del` 7 件と `ins` 1 件は、すべて行の変更履歴の中）。ただし、段落記号の引き継ぎ（`pPr`、`sectPr`）が未定義である（N1）。 |
| **M2** | 解消 | 行削除の 3 ファイル（run 3/3 が `del` の中、段落記号 3/3 が `del`）と一致する。行挿入の証拠は、text run のない 1 行だけである。run を `ins` で包む部分は、コーパスでなく対称性による推定なので、*(u)* と書くとよい（n5）。 |
| **M3** | 解消（範囲は n2） | 既存の `rPrChange`/`pPrChange` は拒否する。`detach_as` で `CT_RPrOriginal`/`CT_PPrBase` に変換し、§10 の `[detach]` も更新した。ただし「9 種」の列挙が ADR-0011 §2 の一覧より狭い（n2）。 |
| **M4** | 解消 | 上書き継承、段落スタイルでの再設定、文字スタイルでのトグル、直接書式は絶対値、表スタイルは *(u)*。OI29500 2.1.258 の引用は原文どおりである。同じ節の周辺に、反映すべき注記がさらに 2 つある（n3）。 |
| **M5** | 解消 | `updateFields` の opt-in と `StaleFieldRisk`。ストアの内訳は再測定と一致した（core 161、extended 82、cover page 25 + 141 = 166、その他 37 + 17 = 54）。 |
| m1 | 解消 | 460 件（start）と 421/461 件（end）の形に沿っている。ただし、親が参照 run だけのコメント（SDK 18 件、参照だけの返信 7 件）の場合が書かれていない（n5）。 |
| m2 | 解消 | スレッド単位の書き込みと、`ParentNotAddressable` の規則。 |
| m3 | 解消 | `delete_comment` の細部、孤立 ID、T-CMT を差分で見ること。 |
| m4 | 解消 | 最高水位と `AnnotationIdsExhausted`。 |
| m5 | 解消 | §2 の Evidence に入った。 |
| m6 | 解消 | 走査範囲と `textId`。 |
| m7 | 一部解消 → n4 | 「宣言されていて、かつ Ignorable」の判定は入った。PR #45 で接頭辞が付け替えられる場合と、`mc` の宣言が必要なことが漏れている。 |
| m8 | 解消 | `RunNotTextOnly`、全子孫の `t` を `delText` に、`instrText` を `delInstrText` に。 |
| m9 | 解消 | ただし「ストアの編集」の手段が存在しない（N2）。 |
| m10 | 解消 | `tblLook` のビットマスク、グリッド列、2 つの oracle、Times New Roman 10 pt。 |
| m11 | 解消 | 注を組で作ること、区切りの ID、§3.5 の参照。`settings` の子要素の allowlist は n6。 |
| §1（PR #38 への書き直し） | おおむね妥当 → n7 | `RegistryPlan::new`、`stage_part(&pkg, name, doc)`、`Package::apply` の使い方は実装と合う。plan に `add_related_part` はない。PR #38 の round 1 指摘への依存も書かれていない（n7）。 |

**PR #38 の round 1 指摘（M3、M4、M7）への依存:**

- M3（読めない `.rels` で `remove_parts` が `Ok`）と M4（`copy_relationships` が plan の view を見ない）: ADR-0015 は `remove_parts` も `copy_relationships` も使わない。影響はない。
- M2（dirty でない `XmlDocument` を stage すると元のバイトが出る）: ADR は「clone を lens で編集して stage する」としか書いていないので、木は必ず dirty になる。値が変わらない編集（`set_text` の no-op）は元のバイトを出すが、結果は同じなので無害である。
- **M7**（新しい `rId` がダングリングな `r:id` を拾う）: ADR-0015 は `document.xml` から `comments`、`footnotes`、`endnotes`、`commentsExtended` への関係を新しく作る。修正前の #38 にこれが乗ると、本文に残ったダングリングな `r:id` が、新しいコメントのパーツに解決されうる。Word のコーパスでは 0 件だが、依存関係として明記すべきである（n7）。

## 指摘

### Major

#### N1. 段落記号を引き継ぐとき、`pPr` と `sectPr` がどちらに残るかが決まっていない（M1 の修正の続き）

- **根拠:**
  - §5.2（L351 付近）: P の段落記号を挿入扱いにし、「The new paragraph takes over P's original mark」とする。
  - 段落の書式（`pPr`。`sectPr`、`numPr`、段落記号の `rPr` を含む）は、段落記号に属する。却下すると、P の挿入された段落記号が消えて P と N が結合し、結合した段落は **N の段落記号**（つまり N の `pPr`）を持つ。
  - 一方、ADR-0015 は N の `pPr` を指定していない。`append_paragraph("text")` は ADR-0011 §9.1 の `insert_paragraph` に従うと読めるが、§9.1 の `pPr` は「隣の `pPr` のコピーから **`sectPr`** と `pPrChange` を除いたもの」か「スタイル ID」である。
    - **P が `sectPr` を持つ場合**（セクション区切りの段落。ADR-0011 によれば POI で 88/123、SDK で 480/613 の `sectPr` が段落にある）: N はそれを持たず、P がそのまま持ち続ける。却下すると、結合した段落は N の段落記号を使うので、**セクション区切りが消える**。承認した場合も、区切りの位置が Word の Enter と異なる（Word では区切りは N の末尾に残る）。
    - **スタイル ID を指定した場合:** 却下すると、P の段落スタイルが N のものに変わる。
  - T-REV の「rejected view = old text」は文字列しか比べないので、どちらも検出できない。
- **推奨:**
  1. 変更履歴の中で P の後ろに段落を挿入するときは、次のようにすると §5.2 に明記する。
     - **N が P の元の `pPr` を verbatim に受け取る**（`sectPr`、`numPr`、段落記号の `rPr` を含む。`sectPr` の移動は、ADR-0011 が A2 を外したのと同じ操作になる）。
     - **P は、そのコピーから `sectPr` と `pPrChange` を除き、`rPr` の先頭に `ins` を足したもの**になる。
     - 却下すれば元の P に戻り、承認すれば Word の Enter と同じ形になる。
  2. N に別の書式（スタイル ID など）を求める呼び出しは、セッションの中では拒否する。または、N に `pPrChange`（元の値は P の `pPr`）を書く。後者は Word の挙動が *(u)* なので、Phase3 では拒否を推奨する。
  3. T-REV に、`sectPr` を持つ段落の後ろへの追加と、`numPr` を持つ段落の後ろへの追加を加える。比較は文字列だけでなく、「却下ビューの `pPr` 列が入力と一致すること」とする。
  4. 「先頭への挿入、または表の直後」の場合（N 自身の段落記号を挿入にする）について、「it is then not the container's last paragraph」は Word が書いたファイルでは成り立つ。しかし本文が `w:tbl` の直後に `w:sectPr` で終わる他ツールのファイルでは成り立たない。その場合は拒否すると明記する。

#### N2. custom XML と `core.xml` のストア編集に使う primitive が core に存在しない

- **根拠:**
  - §6.2（L460 付近）: 「a Layer 2 replacement of one element's text through the untyped primitive ADR-0002 already allows for registry-like parts」。
  - 実際に core が公開している書き込みは、**型付きの `LensMut<E: ElementSpec>`** だけである（`set_attr`、`set_text`、`insert_*`）。`XmlDocument::root_mut` は `pub(crate)` である（`xml/mod.rs:367`）。
    - `set_attr` は QName では untyped だが、`E::ATTRS_INCOMPLETE` を見るし、要素へのたどり着き方は型付きである。
    - `set_text` は `E::CONTENT` が `Content::Text` であることを要求する。
  - ADR-0002 に「registry-like parts」の記述はない（`untyped` が出てくるのは `set_attr` の 2 か所だけ）。逆に、ADR-0004 の Alternative 9 は「Generic untyped `LensMut` descent in core」を**採らない**と決めている。
  - custom XML の中身のスキーマは任意なので、`ElementSpec` を生成できない。`docProps/core.xml` の `cp`/`dc` 要素が生成器の入力にあるかどうかも、確認されていない（`generated/parts.rs` にあるのはパーツの型だけである）。
  - したがって §1 の「No core API beyond PR #38 and PR #45 is needed」と、Q1 の提案される答えは、§6.2 については成り立たない。
- **推奨（どちらかを選んで §1、§6.2 と Q1 に書く）:**
  - (a) **core に小さな API を足す。** 例として、`XmlDocument::replace_leaf_text(path: &[(ns, local, index)], value)` がある。名前空間 URI と位置で 1 要素を特定し、子要素がなければ文字だけを置き換え、その要素だけを dirty にする。ADR-0004 Alternative 9 を一部見直すことになるので、その理由（スキーマのない store）と、置換だけで構造は変えないという範囲を書く。core の PR を前提条件に加える。
  - (b) **バイトの差し替え。** 元のバイトから対象要素の文字列範囲を特定してつなぎ直し、`stage_part(Bytes)` で丸ごと置き換える（`Replaced`）。core の変更はいらないが、範囲の特定（エンティティ、CDATA、名前空間の解決）を docx 側で正しく書く必要があり、T2 の局所性は自前で保証することになる。
  - どちらにしても、T-SDT の「the store part changed only at the target element」を、その primitive の受け入れテストにする。

### Minor

- **n1. `delete_comment` とプレリリース名前空間:**
  - §3.2 の表は、`reply` と `set_resolved` だけを `LegacyCommentsExtended` にしている。
  - SDK の 54 ファイルでは、ほぼすべてのコメントが 2010/11 名前空間の `commentEx` を持つ（`Comment010` も同じ）。`delete_comment` がその項目を消さなければ、`paraId` が何も指さない `commentEx` が残る。T-CMT の「新しく壊さない」に反する。
  - 消すには、プレリリース名前空間の `w15:commentEx` を allowlist に入れる（生成器の入力に入っていない可能性が高い。Q9 を参照）。
  - 推奨: `delete_comment` も、プレリリース名前空間のパーツがあるときは `LegacyCommentsExtended` で拒否し、§3.2 の表に列を足す。

- **n2. 「9 種の変更履歴」が ADR-0011 §2 より狭い:**
  - §2（L183）と §5.2（L366）は「nine revision kinds」として、`ins`、`del`、`moveFrom`、`moveTo`、`rPrChange`、`pPrChange`、`trPr/ins|del`、`numberingChange`、`sectPrChange` を挙げている。
  - ADR-0011 rev 2 の §2 は、ほかに `tblPrChange`、`trPrChange`、`tcPrChange`、`tblGridChange`、`cellIns`/`cellDel`/`cellMerge`、`move*Range*`、`customXml*Range*` を注釈 ID の対象にしている。`tblPrExChange` もある。
  - コーパスには表の変更履歴は 0 件だった（`p1_revkinds.py`）。しかし Word は、変更履歴をオンにして表の書式を変えるとこれらを書く。R2 を拡張した目的（スナップショットを上書きしない）からすると、ADR-0011 の行・セル編集がこれらを素通りしてはいけない。
  - 推奨: §2 は ADR-0011 rev 2 の一覧を参照するように直し、「nine」をやめる。R2 の拡張対象もその一覧に揃える。
  - あわせて、R2 の拡張は Accepted の ADR-0004 の挙動を変える（Phase0 の `set_text` で、`rPrChange` を持つ run が新たに拒否される）。ADR-0004 に post-acceptance note を付けると明記する。「paragraph carrying `pPrChange`」の中の run を編集できるかどうかも書く。

- **n3. MS-OI29500 の周辺の注記:**
  - **2.1.229**（Part 1 §17.7.2, Style Hierarchy）には、「Word applies the properties from a paragraph style … **before** it applies the properties from a numbering style」とある。つまり**番号付けが段落スタイルを上書きする**。
    - §8.1 は、番号付け（3）を段落スタイル（4）より前に置き、*(u)* としている。これは順序が逆で、しかも仕様に書かれている事柄である。
    - 推奨: 3 と 4 を入れ替えて 2.1.229 を引用する。*(u)* は外す。
  - **2.1.230**（§17.7.3, Toggle Properties）:
    - (a) `docDefaults` が true のとき、実効値は「false になっている階層の数が偶数のとき、そのときに限り true」になる。
    - (b) 値のない階層は、既定値ではなく `docDefaults` の値になる。
    - (c) Word 2016 より前の版は、表スタイルのトグルプロパティを正しく表示しない。
    - §8.2 の規則は `docDefaults` で `b=1` の場合を扱っていない。推奨: 2.1.230 を引用し、T-FMT に「`docDefaults` が `b=1`」の合成ケースを加える。表スタイルの *(u)* には (c) を根拠として書き添える。

- **n4. PR #45 で付け替えられる接頭辞と `mc:Ignorable`（m7 の続き）:**
  - `NsDecl` は、同じ URI がスコープ内で接頭辞付きで束縛されていれば、**その接頭辞を使う**。希望した接頭辞が別の URI に束縛されていれば、`w141`、`w152` のように付け替える（`lens.rs:420-460`）。
    - たとえば `sdk-Comment010` のルートでは `w15` が 2010/11 名前空間に束縛されている。
    - 判定や書き込みを「`w14` という文字列が Ignorable にあるか」で行うと誤る。
  - 新しく作る要素に `mc:Ignorable` を付けるには、`set_attr` のために `mc` 自体もスコープ内に必要である（`NamespaceNotInScope`）。
  - `detach_as` は、祖先の Ignorable に含まれる名前空間の要素を含む部分木を拒否する（`NotDetachable`）。そのため、`w14` の効果（`w14:ligatures` など）を持つ run には `rPrChange` を作れない。コーパスでは 10 run、4 ファイルとまれである（`p3_rpr_ext.py`）。
  - 推奨:
    - 判定は「URI → スコープ内の接頭辞 → その接頭辞が Ignorable にあるか」で行う。
    - 作成する要素には `NsDecl` で `mc` も宣言し、Ignorable には**実際に選ばれた接頭辞**を書く。
    - `rPrChange` が拒否される場合（`NotDetachable`）を §5.2 に書く。

- **n5. コメントの細部:**
  - **日付:** `poi-testComment` は、`w:date="2021-05-20T10:57:00Z"` に対して `w16cex:dateUtc="2021-05-20T02:57:00Z"` を持つ。Word は `w:date` に**ローカル時刻を `Z` 付きで**書き、真の UTC は `dateUtc` に書く。
    - ADR はどちらも UTC で書く。Word は `w:date` をローカル時刻として表示するので、時刻がずれる。
    - 推奨: `w:date` はローカルの壁時計（呼び出し側の指定またはローカル時刻）、`dateUtc` は UTC とする *(u)*。変更履歴の `w:date` も同じ扱いにするかを決める。
  - **参照 run だけの親:** 親が参照 run だけのコメント（SDK 18 件）に返信するときの anchor の規則がない。推奨: 返信も参照 run だけにして、親の参照 run の後ろに置く（コーパスには参照だけの返信が 7 件ある）。
  - **作成する `commentsExtended`:** 関係の型（`http://schemas.microsoft.com/office/2011/relationships/commentsExtended`）と content type（`application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml`）を書く。`poi-testComment` がこの形である。
  - **M2 の行挿入:** run を `ins` で包むことは、コーパスでは確かめられていない（行挿入は text run のない 1 行だけ）。*(u)* と書く。
  - **M2 の承認ビュー:** round 1 は「`trPr/del` の行を飛ばすかどうか」も求めていたが、ADR は文字列についてしか書いていない。ADR-0007 の表の読み取りで、削除された行を行として返すかどうかを書く。

- **n6. allowlist の漏れ:**
  - `settings.xml` の `w:footnotePr`/`w:endnotePr` には、子要素 `w:footnote w:id`/`w:endnote w:id`（`CT_FtnEdnSepRef`）が要る。§10 には親（`CT_FtnDocProps/w:footnotePr`）だけがあり、`CT_EdnDocProps/w:endnotePr` もない。
  - 段落記号の `del`（`w:CT_TrackChange/w:del`）、`w:CT_FtnEdn/w:endnote`、`w:CT_FtnEdnRef/w:endnoteReference` は「counterparts」と書くのではなく、行として挙げる。

- **n7. §1 と PR #38:**
  - `RegistryPlan` に `add_related_part` はない（`Package::add_related_part` だけ）。plan では `add_part` と `add_relationship` を組み合わせる、と直す。
  - plan を作ってから `apply` するまでの間は、`Package::xml_part`/`parsed_part` だけを使い、`xml_part_mut` は使わないと書く。`xml_part_mut` は `touch()` するので、plan が `StalePlan` になる。
  - PR #38 round 1 の **M7**（ダングリングな `r:id`）と **M2**（dirty でない木の stage）の修正を、前提条件として書く。
  - Q1 の答えを、N2 の結果に合わせて直す。

- **n8. 編集上の細部:**
  - Consequences に「Positive:」が 2 回ある。
  - T-CMT の「every `paraIdParent` names a comment that is not itself a reply」は、作成したものだけに当てはまると書く。入力には 52 件の入れ子がある。
  - `core.xml` は固定のパス名ではなく、パッケージの core-properties 関係でたどると書く。

## 判定

**Minor だけを直せば Accepted にできるか: no。**

- B1 と M2〜M5 は解消し、spike の数値は完全に再現した。
- 新しい Major が 2 件ある。
  - **N1:** 段落記号を引き継ぐときに `pPr`/`sectPr` がどちらに残るか。却下するとセクション区切りが消える。
  - **N2:** ストア編集の primitive が存在せず、§1 の「core API は追加不要」が成り立たない。
- どちらも設計の範囲は小さい。N1 は §5.2 に規則を 1 つ足し、T-REV を拡張する。N2 は core API の追加かバイトの差し替えのどちらかを選ぶ。直したうえで round 3 に進めば、収束が見込める。

## Sources

- [MS-OI29500] 2.1.258, Part 1 §17.7.8, Paragraph Styles: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/bd62641d-7786-473f-872d-4b3205e91a82
- [MS-OI29500] 2.1.230, Part 1 §17.7.3, Toggle Properties: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/f7130225-2368-48f3-acae-a9d278d0fb25
- [MS-OI29500] 2.1.229, Part 1 §17.7.2, Style Hierarchy: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/f20aa1bd-46a8-4833-bf1c-ee47082e12ba
- 節番号は、[MS-OI29500] の目次（`toc.json`）で確かめた。
