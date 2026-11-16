# ADR-0015 レビュー（Opus, round 1）

- 対象: PR #39 `docs/adr/0015-docx-phase3-review-features.md`（Proposed, revision 1）と `spikes/docx-phase3/`
- 対象コミット: `c8a9d86`（worktree `plan-docx-phase3`）
- 参照: ADR-0001/0002（post-acceptance notes を含む）、ADR-0004（Accepted）、ADR-0007 rev 2、ADR-0011 rev 2、`core/add-part`（`crates/ooxml-core/README.md` の `RegistryPlan` / `stage_part`）
- 追加の測定: `scratchpad/review-pr39/probe1.py`〜`probe7.py`。spike と同じ方法で、パースだけを行い、エンティティは解決しない。対象は fx / poi / sdk / d4j / tpl。

## 結論（先に）

**このままでは Accepted にできない（判定: no）。**

全体の骨格は妥当である。多パーツのトランザクションを前提にしていること、注釈 ID を和集合から max + 1 で取ること、`paraId` を乱数で `0x80000000` 未満にすること、注 ID を種類ごとに分けること、フィールドを閉じた enum から complex 形式で作ること、`cnfStyle` を読まずに計算すること。いずれもコーパスと仕様に合っている。

ただし、次の問題がある。

- **B1:** コメント節の根拠になっている spike の数値が、名前空間の取り違えで誤っている。
  - `commentsExtended` と `people.xml` を、Office 2013 プレリリース版の名前空間（`http://schemas.microsoft.com/office/word/2010/11/wordml`）で書いているファイルを数え落としている。
  - その結果、「`commentsExtended` は部分的にしかない」「返信はまれ」「入れ子の返信はない」「`people` の author は一致しない」という結論が、どれも逆になっている。
  - 受け入れテストの対象 `sdk-Comment010` 自体がこの名前空間を使っている。設計どおりに書くと、5 パーツの整合が壊れる。
- **Major 5 件:**
  - 変更履歴の構造が 2 か所で Word と違う（末尾段落の段落記号、行の挿入・削除）。
  - `rPrChange` のスナップショットを上書きしてしまう。
  - トグルプロパティの規則が MS-OI29500 と食い違う。
  - `core.xml` へのバインディング書き込みが、ADR-0004 の W2 の安全分類を崩す。

## 指摘

### Blocker

#### B1. `w15` プレリリース名前空間の見落とし: コメントの根拠が逆転し、受け入れ fixture で整合が壊れる

- **根拠:**
  - spike の `review.py:41` は `W15 = …/2012/wordml` しか見ていない。
  - sdk で `commentsExtended` を持つ 59 ファイルのうち **54 ファイル**、`people.xml` を持つ **25 ファイルすべて**は、ルートが `http://schemas.microsoft.com/office/word/2010/11/wordml` である（`probe2.py`）。
  - fx の `sdk-Comment010.docx` も同じである（`xmlns:w15="…/2010/11/wordml"`、`mc:Ignorable="w14 w15 wp14"`）。
  - 2012 版を使っているのは `poi-testComment.docx` だけである。
  - 両方の名前空間を数え直すと、次のようになる（sdk）。

    | 項目 | ADR / spike | 再測定 |
    |---|---|---|
    | `commentEx` のないコメント | 1,081（L62, L149） | **1,090 件中 1 件** |
    | `commentEx` が最終段落を指す | 9 件中 9 件 | 1,090 件中 1,089 件（残る 1 件は孤立した `Comment040`） |
    | 返信 | 3 件（L195「コーパスにない」の根拠） | **469 件** |
    | 返信への返信 | なし | **52 件**（`probe3.py`） |
    | `done` | 親だけ設定すればよい（L198） | コメントごとに独立。親 0・返信 1 が 36 件、親 1・返信 0 が 12 件 |
    | `people` の author 一致 | 一致しない（L66） | **503 件中 430 件が完全一致** |

  - `sdk-Comment010` に対して ADR のとおり 2012 名前空間の `w15:commentEx` を追記すると、2010/11 名前空間の `w15:commentsEx` の下に別名前空間の子要素が混ざる。どちらの名前空間を理解する consumer から見ても、その返信は存在しない。T-CMT（L505「5 パーツが一致」）は、この fixture では原理的に満たせない。
- **推奨:**
  1. spike の名前空間を `{2012, 2010/11}` の両方にして再実行し、Context（L60–66）、§3.1、§3.3 と README を書き直す。
  2. 既存の `commentsExtended` と `people` のルート名前空間を判定する。2012 以外のパーツには追記しない。
     - その場合 `reply` と `set_resolved` は `LegacyCommentsExtended` で拒否する。
     - `add_comment` は `commentEx` を書かずに進める。読み取り側は欠落を許すので壊れない。
     - プレリリース名前空間に合わせて書く案は、Word 2013 RTM 以降がそれを読むかどうかが *(u)* なので採らない。
  3. 受け入れ fixture を 2012 名前空間のファイル（`poi-testComment` など）に替える。`sdk-Comment010` は「レガシー名前空間では拒否する」ことのテストにする。roadmap の L374 も直す。
  4. `NestedReply` の拒否は維持してよい。ただし理由は「コーパスにない」ではなく「Phase3 では作らない」とし、読み取りは深さ 2 以上も扱うと明記する。
  5. 他の spike 項目にも、同じ取り違えがないか確認する（`w15:appearance`、`repeatingSection`、`w15:dataBinding`）。

### Major

#### M1. 末尾段落を追加するとき、段落記号の挿入マークが Word と逆の段落につく

- **根拠:**
  - L270 は、新しい段落自身の `pPr/rPr/ins` を書くとしている。
  - しかし Word では、段落 P の後ろに Enter で段落 N を作ると、**P の段落記号**が挿入扱いになり、元の段落記号は N に移る。
  - コーパスでは、段落記号の `ins`/`del` が容器（body、セル、注、コメント）の**最後の段落**についている例は、行単位の変更履歴の中を除いて 0 件だった。sdk では、末尾段落の run がすべて挿入されていて自身の段落記号は挿入でなく、直前の段落記号が `ins` になっている形が見つかった（`probe4.py`）。
  - `append_paragraph` で最後の段落の段落記号を `ins` にすると、却下したときに文書やセルの最終段落記号を消そうとすることになり、空の段落が残る。T-REV の「却下ビュー = 元の文書」も満たせない。
- **推奨:**
  - 直前の兄弟が段落なら、**その段落の** `pPr/rPr/ins` を立てる。これは原本の `pPr` の中に子を足す局所的な編集である。
  - 直前が段落でないとき（容器の先頭、表の直後）だけ、新しい段落自身に立てる。このとき新しい段落は最後の段落ではない。
  - `ins` は `rPr` の**先頭の子**に置く。`CT_ParaRPr` では `ins`/`del`/`moveFrom`/`moveTo` が `EG_RPrBase` より前に来る。
  - 末尾への追加を T-REV の受け入れケースにする。

#### M2. 表の行の挿入・削除が `trPr/ins|del` だけでは足りない

- **根拠:**
  - sdk で `trPr/del` を持つ行は、中の run がすべて `w:del`（`delText`）で、段落記号も 3/3 が `del` だった。`trPr/ins` の行も段落記号が `ins` になっている（`probe4.py`）。
  - L274 は `trPr` だけを書くとしている。それでは、ADR-0004 の承認ビュー（`Paragraph::text`）に削除した行の文字が残る。T-REV の「承認ビュー = 新しい文字列」とも矛盾する。
- **推奨:**
  - 行を削除するときは、セル内の run を `w:del` で包んで `t → delText` にし、各段落記号に `pPr/rPr/del` をつける。行を挿入するときは `w:ins` と `pPr/rPr/ins` をつける。
  - 承認ビューが `trPr/del` の行を飛ばすかどうかを、ADR-0004 の定義に照らして明記する。

#### M3. 既存の `rPrChange` を上書きすると、却下時に元の書式へ戻らない

- **根拠:**
  - L272 の「old `rPr` の copy（既存の `rPrChange` を除く）」を同じ作者の 2 回目の変更に適用すると、元のスナップショットが消え、1 回目の変更後の状態が「元」になる。Word は最初のスナップショットを保持する。
  - ADR-0004 の R2（L596）は `ins`/`del`/`moveFrom`/`moveTo` の内側だけを拒否する。`rPrChange` や `pPrChange` を持つ run・段落は R2 の対象外で、素通りする。
- **推奨:**
  - `rPrChange` か `pPrChange` を**すでに持つ**対象は、作者を問わず `InsideRevision` で拒否する。
  - 代わりに保持する案もある。既存のスナップショットをそのまま残し、`w:id`、`w:author`、`w:date` だけを更新する。ただしこれは Word の挙動が *(u)* である。
  - どちらにするかを §5.2 に書き、R2 の対象を「変更履歴 9 種」に広げる。
  - `detach_as` の型変換も明記する。`CT_RPr` から `CT_RPrOriginal` へ、`CT_PPr` から `CT_PPrBase` へ。§10 の `[detach]` 行もこれに合わせる。

#### M4. トグルプロパティの規則が MS-OI29500 と食い違う

- **根拠:**
  - L433 は「スタイルの中（表、段落、文字の各チェーン）では、出現するたびに XOR」としている。
  - しかし [MS-OI29500] 2.1.258（Part 1 §17.7.8, Paragraph Styles）は次のとおりである。
    > Word resets the value of the toggle property to the value specified by the paragraph style if a value is present; for example, a value of 1 resets the property's state to True instead of toggling it.

    つまり段落スタイルの値は XOR ではなく**再設定**である。
  - `basedOn` チェーンの中でも、派生スタイルの値は基底の値を上書きするのが通常の継承である。
  - L436 の「スタイルの `w:val="0"` は false に設定する」は ECMA の本文（false なら変化なし）とも食い違うが、実データには合う。sdk の Microsoft テンプレートには、使われている段落スタイルのチェーンが `[b=1 → b=0]` になっている例が 17 件、`caps` で 2 件ある（`probe5.py`）。
  - 規則がつぎはぎで、`[1, 1]` のチェーンや、表スタイルと段落スタイルの組み合わせで結果が変わる。
- **推奨:**
  1. 各スタイルの値は、`basedOn` をたどって上書き継承で決める（XOR しない）。
  2. 段落スタイルは、値があれば再設定する（OI29500）。
  3. 文字スタイルは、それまでの結果に対してトグルする。表スタイルと文字スタイルの組み合わせは *(u)* として差分テストで確かめる。
  4. 直接書式は絶対値。
  - `[1, 1]`、段落と文字が両方 `b`、表と段落が両方 `b` の合成ケースを T-FMT に入れる。

#### M5. `core.xml` へのバインディング書き込みが W2 の安全分類を崩す

- **根拠:**
  - ADR-0004 は `TITLE`、`SUBJECT`、`AUTHOR`、`KEYWORDS`、`COMMENTS` と `DOCPROPERTY Title` などを安全としている（L539–550）。その前提は、`docProps` を書かないこと（L431）である。
  - §6.2（L329）で `dc:title` などを書き換えると、それらのフィールドのキャッシュされた結果が黙って古くなる。これは W2 が防ぐはずだった失敗そのものである。
  - あわせて、L329 の「SDK 401 件が Office 組み込みストア」は実測と合わない。`storeItemID` の内訳は次のとおり（`probe7.py`）。
    - core properties 161 件
    - extended properties 82 件
    - `{55AF091B-3C7A-41E3-B477-F2FDAA23CFDA}`（cover page properties。パッケージ内に対応するパーツがない）141 件
    - その他 20 件

    最も多いのは core の 161 件である。
- **推奨（Q7）:**
  - core へのバインディング書き込みは許可してよい。
  - ただし、同じプロパティを読むフィールド（`TITLE`、`AUTHOR`、`DOCPROPERTY Title` など。ストーリーと `fldSimple` を含む）が文書にあれば、`updateFields` の opt-in を要求する。opt-in がなければ `StaleFieldRisk` で拒否する。
  - `modified`、`lastModifiedBy`、`revision` は ADR-0004 のとおり古いまま残す。
  - 数値を直す。

### Minor

- **m1. 返信アンカーの位置（L187）:**
  - 実際の形は次のとおりである（`sdk-Comment010` と sdk の 460 件）。
    - range start は親の直後に並ぶ（429/460）。
    - range end と参照 run は、**親の参照 run の後ろに `(end, ref)` の組で**続く（end が親の end に隣接するのは 39/460 だけ）。
  - 推奨する書き方:
    - start は、最後の兄弟返信の start の後ろに置く。
    - end と参照 run は、最後の兄弟返信の参照 run（兄弟がなければ親の参照 run）の後ろに組で置く。
  - 「親の直後」に挿入すると、既存の返信との順序が逆になる。

- **m2. `set_resolved`（L197–198）:**
  - `done` はコメントごとの属性である（B1 の数値）。
  - 推奨: スレッドの解決として、親とすべての返信に同じ値を書く *(u: Word 365 の挙動。manual check に追加)*。読み取りはコメントごとに返す。
  - 既存のコメントに `commentEx` を作るとき、そのコメントの最終段落に `paraId` がなければ、`comments.xml` の原本の段落に属性を足すことになる。これに `ParentNotAddressable` と同じ規則を適用すると明記する。

- **m3. `delete_comment`:**
  - 参照 run の扱い: `commentReference` 要素だけを外し、run は空になったとき（`rPr` を除く）だけ消す。
  - 変更履歴の中にある参照（sdk で `del` 内 32 件、`ins` 内 19 件）は、R2 の扱いを決める。拒否を推奨する。
  - 注釈 ID の和集合は、**参照だけがあり定義がない ID**（孤立アンカー 2 件）も含むことを §2 に明記する。ADR-0011 rev 2 の「定義 ∪ 参照」を引用すればよい。
  - T-CMT（L500–504）は「全出力で」ではなく「入力からの差分で新たに壊さない」にする。入力にはもともと孤立アンカーと孤立 `commentEx`（`Comment040`）がある。

- **m4. 解放した ID の再利用（L201）:**
  - 和集合の max + 1 で取ると、最大の ID のコメントを削除した直後に同じ ID がまた割り当てられる。
  - core の registry が rId で採った方式（`4b03e93`「セッション中に削除した ID より上を保つ」）と同様に、セッション内の最高水位で割り当てる。注 ID も同様にする。
  - ADR-0011 の `AnnotationIdsExhausted` も §2 に引き継ぐ。

- **m5. 注釈 ID の和集合は Word の挙動と合っている（ADR-0011 の論点への回答）:**
  - コメントとブックマークを両方持つ sdk 72 ファイル、ブックマークと変更履歴を持つ 11 ファイル、コメントと変更履歴を持つ 8 ファイル、および fx と poi で、**種類をまたいだ衝突は 0 件**だった。衝突は docxtemplater の出力 2 件だけである（`probe1.py`）。
  - 種類ごとに独立した 0 始まりの採番なら、これだけのファイル数で衝突しないことは考えにくい。Word は保存時に 1 つの空間で採番していると読むのが自然である。
  - この測定を §2 の Evidence に加え、「保守的な解釈」から「観測と一致」に格上げしてよい。

- **m6. `paraId` の走査範囲（L121）:**
  - MS-DOCX 2.6.2.3/2.6.2.4 は、`paraId` と `textId` を「0 より大きく `0x80000000` 未満」とし、`textId` には `paraId` が必須だとしている。これは ADR と一致する。
  - 重複がある 17 ファイルの内訳: `mc:AlternateContent` の Fallback で同じ段落が複製されたもの（正当）と、テンプレートのループ出力。
  - 推奨: 走査範囲に、AC の両方の分岐、テキストボックス、glossary を含めると明記する。ADR-0011 の描画 ID と同じ扱いにする。
  - **Q2 は `77777777` を推奨する。** 仕様の範囲内であり、`paraId` との組で書く。

- **m7. MCE（Markup Compatibility）:**
  - 原本のパーツに `w14:paraId` を足したり、新しい要素に `w14:checkbox` や `w15:*` を入れたりするときは、名前空間が「宣言されている」だけでは足りない。**`mc:Ignorable` に含まれている**必要がある。
  - 推奨:
    - L192 の判定を「宣言されていて、かつ Ignorable」にする。
    - 作成したノードに `declare_namespace` するときは、同じノードに `mc:Ignorable` も付ける。

- **m8. `set_run_text` の対象:**
  - `commentReference`、`footnoteReference`、`drawing`、`fldChar`、`sym` などを含む run を `del` にする場合の扱いを明記する。Phase3 では拒否を推奨する。
  - `w:del` の中の `w:t` から `delText` への改名は、run 内の全子孫に及ぶことも書く。

- **m9. SDT:**
  - `(store, xpath)` の同一性は、prefix を名前空間 URI に解決し、`[1]` を正規化してから比べる。sdk の core バインディングには、`ns0:title` と `ns1:title` が混在している。
  - `dropDownList`/`comboBox` のストアに書くのは `value` で、表示に使うのは `displayText` である。`displayText` が省略されていれば `value` を表示する。
  - `date` はストアに書く形式が `storeMappedDataAs` で決まる。
  - 同じノードに異なる種類のコントロールがバインドされているときは、それぞれの種類の規則で表示を作る。
  - `wrap_in_control` の Checkbox で必要な `w14:checkedState`/`uncheckedState` が allowlist にない。
  - custom XML パーツと `core.xml` の編集は、WML のスキーマを持たない Layer 2 の編集である。§10 の allowlist では表せないので、使う primitive（テキスト内容の置換）と、ADR-0002 でそれを許す根拠を書く。

- **m10. 実効書式の resolver（§8.1、§8.2）:**
  - `tblLook` は、16 進の `w:val` だけを持つものが sdk で 357 件ある。ビットマスクの解釈が必要である。
  - 縞模様（band）のインデックスは、`gridSpan` と `vMerge` を考慮したグリッド列で数える。
  - **差分テストの基準（oracle）が手元にある。**
    - `themeColor` を持つ `w:color` は、すべて RGB のキャッシュ（`w:val`）も持っている（sdk 2,839 件。tint/shade つきが 1,517 件）。tint と shade の計算はこれと比べられる。
    - `cnfStyle` を持つ 19 ファイルは、条件付き書式の計算結果と比べられる。
    - LibreOffice がなくても、T-FMT に入れられる。
  - Q8: `docDefaults` がないときは Times New Roman 10 pt を使い、`Provenance::ApplicationDefault` を返す。拒否はしない。

- **m11. 細部:**
  - L165 は「§3.5 の template」を参照しているが、§3.5 に template はない。
  - §2 の注 ID と ADR-0011 の記述（区切りの ID は `-1`/`0`）を揃える。
  - Q5 は (`-1`, `0`) を推奨する。tpl の 52 件と Word 2010 以降の出力がこの形である。
  - 注のパーツを新しく作るときは、footnotes と endnotes を組で作るかどうかを決める。コーパスでは 162/162 が組で存在する。
  - W3 の `trackedChanges` でコメントを許すかどうか *(u)* を manual check に加える。

## Open questions への推奨

| Q | 推奨 |
|---|---|
| **Q1 core トランザクション** | 既存パーツの編集は、`core/add-part` の `RegistryPlan` と `stage_part(name, XmlDocument)` で扱える。clone を lens で編集して stage し、`Package::apply` は `StalePlan` 以外では失敗しない。§1 をこの API に合わせて書き直す。docx 側のレジストリ（注釈 ID、`paraId`、durable ID、SDT ID）は、apply が成功したときだけ最高水位を進める（m4）。 |
| **Q2 `textId`** | `77777777` を書く。必ず `paraId` と組にする（MS-DOCX 2.6.2.4）。 |
| **Q3 付随パーツの作成** | 「存在するときだけ追記する」案に賛成する。ただし B1 に従い、2012 名前空間のときだけ追記する。`commentsExtended` は作成してよいが、プレリリース名前空間のパーツが既にあるときは作らない（関係が二重になる）。 |
| **Q4 組み込みスタイルの定義** | owner から Word 365 の文書をもらってコピーする。もらえるまでは、ADR-0011 §8 の手順で最小限の定義（`annotation text`、`annotation reference`、`footnote text`、`footnote reference`）を書く。名前で検索し、ID は名前から作る。 |
| Q5 | (`-1`, `0`) |
| Q7 | M5 のとおり、フィールドがあれば `updateFields` を必須にする。 |
| Q8 | m10 のとおり。 |

## 判定

**Minor だけを直せば Accepted にできるか: no。**

B1（spike の再測定と、レガシー名前空間の扱い）と M1〜M5 を直したうえで round 2 に進む。

## Sources

- [MS-OI29500] 2.1.258, Part 1 §17.7.8, Paragraph Styles: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/bd62641d-7786-473f-872d-4b3205e91a82
- [MS-DOCX] 2.6.2.4 textId（`paraId` と `textId` の値の範囲）: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-docx/b7eeddec-7c50-47fb-88b6-1feec3ed832c
- [MS-DOCX] 2.8.3.2 CT_CommentsIds: https://learn.microsoft.com/en-us/openspecs/office_standards/ms-docx/6164f0a7-58f1-439a-a110-f52532b20abd
- ECMA-376 Part 1 §17.7.2 Style Hierarchy（docx4j のミラー）: https://webapp.docx4java.org/OnlineDemo/ecma376/WordML/Style%20Hierarchy.html
