# PR #14 レビュー（Opus, round 2）

対象: `core/remove-part`（3393688。round 1 への修正 2232c25、ADR-0001 の暫定 note 3393688）。
照合した文書: round 1（`docs/reviews/pr14-round1.md`）、ADR-0001 の暫定 post-acceptance note、ADR-0003 rev 3（`impl/xlsx-phase1`）の §3.1 と §4.3、`crates/ooxml-core/README.md`、PR 本文。
プローブ: scratchpad `review-pr14-r2/probe_r2.rs`（Q1〜Q7 とコーパス走査）、`review-pr14-r2/mutants.py`（mutant 19 件）。作業コピーは `review-pr14-r2/wt` で、worktree には何も書いていない。

## 結論（先に）

**未収束（Major 1 件）。** round 1 の M1、M2、M3、m1、m2、m3、m5、m7 は解消している。m4 と m6 は部分的な解消である。

- 適用の経路は infallible になった。`plan_removal(&self)` を検証で共有し、`apply_removal` には `expect`、`unwrap`、`?` がない。`package_rels` は `without_ids` で作り、parse し直さない。lens で編集した `_rels/.rels`、Replaced/Parsed なレジストリ、重複した Override、自分の `.rels` の大小文字違い、のどれでも panic は再現しなかった。
- 「unescape 後の値が Id と一致する属性があれば拒否する」規則が、正当な削除を誤って拒否した例はなかった。対象は 2,094 パッケージで、そのうち calcChain を持つ 279 件はすべて `Ok` になった。全 part 26,286 件の検査でも、`r:`/`o:` 名前空間だけを見る規則より余分に拒否したものは 0 件だった。

残る Major は 1 件である。

- **N1**: 削除のための走査は part 名の同値性を完全には扱っていない。大小文字の違う `_RELS` ディレクトリにある `.rels`（Q2）と、zip entry 名と符号化の違う Target（%エンコードと生の UTF-8、Q1）を見落とす。このとき `Ok` を返し、**ダングリングした関係が残る**。Phase1 の calcChain の経路は事実上影響を受けない（理由は N1 に書く）。ただし汎用の pub API が保証している内容には反するので、規定どおり Major とする。

ほかは Minor が 5 件、メモが 2 件である。

## 解消状況

| round 1 | 状況 | 根拠 |
|---|---|---|
| **M1** 参照の見落とし（VML `o:relid`、文字参照） | **解消** | `refers_to_rel_id`（`package.rs:652-675`）は、名前空間を問わず全属性を `normalized_value` で比べる。DTD は Err（scan 不能）になる。テスト `remove_part_sees_every_reference_form` は P1、P8、Strict、無関係な属性名、非 XML のソースを扱う。mutant C（unescape しない）、M（属性名で絞る）、N（ソースの走査を省く）、O（scan できないソースを許す）はすべて検出された。過剰拒否は 0 件（下記）。 |
| **M2** check が Ok でも remove が panic する | **解消** | apply（`package.rs:519-537`）に失敗し得る処理はない。`remove_root_child_with_leading_ws` の index は同じ木から計算する。`children[index-1]` は remove の後でも範囲内にある。唯一の `debug_assert!(false)` は、Replaced な `.rels` を plan で拒否するガード（`package.rs:458-460`）があるので到達しない。ただしこのガードはテストされていない（N4）。P4 の形はテストになっている。 |
| **M3** Proposed の ADR を根拠に Accepted の ADR へ例外を入れる | **解消（N3 の注記つき）** | ADR-0001 の暫定 note は狭い。意図した呼び出し元は calcChain だけで、型の allowlist、ソースの属性の走査、番号を振り直さないことを条件にしている。可逆でもある（ADR-0003 が不採択なら API を削除する）。コードとも一致する。ただし、「ADR-0003 §4.3 がこの note を置き換える」とした部分には問題がある（N3）。 |
| **m1** 暗黙の参照 | **解消** | `allowed_rel_types` を必須の引数にし、許可されない型は `RelationshipTypeNotAllowed` で拒否する。mutant D は検出された。 |
| **m2** エラーの型と検査範囲 | **解消** | `RegistryNotEditable` に統一した。対象を含まない壊れた `.rels` は無視する。対象を含む Replaced な `.rels` は拒否する（Q5 で確認）。Replaced な CT が Override を含まない場合は通る。 |
| **m3** Err でも状態が変わる | **解消** | plan は `&self` で動き、parse した木は `install` に入れて apply で初めて差し込む。`assert_refused` で状態と L1 出力が変わらないことを確かめている。mutant L は検出された。 |
| **m4** 大小文字 | **部分的** | `find_ci` で part を探し、自分の `.rels` も大小文字を区別せずに探す（mutant F と R は検出）。ただし、`.rels` part かどうかの判定（`is_rels_part`: `dir == "_rels"`）と、ソースの導出（`strip_suffix("_rels")`）は大小文字を区別したままである。→ N1(a) |
| **m5** 重複した Override | **解消** | 重複した Override をすべて削除する（XML 側も `ContentTypes` キャッシュ側も）。mutant G は検出された。大小文字だけが違う part の組は `open` が `duplicate part name` として拒否する（Q6）。 |
| **m6** テストの穴 | **部分的** | round 1 の mutant 2 件（`package_rels` の更新を省く、CT の Override の削除を省く）は、どちらも今回は検出された（A と B）。新たに生き残った mutant がある。→ N4 |
| **m7** 理由が String | **解消** | `RemoveRefusal` は `#[non_exhaustive]` の enum になり、`lib.rs` で再 export されている。 |

## 新しい指摘

### N1 [Major] part 名の同値性に穴があり、`Ok` のままダングリングした関係が残る

**根拠**（`probe_r2.rs`）
- (a) Q2: `xl/_RELS/workbook.xml.rels` が calcChain への関係を持つ場合。`remove_part("xl/calcChain.xml", &[CALC_CHAIN])` は `Ok(RemovedPart { relationships: [], .. })` を返す。part は消えるが、関係は残る。原因は `is_rels_part`（`package.rs:631-634`）が `dir == "_rels" || dir.ends_with("/_rels")` と大小文字を区別して比べ、この part を走査の対象から外すことである。OPC の part 名の同値性は ASCII で大小文字を区別しないので、これは workbook の `.rels` そのものである。
- (b) Q1: Target と zip entry 名で符号化が違う場合。entry が `xl/calc%E7%94%BB.xml` で Target が `calc画.xml` の組と、その逆の組は、どちらも `Ok`（関係 0 件）になり、関係が残る。`resolve_target` は %デコードもせず、IRI を URI に写すこともしない。OPC では、IRI の Target は UTF-8 を %エンコードした part 名と同じ part を指すはずである。ASCII の `calc%43hain.xml` も同じように素通りするが、非予約文字を %エンコードした part 名は OPC で禁止されているので、こちらは参考に留める。
- 影響の範囲: xlsx Phase1 が消すのは ASCII の `calcChain.xml` だけである。また、(a) の形のパッケージでは `relationships("xl/workbook.xml")` も 0 件を返すので（Q2）、xlsx の open がシートを見つけられない。そのため Phase1 の経路にはまず届かない。一方、doc（`package.rs:350-351`）と README は「part 名は大小文字を区別しない」「after a successful validation … nothing still refers」と保証している。汎用 API が破損したパッケージを `Ok` で返すので、規定の「ダングリングがあり得る」に当たる。

**提案**（どれも局所的な修正）
- `is_rels_part` と `source_of_rels_part` で、`_rels` の比較を ASCII の大小文字を区別しないものにする。
- Target と part 名は、%デコードしたうえで、大小文字を区別せずに比べる（OPC の正規化）。
- 最低限の安全網として、走査の後に、名前が `.rels` で終わる part（大小文字を問わない）の生テキストに、ファイル名（生、%エンコード、%デコードの各形）が含まれていて、どの関係とも一致しなかったときに拒否する。`mentions()` を一般化するだけで済み、外れても拒否する方向にしか外れない。
- Q1 と Q2 をテストにする。

### N2 [Minor] `dir/_rels/.rels` を package の `.rels` として扱うので、キャッシュが壊れる

- `source_of_rels_part`（`package.rs:637-645`）は、stem が空なら `None`、つまり package 扱いにする。そのため `foo/_rels/.rels` も package の関係として扱われ、その Id が `package_ids` に入る。
- Q3: `foo/_rels/.rels` の `rId1` が `/x.xml` を指している状態で `remove_part("x.xml", &["urn:x"])` を呼ぶと、`package_rels` キャッシュから本物の `_rels/.rels` の `rId1`（officeDocument）まで消える。`main_document_part()` は `Some("xl/workbook.xml")` から `None` に変わる。保存した結果を開き直すと正しい値に戻るので、ズレは**メモリ上だけ**である。入力が不正（`foo/` は part 名になれない）なので Minor とする。
- 提案: package として扱うのは `rels_name` が `_rels/.rels` に（大小文字を区別せずに）一致する場合だけにする。

### N3 [Minor・ADR の整合] 暫定 note の「ADR-0003 §4.3 がこの note を置き換える」は、今の rev 3 の文面のままでは条件を弱める

- ADR-0001 の note: 「If ADR-0003 is accepted, its §4.3 text replaces this note」。
- `impl/xlsx-phase1` にある ADR-0003 rev 3 は、まだ旧仕様のままである。
  - §4.3 の note 文面は「`r:id`-style attributes」で、型の allowlist の条件がない。
  - 前提条件は「attribute in the relationships namespace」になっている。
  - §3.1 と §4.3 は 1 引数の `remove_part(name)` と `check_remove_part(name)` を呼んでいる。
  - 同じブランチの `ooxml-core` も 1 引数の古い API のコピーである。
- このまま ADR-0003 が accept されると、ADR-0001 が記録している不変条件から「呼び出し側が型を許可したものに限る」が落ち、コードと ADR が食い違う。今の時点で main の文書どうしは矛盾していないので Minor とするが、ADR-0003 を accept するまでに必ず直す。
- 提案（どちらか、または両方）
  - note を「ADR-0003 が accept され、その §4.3 が少なくともこの note の条件（型の allowlist、全属性の走査）を含むとき、それがこの note を置き換える」に変える。
  - ADR-0003 rev 4 で §3.1 と §4.3 を 2 引数の API と any-attribute の規則に合わせる。

### N4 [Minor] 生き残った mutant（テストの穴）

| mutant | 結果 | 意味 |
|---|---|---|
| I: Replaced な `.rels` に一致する関係があっても拒否しない（`package.rs:458-460` を削除） | **生存** | このガードは M2 と破損の両方を支えている。ガードがないと、debug ビルドでは `debug_assert!(false)` で panic し、release ビルドでは `.rels` を編集せずに part だけを消す（ダングリング）。挙動自体は Q5 で正しいことを確認した。テストを 1 本足す。 |
| J: `TargetMode="External"` の判定を削除 | 生存 | 既存のテストにある外部関係は `http://example.com/calcChain.xml` で、そもそも解決しても part 名と一致しない。`Target="calcChain.xml" TargetMode="External"` のケースが要る。 |
| Q: Target を大小文字を区別して比べる | 生存 | `Target="CALCCHAIN.xml"` のケースがない。 |
| H: scan で `<!DOCTYPE>` を受け入れる | 生存 | DTD を持つソースを `SourceNotScannable` にする挙動のテストがない。 |

そのほか、A〜G、K〜P、R、S は検出された（mutant T は等価なので除外した）。

### N5 [Minor・API] `allowed_rel_types` は文字列の完全一致である

- Strict の calcChain 型（`http://purl.oclc.org/ooxml/officeDocument/relationships/calcChain`）は、渡し忘れると `RelationshipTypeNotAllowed` になる（Q5）。安全側には倒れているし、xlsx は Strict を拒否する。
- core に `REL_CALC_CHAIN` と Strict 版の定数を置くか、doc に「Strict の型は別の項目として渡す」と書くとよい。
- 空の slice を渡しても、入ってくる関係を持たない（孤立した）part は削除できる。関係がなければ型による暗黙の参照もないので、仕様として妥当である。doc に一言書いてもよい。

### N6 [Minor・nit] `opc.rs` のモジュール doc（L3-5）は、根拠として「ADR-0003 §4.3」だけを挙げている

- `package.rs` の doc と README は、ADR-0001 の暫定 note を先に挙げる形に直っている。`opc.rs` だけが古い。

### メモ（指摘ではない）

- **`mentions()` の判定について**: 読めない `.rels` は、対象のファイル名を含むときだけ拒否する。ファイル名を文字参照（`calc&#67;hain.xml`）や %エンコードで書くと、この判定を素通りする（Q4: どちらも `Ok`）。ただし、それが起きるのは well-formed でないか、ルート要素が違う `.rels` に限られ、適合する consumer はそもそもその関係を解決できない。そのため、解決できるダングリングは生じない。誤判定は拒否の方向にしか起きない（`mycalcchain.xml` のような部分一致）。判定として妥当である。N1 の安全網を入れる場合は、同じ正規化を使うとよい。
- **テキストに書かれた Id**（Q7: `<ref>rId2</ref>`）は走査しない。要素のテキストで関係の Id を参照する OOXML の構文は知られていないし、doc も「attribute」と書いているので、仕様どおりである。
- **走査の範囲**: すべての `.rels`（Raw、Parsed、Replaced のいずれも）を対象にする。どこかの `.rels` から対象を指す関係があれば、その関係の**ソース part**（`read_part` なので、Replaced や dirty な Parsed なら現在の内容）を走査する。そのため、別のソースが自分の `.rels` 経由で同じ part を参照していても扱える（`remove_part_edge_cases` に、3 件の関係が 2 つの `.rels` にある例がある）。ソースが存在しない孤立 `.rels` の場合は走査しない。参照する主体がいないので、これは正しい。
- **L1 の素通し**: 触らなかった entry は raw-copy される（既存テストで name、CRC、圧縮バイト、順序を比較している）。コーパスにある calcChain 279 件は、apply、保存、開き直しまで panic や Err なしに通り、開き直した後には calcChain が存在しない。

## 過剰拒否の計測

- 対象: `reference/fixtures`（全形式）、POI と calamine の xlsx（scratchpad `xlsxfx/dl`、`dlbig`、`calam`）、umya のテストファイル、docx/pptx コーパス（`corpus/files`）で 746 パッケージ。さらに `p1/o3in`（POI の xlsx を Phase1 の入出力にした変種）で 1,348 パッケージ。
- calcChain: 279 件すべてで `check_remove_part(cc, &[transitional, strict])` が `Ok` になり、`remove_part`、保存、開き直しも成功した。拒否は 0 件だった。
- 全 part（各パッケージに現れるすべての関係の型を許可した場合）: 26,286 件を検査し、`StillReferenced` は 5,989 件だった。そのうち、「`r:`（Transitional と Strict）と `o:` の名前空間の属性だけを見る規則」なら通るものは **0 件**だった。つまり any-attribute 規則による実際の過剰拒否は、このコーパスでは観測されなかった。

## 判定

**converged: no**。

- Blocker: なし。
- Major: N1 の 1 件（`.rels` の `_rels` ディレクトリ名の大小文字と、Target の %エンコード/IRI を同値として扱わないので、`Ok` のままダングリングが残る）。修正は `is_rels_part` と `source_of_rels_part` の大小文字の扱い、および Target の正規化（または、拒否する方向にしか外れない安全網）で、局所的に済む。
- Minor: N2〜N6。N3 は ADR-0003 を accept するまでに必須。N4 の mutant I は、M2 の保証を支えるガードなので、テストを足すことを強く推奨する。
- M1〜M3 と m1、m2、m3、m5、m7 は解消。m4 と m6 は部分的な解消（N1 と N4 に引き継ぐ）。
