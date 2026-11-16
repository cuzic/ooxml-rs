# PR #13 レビュー（Opus, ADR-0004 round 3 / 実装 round 2）

## 対象・方針

- 対象: PR #13（branch `impl/docx-phase0`, HEAD `498a47a`）。`docs/adr/0004-docx-phase0.md` revision 3 と `crates/ooxml-docx`（src / tests）、`docs/roadmap/docx.md`。比較の基準は round 2 レビュー（`0004-opus-review-round2.md`, HEAD `91c997b` / レビュー commit `2576c6e`）。
- 方法: 読み取り専用の敵対的レビュー。worktree を scratchpad（`review-docx-r3/repo`）に複製して、次のことをした。
  - 既存テストを全部実行した（lib 3、adversarial 14、editing 6、fixtures 7、gates 10、oracles 4、readback 2、doctest 1。全 green）。
  - **差分テスト**: rev 2 の再帰版 `body.rs` / `fields.rs`（`2576c6e`）を `old_body` / `old_fields` としてクレートに入れ、新しい反復版と突き合わせた（`src/difftest.rs`）。比べたのは、段落と run の path、wrapper、`in_field`、`open_fields_at_end`、フィールドのコード列。
  - 新しいプローブ `tests/r3.rs`（Q01〜Q07）。
  - ミューテーション 15 本（`review-docx-r3/mut/run.py`）。
  - POI 129 件、SDK 419 件、docx4j 12 件を `python3 -I` で走査した（`scan.py`）。
  - fixture は parse にだけ使った。リポジトリはこのファイル以外触っていない。
- 重大度: **Blocker/Major** = 破損、黙った内容変更、古いフィールドが残りうること、拒否の欠落。それ以外は **Minor**。判断が分かれるものには「判断」と付けた。*(u)* は Word で未確認の事項。

## 結論（先に）

N1 と N2 は解消した。m1〜m6 も、m1 と m2 に残った穴を除けば解消している。

- N1（深い入れ子）:
  - core の深さ上限 256 による O5 と、frame 方式の線形な index で解消した。
  - 反復化した走査は、再帰版と同じ run を訪れることを確かめた。差分テストの範囲は次のとおり。
    - 実コーパス 563 ファイル（56,481 段落、82,677 run。深さ 15,000 超の `deep-table-cell` を含む）
    - ランダム生成 50,000 文書（292 万段落、820 万 run）
  - 結果は全件一致した。
- N2（引数による分類）: 解消した。`INFO`、`DOCPROPERTY` の統計値、`FILESIZE`、`BARCODE \b`、計算型の `FORMTEXT` は refused になる。ミューテーションでも、どの規則を外してもテストが落ちる。

ただし、フィールド分類の監査（task 2）で **Major が1件**見つかった。

- **N3**: `{ INCLUDETEXT "{ FILENAME \p }" ブックマーク }`（自分自身の文書を取り込む Word の定番の書き方）が safe として通る。
- その結果、ブックマーク内の run を編集すると、取り込んだ側の結果が古いまま残る。これは REF と同じ依存で、REF は refused にしている。
- ADR は「自己参照は exotic で検出しない *(u)*」としている。しかしこの形は入れ子の `FILENAME` から構文だけで検出できる。修正は数行で済む。

それ以外はすべて Minor で、いずれも Word が書かない入力か、コーパスに 0 件のもの。

- m1 の残り: begin を本文走査から見えない場所に置くと、R1 が fail open になる。
- m2 の残り: 末尾が空の `w:sdt` などのとき、A2 をすり抜ける。
- settings の関係が2本ある場合。
- 曲引用符で書いた `DOCPROPERTY`。
- `BARCODE` のスイッチの綴り。
- テストの検出力の小さな穴。

## round-2 指摘の解消状況

| ID | 内容 | 状況 | 根拠 |
|---|---|---|---|
| N1 | 深い入れ子で O(d²) メモリと stack overflow | **解消** | core は `DEFAULT_MAX_DEPTH = 256` を超えると parse 中に `TooDeep` で拒否する（`ooxml-core/src/xml/parse.rs:214`）。main、settings、story、custom.xml はすべて既定の上限で parse される。`body.rs` は frame（parent, index, wrapper）の arena になり、run ごとの path を持たなくなった（`body.rs:118-176`）。走査は明示的なスタックで書かれている（`body.rs:196-256`、`fields.rs:178-197`）。`run_lens` も反復になった（`document.rs:345-369`）。テストは `adversarial.rs:341-379`（10k と 40k の深さは 5 s 以内に `TooDeep`、header でも同じ。深さ 254 は開けて編集でき、255 以上は拒否）。**差分テスト**で、再帰版と path、wrapper、`in_field` が全件一致した（上記）。ADR §6 O5、§7 の注記も実装と一致する |
| N2 | 先頭トークンだけで分類 | **解消**（分類の穴は新規 N3 と m9） | `field_key` と `is_safe` は `fields.rs:123-152`。Q04 の 120 コードの表（下記）で、INFO/DOCPROPERTY の統計値はすべて refused、`NumPages`/`Pages` は safe になる。ミューテーション M7（DOCPROPERTY を常に safe）、M8（INFO の引数を無視）、M9（calculated を検出しない）、M15（linkTarget を無視）は、どれも専用テストで落ちる |
| m1 | 認識できない fldChar で fail open | **一部** | 指摘した3形（type 不正、開いていないところの instrText/separate/end、段落内の AC）は MALFORMED になる（`fields.rs:216-262`）。ただし次の2点が残る。(a) begin が本文走査に見えない場所にあり、結果の run が本文にある場合は、まだ fail open（新規 m7）。(b) type 不正の規則だけを外すミューテーション M4 で、テストが1本も落ちない（新規 m10） |
| m2 | 段落の `sectPr` の後ろへの append | **一部** | A2 `SectionBreakAtEnd` は実装されていて（`document.rs:272, 290-325`）、テストもある（M6 で落ちる）。ただし、区切りの段落の後ろに段落を持たないブロック（空の `w:sdt`、空の `w:customXml`、行の無い `w:tbl`）があると、すり抜ける（新規 m8） |
| m3 | 保護信号と重複 | **解消**（settings part 自体の重複は新規 m11） | `trackRevisions` と `documentProtection` は、どれか1つでも on なら on（`document.rs:112-123`）。`writeProtection` と Mark as Final は報告だけで、gate にはしない。M10（最初の1つだけ見る）は `m3_protection_variants` で落ちる |
| m4 | オラクルの検出力 | **解消** | T2 は開始タグと `w:rPr` が byte 一致することを確かめ、文字列 `"a  b\t {own} "` を使い、保存して開き直して確かめる。M1（rPr を落とす）では `t2_every_settable_run` を含む6本、M2（二重空白の preserve を外す）では T2 と append を含む4本、M3（隣の run に書く）では11本が落ちる。ADR §7 のミューテーションの記述は再現した |
| m5 | §5 の docProps | **解消** | §5 に `core.xml`、`custom.xml`、SAVEDATE/LASTSAVEDBY/REVNUM の対応が書かれている |
| m6 | no-op が `xml:space` を見ない | **解消** | `lacks_needed_preserve`（`text.rs:127`）。M11 は `m6_noop_requires_needed_preserve` で落ちる。Q07 の境界ケースも妥当だった。`w:r` に継承された preserve、`xml:space="default"` で端に空白、空白だけの `w:t`、2つ目の `w:t` だけ preserve が無い場合は、どれも preserve を付けて書き直し、読み直すと同じ文字列になる |

## task 2: フィールド分類の監査

基準は「キャッシュされた結果が、Phase0 の編集（フィールド外の run の文字列置換と、本文末尾への段落追加）で変わる内容に依存しないこと」。参照した Microsoft のドキュメントは次のとおり。

- [List of field codes in Word](https://support.microsoft.com/en-us/office/list-of-field-codes-in-word-1ad6d91a-55a7-4a8d-b535-cf7888659a51)
- 各フィールドのページ（[IncludeText](https://support.microsoft.com/en-us/word/field-codes-includetext-field)、[DocProperty](https://support.microsoft.com/en-us/word/field-codes-docproperty-field) など）

Q04 は、各コードを1つのフィールドとして置いたときの `write_gate()` を実測したもの。

| 区分 | コード（実測） | 判定 |
|---|---|---|
| ページ割り | `PAGE` `NUMPAGES` `SECTION` `SECTIONPAGES` → safe | 妥当。ADR の基準どおり、Word がレイアウトのたびに再計算する。本文中の PAGE を Word がレイアウトのたびに更新するかは *(u)* で、round 1 から owner が受け入れている。A2 があるので、SECTION/SECTIONPAGES は Phase0 では節の数が変わらない |
| 日時 | `DATE` `TIME` `CREATEDATE` `SAVEDATE` `PRINTDATE` `EDITTIME` → safe | 妥当。SAVEDATE は core.xml と同じく古いまま残るので、互いに矛盾しない（§5） |
| 文書情報と環境 | `AUTHOR`（引数で設定する形も含む）`TITLE` `SUBJECT` `KEYWORDS` `COMMENTS` `FILENAME` `LASTSAVEDBY` `TEMPLATE` `DOCVARIABLE` `USERNAME` `USERINITIALS` `USERADDRESS` `REVNUM` → safe。`FILESIZE` → refused | 妥当。DOCVARIABLE の値は settings の `w:docVars` にあり、Phase0 は書かない |
| INFO | `INFO Author/NumPages/RevNum/SaveDate/EditTime/Title "x"` → safe。`INFO NumWords/NumChars/FileSize`（引用符付き、小文字も）→ refused | 妥当。INFO の引数の種類の一覧（Author … Title）と完全に対応する。未知の引数は fail closed |
| DOCPROPERTY | `Title/Pages/Security/TotalEditingTime/RevisionNumber/LastSavedTime/Company` → safe。`Words/Characters/CharactersWithSpaces/Lines/Paragraphs/Bytes`、`\*` が直後に続く形 → refused。linkTarget を持つカスタムプロパティ → refused | 組み込みプロパティの一覧に照らして妥当。ただし**拒否リスト方式**なので、未知の綴りは safe になる。曲引用符の `DOCPROPERTY “Words”` は safe になった（m9） |
| 差し込み印刷 | `MERGEFIELD` `MERGEREC` `MERGESEQ` `NEXT` `NEXTIF` `SKIPIF` `GREETINGLINE` `ADDRESSBLOCK` → safe。`DATABASE`、`MERGEBARCODE` → refused | safe は妥当（差し込みデータか、結果を持たない）。DATABASE と MERGEBARCODE は過剰な拒否だが、安全側 |
| 条件・式 | `IF` `COMPARE` `=` `QUOTE` → refused | 妥当。COMPARE と IF はブックマーク名を参照しうる。QUOTE は内容に依存しない（入れ子のフィールドは別に分類される）ので過剰な拒否であり、`CONTENT_DEPENDENT` に置く理由の説明は不正確（Minor、文書のみ） |
| 外部の内容 | `INCLUDETEXT "file" bm` `INCLUDEPICTURE` `LINK` `EMBED` → safe | **`INCLUDETEXT`/`LINK` の自己参照は誤分類（N3）**。INCLUDEPICTURE と EMBED は妥当 |
| 表示だけ | `SYMBOL` `EQ` `ADVANCE` `GOTOBUTTON` `MACROBUTTON` `PRIVATE` `DISPLAYBARCODE` → safe。`PRINT` → refused | 妥当。PRINT は結果を持たないので過剰な拒否 |
| BARCODE | `BARCODE "98052" \u` → safe。`BARCODE bm \b` → refused。**`BARCODE bm\b` と `BARCODE bm \B` → safe** | スイッチの検出がトークン一致だけ（m9） |
| 入力 | `FILLIN` `ASK` `SET` `FORMTEXT` `FORMCHECKBOX` `FORMDROPDOWN` → safe。計算型の FORMTEXT → refused | 妥当 |
| 番号・参照・索引 | `STYLEREF` `LISTNUM` `BIDIOUTLINE` `AUTONUM*` `TOA` `TA` `INDEX` `XE` `RD` `TC` `CITATION` `BIBLIOGRAPHY` `TOC` `REF` `PAGEREF` `NOTEREF` `SEQ` `AUTOTEXT` `AUTOTEXTLIST` `ADDIN` → refused | 妥当（一部は過剰な拒否） |
| HYPERLINK | `"url"`、`\l bm` → safe | 妥当。表示するテキストはフィールド自身のもの |
| 字句 | `page`、`Page\*Arabic` → safe。全角空白で始まる `　PAGE` → refused `[""]` | fail closed。日本語の文書では過剰な拒否になりうる（参考） |

誤分類（safe であってはならないのに safe になるもの）は、N3（INCLUDETEXT/LINK の自己参照）と m9（曲引用符、BARCODE のスイッチの綴り）だけ。refused 側の誤り（過剰な拒否）はあるが、安全側なので指摘しない。

## 指摘

### N3 [Major, 判断] `INCLUDETEXT "{ FILENAME \p }" bm`（自己取り込み）が safe として通り、結果が古いまま残る

**根拠**
- `INCLUDETEXT` と `LINK` は無条件で SAFE（`fields.rs:39`）。入れ子の `FILENAME \p` も safe。
- Q05 の入力: 本文 `<bookmark Client>ACME</bookmark>`、`{ INCLUDETEXT "{ FILENAME \p }" Client }`（結果は `ACME`）。
  - `write_gate()` は Ok になる。
  - `set_run_text(0, 0, "Globex")` も Ok になり、取り込んだ側は `ACME` のまま残る。
- IncludeText は、ファイルの全体か、ブックマークで指定した一部を挿入する（Microsoft の IncludeText のページ）。`FILENAME \p` を入れ子にすれば、取り込み元は文書自身になる。同じ文書の中で文字列を繰り返すための、よく知られた書き方である。
- これは `REF bm` と同じ依存関係で、REF は refused にしている。ADR の監査表（L519）は「exotic で検出しない *(u)*」としている。しかし入れ子の FILENAME は**構文だけで検出できる**。そこを safe とするのは、W2 の基準（L486-487）と食い違う。
- コーパスでの出現は 0 件（INCLUDETEXT/LINK は POI、SDK、d4j の 626 ファイル中 0 件）。

**提案**
- `Open` に「入れ子のフィールドを含んだ」を表すフラグを足す。`INCLUDETEXT`/`LINK` の命令に入れ子のフィールドがあれば、キー `INCLUDETEXT (nested source)` などで refused にする。
- リテラルのパスで自分自身を指す場合は、検出できないものとして *(u)* のまま ADR に残す。
- 判断: 「コーパス 0 件で、ADR が既知の穴として書いている」ことを重く見るなら Minor でもよい。ただ、round 2 の N2（INFO NumWords、コーパス 0 件）を Major にしたこととの整合を取って、Major とした。owner が明示的に受け入れるなら、ADR に一行足すだけでもよい。

### m7 [Minor] begin を本文走査が見ない場所に置くと、R1 が fail open になる（m1 の残り）

**根拠（Q01）**
- 正しい形の対照では、`RESULT` の run は `InsideField` になる。
- 次の6通りでは、`MERGEFIELD Name` の結果の run が `in_field == false` かつ `check_settable() == Ok` になる。gate は Ok で、`set_run_text` も通る。
  - begin/separate がテキストボックスの `w:txbxContent` の段落にある。
  - begin/separate がルビの `w:rt` にある。
  - `w:fldChar` が `w:p` の直下にある（run の外）。
  - begin/separate が本文レベルの `mc:AlternateContent` の段落にあり、end が次の本文段落にある。
  - begin/separate が `m:oMath` の中にある。
  - `w:fldChar` が `w:r` の子のさらに子にある。
- 原因: `FieldScan` はすべての要素を見る。これらを「完結したフィールド」として safe に分類する。一方で、本文走査（`body.rs:235`）は本文の流れにある run の直下の fldChar しか数えない。
- 影響は safe なフィールドの結果の書き換えに限られる。refused のコードは文書全体の gate で止まる。ただ、R1（「フィールドの結果は編集させない」）の保証の穴であることに変わりはない。どれもスキーマ違反か、Word が書かない形。

**提案**
- `FieldScan` で、fldChar の begin と separate/end の「容器」（最も内側の `w:p`、`w:txbxContent`、`w:rt`、`m:oMath`、`mc:Choice`/`mc:Fallback`）が違う場合に MALFORMED とする。
- 親が `w:r` でない `w:fldChar` も MALFORMED とする。
- 上の6通りをテストに足す。

### m8 [Minor] A2 のすり抜け: 区切りの段落の後ろに、段落を持たないブロックがある場合（m2 の残り）

**根拠（Q02）**
- 最後の段落に `pPr/sectPr`（横向き）があり、本文直下に `sectPr` が無いとする。その後ろに次のどれかが続くと、`append_paragraph` は Ok になる。
  - 空の `<w:sdt><w:sdtPr/><w:sdtContent/></w:sdt>`
  - ブックマークだけの `w:sdtContent`
  - 空の `<w:customXml/>`
  - 行の無い `<w:tbl>`
  - 段落の外の `w:sdtContent`
- このとき、新しい段落は節区切りの**後ろ**に入る。つまり既定の用紙設定の新しい節になる。これは m2 で防ごうとした結果そのもの。
- 原因: `ends_with_section_break`（`document.rs:290-325`）は「最後のブロック要素」と「流れの最後の段落」が同じブロックにあることを条件にしている。そのため、最後のブロックが段落を持たないと false になる。`altChunk` の場合は正しく拒否された。
- コーパスでの出現は 0 件。空のブロック sdt はスキーマ上は有効。

**提案**
- 判定を「本文直下に `sectPr` が無く、`pPr/sectPr` を持つ段落の後ろに、本文の順序で段落（`w:p` の子孫）が1つも無い」に変える。
- 最後の `pPr/sectPr` 段落を本文の順序で探し、それより後ろに段落が無ければ A2 とする。

### m9 [Minor, 判断] DOCPROPERTY が拒否リスト方式で、曲引用符の綴りを通す。BARCODE のスイッチ検出がトークン一致だけ

**根拠（Q04, Q06）**
- 次の2つは safe になる。
  - `DOCPROPERTY “Words”`（U+201C/U+201D）: `first_argument` は ASCII の `"` しか引用符として扱わないので、キーが `DOCPROPERTY “WORDS”` になり、統計値の一覧と一致しない。
  - `BARCODE bm\b` と `BARCODE bm \B`: `split_whitespace` で `\b` と完全に一致するかだけを見ている。
- 同じ綴りでも、INFO は許可リスト方式なので fail closed になる（`INFO “NumWords”` は refused）。
- Word が曲引用符を引用符として扱うかは *(u)*。「入力オートフォーマット」を有効にしてフィールドコードを手で打つと、`“…”` が普通にできる。もし Word がこれを引用符として扱うなら、統計値のフィールドが gate を通ることになり、Major に上がる。
- BARCODE は現行の Word では提供されていないとされる *(u)* ので、影響は小さい。
- コーパスでの出現は、曲引用符、BARCODE ともに 0 件。

**提案**
- `first_argument` で `“ ” „ ＂` も引用符として扱う。
- さらに `DOCPROPERTY` を許可リスト方式にする。safe とするのは、組み込みのうち統計値でないもの（Microsoft の DocProperty の一覧）と、`custom.xml` にあって `linkTarget` を持たない名前。それ以外は refused にする。
- `BARCODE` は、命令の中に大文字小文字を問わず `\b` が現れたら refused にする。

### m10 [Minor] テストの検出力: type 不正の MALFORMED 規則と `has_fld_char` を単独で落とすテストが無い

| ミューテーション | 落ちるテスト |
|---|---|
| M4: `_ => MALFORMED` を `_ => {}` に（`fields.rs:238`） | **なし**。`m1_unknown_fld_char_type_is_malformed` は、begin が不正なら後ろの instrText/separate が「開いていない」ことになり、別の経路で MALFORMED になるので通ってしまう |
| M13: `in_field` から `has_fld_char` を外す（`body.rs:244`） | **なし**。fldChar を持つ run は R4 でも拒否されるので、変わるのはエラーの種類だけ（実質的に等価なミューテーション） |
| M5、M6、M7〜M12、M14、M15 | それぞれ専用のテストで落ちる |

- 提案: M4 用に、`begin` + `PAGE` + `<w:fldChar w:fldCharType="Separate"/>` + 結果 + `end` が MALFORMED になるテストを足す。この形は他の規則では捕まらない。
- M13 は ADR に「R4 と重複する防御」と書けば足りる。

### m11 [Minor] settings への関係が2本あると、2本目を見ない

- Q03: `settings.xml`（空）と `settings2.xml`（`trackRevisions` と `readOnly` の保護を持つ）への関係が2本あるとき、`write_gates()` は `[]` になる（`document.rs:95` の `.next()`）。
- ECMA-376 Part 1 では、settings への関係は1本まで。Word がどちらを使うかは *(u)*。コーパスでの出現は 0 件。
- 提案: 重複要素（m3）と同じく、全部の settings part を読んでどれか1つでも on なら on とする（fail closed）。2本以上なら型付きで拒否する、でもよい。

## task 3: 新しい経路での破損と黙った変更

- **非再帰の走査**: 差分テストで再帰版と全件一致した（上記）。frame の parent と index から作り直す path は、run_lens が受け付ける形と一致する。T2 と T5 の全 run 編集が green で、M3（隣に書く）も検出される。
- **深さ**: 254 は通り、257 は拒否される。lens の insert も上限で抑えられている（core）。深さが上限に近い文書への append も、テスト `n1_just_under_the_limit_works` で確かめられている。
- **不正なフィールドの拒否**: gate として働き、拒否はすべて変更の前に起きる（`write_gate()` が先頭）。漏れは m7。
- **SectionBreakAtEnd**: 拒否のときに byte が変わらないことはテストで確かめている。すり抜けは m8。
- **settings の重複**: 要素の重複は解消した。part の重複は m11。
- **m6 の no-op 規則**: 書き直すときも fast path で開始タグを保ち、`xml:space` を付けるだけ。T2 の own-text の byte 一致も保たれている。問題は見つからなかった。
- 破損（XML の破損、別の run の変更、`rPr` の消失）は見つからなかった。

## task 4: オラクルのミューテーション再検査

round 2 で検出できなかった M1（rPr を落とす）と M2（二重空白）は、T2 で落ちるようになった。ADR §7 の記述どおり。全 15 本の結果は上記のとおりで、検出できないのは M4 と M13 だけ（m10）。

## 判定

- **converged（Blocker/Major が無い）: no**
  - Major は N3 の1件だけ（INCLUDETEXT/LINK の入れ子の FILENAME による自己参照）。修正は `fields.rs` の数行とテスト1本で済み、設計の方向は変わらない。
  - owner が「既知の穴として受け入れる」と ADR に明記するなら、Minor として扱ってよい（判断）。
- **ADR を Minor の修正だけで Accepted にできるか: yes（N3 を直すか、owner が受け入れた後）**
  - N1、N2 は本文、実装、テストのすべてで閉じている。
  - 残る m7〜m11 は、どれも Word が書かない入力か、コーパス 0 件。Accepted の後で直してよい。
  - ただし、m9 の曲引用符は Word の手動確認（open question 1）のついでに挙動を確かめること。Word が引用符として扱うと分かれば、Major に上がる。
  - N3 の修正の確認は、テスト1本（Q05 相当が refused になること）で足りる見込み。

### 再現手順

- 一式: `<scratchpad>/review-docx-r3/`
  - `repo/crates/ooxml-docx/src/{difftest,old_body,old_fields}.rs`
  - `repo/crates/ooxml-docx/tests/r3.rs`
  - `mut/run.py`、`mut/out.txt`
  - `scan.py`
- 差分テスト: `DIFF_DIRS=<POI>:<SDK>:<d4j> cargo test -p ooxml-docx --lib --release difftest -- --nocapture`
- プローブ: `cargo test -p ooxml-docx --test r3 -- --nocapture --test-threads 1`
- ミューテーション: `S=<scratchpad> python3 -I mut/run.py [M4 …]`
