# ADR-0015 レビュー（Opus, round 3）

- 対象: PR #39 `docs/adr/0015-docx-phase3-review-features.md`（Proposed, revision 3）
- 対象コミット: `1fb4201`（worktree `plan-docx-phase3`）
- 照合したもの:
  - round 2 のレビュー `0015-opus-review-round2.md`（N1、N2、n1〜n8）
  - `main`（`48aa8f0`）: ADR-0007 rev 3（Accepted）、ADR-0012（Accepted）、`codegen/allowlist/wml.toml`、`ooxml-core` の `xml/{mod,write,parse}.rs` と `lens.rs`（`set_text`、`detach_as`、`DetachRefusal`）。PR #14、#18、#26 はマージ済み。
  - ADR-0011（PR #31、`05a2789`。ファイル上は Accepted rev 4 だが、`main` には未マージ）の §2、§6、§9.1、§11
  - PR #38（`252dc8e`。round 1 の修正 `958c8ac` を含む）と PR #45（`2a76520`）
  - [MS-OI29500] 2.1.230 の原文（Microsoft Learn）
- 追加の測定（`scratchpad/review-pr39-r3/`）:
  - `probes/q1_ppr_takeover.py`: P の `pPr` をそのまま detach コピーすると何に当たるか。結果は `q1.txt`。
  - `probes/q2_bind_targets.py`: data binding の対象ノードの形。結果は `q2.txt`。
  - `probes/q3_xpaths.py`: サブセット外の xpath と、`core.xml` の空要素の書き方。結果は `q3.txt`。
  - どれも `python3 -I` で実行し、DTD を含むファイルは飛ばし、エンティティは解決しない。コーパスは `.docx`、`.docm`、`.dotx` を対象にした（SDK 442 ファイル）。

## 結論（先に）

**N1 と N2 は設計としては解消した。新しい Major はない。残りは Minor だけなので、それを直せば Accepted にできる（判定: yes）。**

- **N1 は解消した。** 「N が P の元の `pPr` を受け取り、P は `sectPr` と `pPrChange` を除いたコピーに `ins` を足したものになる」という規則と、`TrackedParagraphFormatting`、`NoFollowingParagraph`、T-REV の `pPr` 列の比較が入った。これで却下したときに `sectPr` や `numPr` が戻る。
  - ただし**仕組みの書き方**に穴が 3 つある（r1〜r3）。
    - 「a detached copy, PR #18」は、ヘッダーやフッターを参照する `sectPr` では `detach_as` に拒否される（`RelationshipAttribute`）。SDK では段落の `sectPr` 174 個のうち 38 個（20 ファイル）、テンプレートでは 37 個のうち 15 個がこれに当たる。
    - P の段落記号の `rPr` に `ins` を書くための allowlist 行（`w:CT_ParaRPr/w:rPr` の `[write]`）がない。
    - 「却下すると N の段落記号が残る」という Word の挙動は *(u)* なのに、断定している。T-REV の却下ビューは自前のモデルなので、それだけでは検証にならない。
  - どれも、ADR-0011 §6 の手順を参照する、行を足す、*(u)* と manual check を足す、といった小さな修正で直る。
- **N2 は解消した。コーパスに対しても API は十分である。**
  - `core.xml` の binding 161/161 件と、サブセットに入る custom XML の binding 42/42 件は、対象の要素が存在し、子要素を持たない。そのうち 105 件と 25 件は空要素である。
  - xsi:nil、CDATA、エンティティ参照、コメント、UTF-16 のストアは 0 件だった。サブセットに入らない 20 件は、すべて `//Name` 形式で、`UnsupportedBinding` になる。
  - ただし、契約が「スキーマのないパーツ」に**限定されていない**。そのままだと WML などの型付きパーツでも呼べるので、ADR-0002 の allowlist を迂回する untyped な書き込みになりうる（r4）。「it is not a schema bypass」という説明は、型付きパーツについては成り立たない。名前空間による拒否を 1 つ足せば閉じる。
- **n1〜n8** は、n2 の `tblPrExChange`（r6）と n3 の規則の合成（r7）を除いて解消した。

## round 2 指摘の解消状況

| 指摘 | 状況 | 確認内容 |
|---|---|---|
| **N1** | 解消（細部は r1〜r3、r5） | §5.2 に「Which mark properties go where」が入った。N は P の元の `pPr`（`sectPr`、`numPr`、段落記号の `rPr` を含む）を受け取る。P はそのコピーから `sectPr` と `pPrChange` を除き、`ins` を先頭に足したものになる。他の書式を求めると `TrackedParagraphFormatting`。先頭への挿入や表の直後の場合は `NoFollowingParagraph`。T-REV は `sectPr`/`numPr` の段落の後ろへの追加で `pPr` 列を比べる（§11）。`numPr` は Word の Enter と同じで、リストが続く。 |
| **N2** | 解消（契約の範囲は r4） | `XmlDocument::set_leaf_text` と `check_set_leaf_text`、`LeafStep { ns, local, index }`、`NoSuchElement`/`NotLeaf`/`InvalidXmlChar`。(b) のバイトの差し替えを退けた理由も妥当。§1 の前提条件、Q1、Consequences も直っている。`root_mut` が `pub(crate)` であること（PR #38 の `xml/mod.rs:367`）と、生成器の入力に `cp`/`dc` の要素がないこと（`main` では `generated/parts.rs` にパーツの型があるだけ）を確認した。 |
| n1 | 解消 | §3.2 の表に `delete_comment` の列が入り、T-CMT の拒否テストにも入った。 |
| n2 | 一部 → r6 | ADR-0011 §2 の一覧を参照し、ADR-0004 の post-acceptance note も書かれた。ただし `tblPrExChange` は ADR-0011 §2 の一覧（`05a2789` の L321）に**ない**。それなのに「That list covers … `tblPrExChange`」と書いている。 |
| n3 | 一部 → r7 | 2.1.229 による順序の入れ替えと、2.1.230 (a)(b)(c) の引用は原文どおりである。ただし、規則 6（偶数規則）と規則 2・3（リセット、トグル）のどちらが優先するかが書かれていない。 |
| n4 | 解消 | URI → 接頭辞 → Ignorable の順で判定し、`mc` も宣言する。`NotDetachable` も書かれた。PR #45 の `NsDecl` と `*_with_ns` は `2a76520` にある（`lens.rs:206, 547-652`）。 |
| n5 | 解消（型は r8） | ローカル時刻に `Z` を付ける規則、参照 run だけの親、`commentsExtended` の関係の型と content type、行挿入の *(u)*、削除された行の読み取りビュー。`NewComment::date` の型が本文と合っていない（r8）。 |
| n6 | 解消 | `CT_EdnDocProps/w:endnotePr`、`CT_FtnEdnSepRef`、`CT_TrackChange/w:ins|del`、`endnote`/`endnoteReference` が行として入った。N1 に必要な行は r2。 |
| n7 | 解消 | `RegistryPlan` に `add_related_part` がないこと（PR #38 では `Package::add_related_part` L1478 だけ）を確認した。`xml_part_mut` が `touch()` すること（`package.rs:356-359`）も確認した。PR #38 の M2 と M7 は `958c8ac` で修正済みである（「new rIds avoid rId values of the source's attributes」）。前提条件としては満たされている。 |
| n8 | 解消（Consequences は r8） | 「Positive:」は 1 回になった。T-CMT の「created」と、関係から `core.xml` をたどることも入った。 |

## 指摘

Blocker と Major はない。以下はすべて Minor である。

### r1. N の `pPr` は `detach_as` で verbatim にはコピーできない（`sectPr` の参照）

- **根拠:**
  - §5.2（L428）は「N's `pPr` is P's original `pPr`, verbatim (a detached copy, PR #18)」としている。
  - `main` の `detach_as` は、`r:` 属性を持つ部分木を `NotDetachable { RelationshipAttribute }` で拒否する（`error.rs:251-254`）。`sectPr` の `w:headerReference`/`w:footerReference` は `r:id` を持つ。
  - `q1.txt` によると、段落の `sectPr` で `r:` 属性を持つものは次のとおり。

    | コーパス | 段落の `sectPr` | `r:` 属性あり |
    |---|---|---|
    | SDK | 174 | 38（20 ファイル） |
    | テンプレート | 37 | 15（13 ファイル） |
    | POI | 16 | 2 |
    | docx4j | 4 | 2 |

    N1 がまさに対象にしている、セクション区切りの段落の 2〜4 割が拒否される。
  - 拒否は fail-closed なので壊れたファイルは出ない。しかし、仕組みの書き方としては誤っている。
  - ADR-0011 §6 はすでに手順を持っている。参照を飛ばして `sectPr` をコピーし、各 `r:id` を関係で確かめてから、型付きの setter で入れ直す。A2 を外したことによる移動も、その手順で行う（「Moving is copy + remove … by the same copy procedure」）。
  - ADR-0015 は「same operation as ADR-0011's lifting of A2 (§9.1 there)」と書いているが、A2 を外しているのは ADR-0011 の **§6** である。§9.1 ではない。
  - 段落記号の `rPr` に Ignorable な名前空間の要素があると `IgnorableNamespace` になる（POI 12、SDK 7）。§5.2 は `rPrChange` についてしか書いていない。
- **推奨:**
  - N の `pPr` は「`sectPr` を除いて detach したコピーに、ADR-0011 §6 の手順で `sectPr` を移したもの」と書き、参照を移すのは移動であって複製ではないと明記する。または、PR #44（same-part move）がマージされたら、元の `pPr` を N に**移動**し、P 用の `pPr` を `sectPr` を除いたコピーで作る。
  - 参照を ADR-0011 §6 に直す。
  - N1 の場合も `NotDetachable` で拒否されることを §5.2 に書く。

### r2. P の段落記号に `ins` を書くための allowlist の行がない

- **根拠:**
  - `ins` は P の `w:pPr/w:rPr`（`CT_ParaRPr`）の先頭に挿入する。しかし ADR-0007 §8 では、`w:CT_ParaRPr/w:rPr` は `[read]` と `[detach]` だけである（`0007` の L954）。
  - P に `pPr` がない場合（POI 5,427 段落、SDK 3,784 段落）は、`w:CT_PPr/w:pPr`（ADR-0007 で `[write]` に移されている）と、`CT_ParaRPr` の `rPr` を `get_or_insert` する必要がある。
  - §10 は `w:CT_TrackChange/w:ins` だけを挙げている。
- **推奨:**
  - §10 に「`w:CT_ParaRPr/w:rPr` を `[read]` から `[write]` へ」と書く。
  - `[detach]` に、ADR-0011 の `w:CT_PPr/w:pPr` と `w:CT_SectPr/w:sectPr` を使うことを書く。
  - `pPr` のない P の場合（`pPr` と `rPr` を作る）を §5.2 に 1 行で書く。

### r3. 「却下すると N の段落記号が残る」は *(u)* で、T-REV の却下ビューでは確かめられない

- **根拠:**
  - §5.2（L418、L434）は、却下すると P と N が結合し、**N の段落記号**の `pPr` が残ると断定している。
  - ECMA-376 は、挿入された段落記号を削除したときに、結合した段落がどちらの `pPr` を持つかを規定していない。Word の UI での段落記号の削除も、版によって挙動が違うと言われている。
  - T-REV の「rejected view」は ooxml-rs のテストが持つモデルである。そのモデル自体が「N の記号が残る」としているなら、比較は同語反復になる。
- **推奨:**
  - この規則に *(u)* を付ける。
  - §11 の manual check に「`sectPr` の段落と見出しの段落の後ろに追加し、Word で Reject All したときの `pPr`」を加える。
  - T-REV の却下ビューは「段落記号の `ins` を消し、後ろの段落の `pPr` を残す」と定義して書く。

### r4. `set_leaf_text` の契約が「スキーマのないパーツ」に限定されていない

- **根拠:**
  - §6.2（L546-554）は、この API を `XmlDocument` の公開メソッドにしている。`XmlDocument` は自分がどのパーツか、どのスキーマかを知らない。
  - そのため、`document.xml` でも呼べる。たとえば、`w:pPr` や `<w:b/>` のように子要素のない WML 要素に文字列を入れられる。前者は要素だけを持つ型で、後者は `Content::Empty` である。どちらもスキーマに反するうえ、allowlist の `[read]`/`[write]` の区別も通らない。
  - したがって、「it is not a schema bypass: it is the text half of `set_text` without the `ElementSpec`」は、型付きパーツについては成り立たない。`set_text` が `E::CONTENT == Text` を要求している（`lens.rs:942-945`）のは、まさにその検査である。
  - 結果として、ADR-0004 の Alternative 9 が避けた「untyped な書き込み」が、名前と範囲を変えて残る。
- **推奨（1 つ選ぶ）:**
  - core に、生成されるスキーマの名前空間（WML、SML、PML、DML、VML、MC と、MS 拡張の `w14` など）の静的な一覧を置く（`name.rs` の `RELATIONSHIP_ATTRIBUTES` と同じ場所）。ルートか経路上のいずれかの要素がその名前空間にあれば、`TypedNamespace` で拒否する。core PR の受け入れテストに、`document.xml` で拒否されることを加える。
  - または、パーツの content type が `application/xml` または core-properties のときにだけ得られる `SchemalessPart` のようなラッパーを通して公開する。
  - どちらにしても、ADR-0004 Alternative 9 を見直す範囲を「スキーマのないパーツのテキスト置換」と、文章だけでなく**コードで**閉じると書く。
- **あわせて書くとよい細部:**
  - **dirty の付け方:** `set_text` と同じにする。`subtree_dirty` を付け、`own_dirty` は元が自己終了タグのときだけ付ける。こうすれば、元の開始タグが verbatim に残る（T2）。`core.xml` で自己終了タグの空要素は 3 個、開始・終了タグの空要素は 1,348 個ある（`q3.txt`）。
  - **`xsi:nil="true"` の要素:** 文字を入れると XSD のインスタンスとして不正になる。属性は変えない API なので、`NilElement` で拒否する。コーパスには 0 件である。
  - **ストアのパース失敗:** UTF-16 のストアは `UnsupportedEncoding` に、DTD は `DtdNotAllowed` になる。これらを `UnsupportedBinding` に写すと書く。

### r5. N1 の規則の対象外を明記する

- **P が変更履歴を持つ場合:**
  - 「P keeps a copy without … `pPrChange`」と読むと、P の既存の `pPrChange` を黙って捨て、N にはその `w:id` ごとコピーするように読める。
  - 実際には、拡張された R2（L440-450）が拒否するはずである。
  - コーパスには `pPrChange` が SDK 2 件、段落記号の `rPr` にある `ins`/`del`/`move*`/`rPrChange` が POI 22 件、SDK 48 件ある。
  - 「P が `pPrChange`、段落記号の変更履歴、`sectPrChange` を持つときは `InsideRevision`」と §5.2 に書き、「without `pPrChange`」は削る。
- **「a following paragraph」の意味:** 「N の**直後の兄弟**が `w:p` であること」と書く。表、表の並び、`w:sdt` のブロックが続く場合は拒否する。
- **N が先頭に来る場合の N の `pPr`:** 「gets no properties from P」のあと、N がどんな `pPr` を持つかが書かれていない。段落記号の `ins` だけを持つ、と書く。
- **連続して挿入する場合:** 先頭に 2 段落を挿入しようとすると、2 回目は「自分の以前の変更履歴」として拒否される。API の文書に書く。
- **見出しの `w:next`:** 見出しの後ろに追加すると、N も見出しになる（`w:next` が別のスタイルを指す段落スタイルを持つ段落が SDK 2,633、POI 66）。Word の Enter は `next` のスタイルを使う。「Accepting gives Word's Enter shape」は、`w:next` が自分自身を指すスタイルの場合に限ると書き、制限として Consequences に入れる。

### r6. `tblPrExChange` が ADR-0011 §2 の一覧にない

- **根拠:** §2 の registry と §5.2 は、ADR-0011 rev 2 §2 の一覧に従うとしている。しかし ADR-0011 の一覧（rev 4 でも同じ、L321）には `tblPrExChange` が入っていない。`CT_TblPrExChange` は `w:id` を持つ。
- **推奨:** 「ADR-0011 §2 の一覧に `tblPrExChange` を加えたもの」と書き、注釈 ID の union にも含める。ADR-0011 は rev 4 で Accepted なので、参照も「rev 2」から直す。

### r7. トグルの規則 6 と規則 2・3 の合成

- **根拠:**
  - 2.1.230 (a) は、`docDefaults` が true で、プロパティが複数の階層に現れるとき、false の階層の数が偶数なら true とする。
  - 一方、規則 2・3 では、段落スタイルが false にリセットし、文字スタイルが false で変えない。この場合は false になる。
  - しかし 2 つの false を数える偶数規則では true になる。どちらが優先するかが書かれていない。
- **推奨:** 「`docDefaults` が true のときは規則 6 だけ、そうでなければ規則 1〜5」のように合成を明記する *(u)*。T-FMT の合成ケースに、この組み合わせ（`docDefaults` が `b=1`、段落スタイルが `b=0`、文字スタイルが `b=0`）を入れる。

### r8. 編集上の細部と状態の更新

- §3.1 の `NewComment { date: Option<UtcDateTime> }` は、§3.2 の「local wall-clock time and its UTC offset」と合わない。オフセット付きの型にする。`track_changes(author, date)` の `date` も同じである。
- Consequences:
  - 「Negative:」が 3 回ある。
  - 「Delete, as well as reply and resolve, is refused …」と「Replies and resolve are refused …」が重複している。1 項目にまとめる。
- 状態の更新:
  - ADR-0007 は `main` で Accepted（rev 3）になった。Depends on と Sources に、`0007-docx-phase1-template-filling.md` へのリンクと状態を書く。
  - ADR-0011 は PR #31 で Accepted rev 4 だが未マージである。マージされたらリンクする。
  - PR #38 の M2 と M7 は `958c8ac` で修正済みと書ける。
  - PR #39 のタイトルが「rev 2」のままである。

## 判定

**Minor だけを直せば Accepted にできるか: yes。**

- round 2 の Major 2 件は、どちらも設計として解消した。
  - **N1:** `pPr` の受け渡しの規則、拒否、`pPr` 列の T-REV。
  - **N2:** 範囲の狭い core API。`core.xml` の 161 件とサブセットに入る custom XML の 42 件で、対象の要素が存在し子要素を持たない。API はコーパスに対して十分である。
- 残りはすべて Minor である。
  - r1〜r3 と r5 は N1 の仕組みの書き方: ADR-0011 §6 の `sectPr` の手順を参照する、allowlist の行、*(u)* と manual check、対象外の明記。
  - r4 は `set_leaf_text` の範囲をコードで閉じる 1 文と、そのテスト。
  - r6〜r8 は一覧、規則の合成、編集上の細部。
- どれも、本文に数行を足すか直すだけで済み、設計の形は変わらない。修正したうえで Accepted にしてよい。r4 は core PR のレビューでも確認する。
