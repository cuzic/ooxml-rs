# PR #14 レビュー（Opus, round 1）

対象: `core/remove-part`（81ab625）。`Package::remove_part` / `check_remove_part`、`Error::RemovePartRefused`、`RemovedPart`、README と `opc` のドキュメント、`tests/package.rs`。
照合した文書: ADR-0001 Layer 3（main, Accepted）、ADR-0003 rev 2（main, Proposed）と rev 3 草稿（`impl-xlsx-phase1`）の §3.1 と §4.3、`crates/ooxml-core/README.md`。
プローブ: scratchpad `review-core-14-16/core-remove-part/crates/ooxml-core/tests/probe14.rs`（P1〜P8）と、mutant 2 件。

## 結論（先に）

**未収束（Major 3 件）。** 基本の経路は正しい。calcChain の削除、Default に触れないこと、外部関係の除外、番号を振り直さないこと、L1 の素通し、Replaced なソースの走査は確認できた。ただし次の 3 点が残る。

- **M1**: 「まだ参照されているなら拒否」の検査に穴がある。VML の `o:relid` と、文字参照で書かれた `r:id` が漏れる。そのため `Ok` のまま、ダングリングした rId を持つパッケージができる（破損）。
- **M2**: `check_remove_part` が `Ok` を返した後でも、`remove_part` が部分的に変更した状態で **panic** する経路がある。これは「check が Ok なら remove は成功する」という契約と、ADR-0003 §3.1 の Poisoned 方式の両方に反する。
- **M3**: Accepted の ADR-0001（"never remove"）に対する例外を main に入れているのに、その根拠が Proposed の ADR-0003 しかない。マージ順の条件を明記する必要がある。

修正はどれも局所的で、設計の方向を変える必要はない。

## 指摘

### M1 [Major] 参照検査が `r:` 名前空間の生バイト一致だけなので、rId の参照を見落としてダングリングを作る

**根拠**
- `package.rs:487-509` の `refers_to_rel_id` は、`R_NS` に解決される属性で、**エスケープを解かない生の値** `&*a.value` が `id` と一致するものだけを数える。
- P1（`probe14.rs::p1_vml_o_relid_dangling`）: `xl/drawings/vmlDrawing1.vml` に `<v:imagedata o:relid="rId1"/>` を置き（`o` = `urn:schemas-microsoft-com:office:office`。xlsx の VML はこの形で画像を参照する）、その `.rels` の `rId1` が `../media/image1.png` を指すようにした。結果は次のとおり。
  ```
  remove_part("xl/media/image1.png") = Ok(RemovedPart { relationships: [("xl/drawings/_rels/vmlDrawing1.vml.rels","rId1")], .. })
  vml は o:relid="rId1" のまま、.rels は <Relationships …></Relationships>
  ```
- P8（`p8_escaped_rid`）: ソースが `r:id="rId&#57;"`（値は `rId9`）のとき、`remove_part("xl/linked.xml")` は `Ok` になり、`rId9` が削除される。
- ADR-0003 §4.3 が ADR-0001 に足すとしている note の文面は「only if no part in the package references the entry's id」である。この不変条件を API 自身が守れていない。xlsx Phase1 が消すのは calcChain だけだが、`remove_part` は汎用の pub API で、doc（`package.rs:258-261`）も「i.e. nothing still refers to it」と保証している。

**提案**
- 比較は unescape した値で行う（`a.unescape_value()` か `normalized_value`）。
- 名前空間の集合に VML の `urn:schemas-microsoft-com:office:office` の `relid` を足す。もっと安全側に倒すなら、「ソース part のどの属性でも、unescape した値が Id と一致すれば拒否」にする。誤って拒否する方向にしか外れないし、calcChain の rId はどこからも参照されないので Phase1 には影響しない。
- 回帰テストに P1 と P8 を足す。

### M2 [Major] `check_remove_part` が Ok でも、`remove_part` が変更の途中で panic する

**根拠**
- `package.rs:352-355`: `_rels/.rels` を編集した後、`Relationships::parse(..).expect("re-parsing an edited, previously valid relationships part")` を呼ぶ。plan（`plan_removal`）は、編集後の `_rels/.rels` が `Relationships::parse` を通るかを確かめていない。
- P4（`p4_panic_after_lens_edit_of_package_rels`）: `xml_part_mut("_rels/.rels")` の lens で、別の `Relationship` の `Type` を `remove_attr` する（公開 API だけでできる）。その後 `remove_part("docProps/core.xml")` を呼ぶと次のようになる。
  ```
  panicked at crates/ooxml-core/src/package.rs:354:22:
  re-parsing an edited, previously valid relationships part: Package("<Relationship> lacks Id, Type or Target")
  ```
  この時点で `_rels/.rels` の木からは要素がすでに削除されている。`catch_unwind` した呼び出し側には、変更が途中まで入ったパッケージが残る。`panic=abort` ならプロセスが落ちる。
- PR 本文と doc（`package.rs:272-275`）は「`check_remove_part` が Ok なら `remove_part` は成功する」と書いている。ADR-0003 rev 3 §3.1 も「apply の Err は Poisoned にする」と書いているが、panic はこの仕組みを素通りする。

**提案**
- plan の段階で、`_rels/.rels` から要素を除いた後の `Relationships` を作って検証し、その結果を `RemovalPlan` に持たせる。apply では代入するだけにする。
- または、`package_rels` の再計算を `XmlDocument` からではなく、既存の `package_rels` から該当 Id を除く形にする（parse しないので失敗しない）。
- P4 をテストにする。

### M3 [Major・手続き] Accepted の ADR-0001 に対する例外を、Proposed の ADR を根拠に main へ入れる

**根拠**
- main の ADR-0001 L360-362 は「may never remove or renumber an existing one」のままである。
- ADR-0003 は main では **Proposed（revision 2）** である。PR が参照している rev 3（`check_remove_part`、§3.1 validate-then-apply）は `impl-xlsx-phase1` ブランチにしかない。
- それでも、`opc.rs:3-5` と README の API 表は「ADR-0003 §4.3 による例外」と書いている。PR 本文の Notes にも「ADR-0001 への note は ADR-0003 の accept 時に足す」とある。そのため、#14 が先にマージされると、main のコードと Accepted ADR が矛盾した状態になる期間ができる。ADR-0003 が rev 4 以降で変わった場合、その状態が固定される。

**提案（どちらか）**
- (a) #14 のマージを ADR-0003 の accept の後にする。PR 本文に「merge after ADR-0003 acceptance」と書き、ADR-0001 の post-acceptance note を同じ PR か直後の PR で足す。
- (b) 先にマージするなら、ADR-0001 に暫定の post-acceptance note を足す。「`remove_part` は ADR-0003（Proposed）§4.3 の文面どおりの狭い例外。ADR-0003 が不採択なら API を削除する」とする。

### m1 [Minor] 暗黙の参照（関係の型で引く part）は検査されないのに、doc は「何も参照していない」と保証している

- P2（`p2_implicit_refs`, `shared_formulas.xlsx`）: `check_remove_part` は `xl/styles.xml`、`xl/sharedStrings.xml`、`xl/theme/theme1.xml` のどれにも `Ok(())` を返す。sharedStrings を消すと `t="s"` のセルが壊れる。
- 汎用 API なので禁止までは要らない。ただ、`package.rs:258-261`、README、ADR-0003 の note 文面に「検査するのは明示的な Id 参照だけ。型で引かれる暗黙の参照は呼び出し側の責任」と書くべきである。

### m2 [Minor] エラーの型と検査範囲が doc と一致しない

- Replaced なレジストリは `Error::Package` を返す（`package.rs:370-375`。テストの `tests/package.rs:316` も `Error::Package` を期待している）。一方、doc（`package.rs:262-263`）、README、PR 本文はこれを `RemovePartRefused` の条件として挙げている。
- 検査範囲は「変更が要るレジストリ」ではなく、**すべての** `.rels` である（`package.rs:291-297`）。P5: 無関係な孤立 `zz/_rels/orphan.xml.rels`（root が `<notRels/>`）があるだけで、calcChain の削除が `Err(Package("zz/_rels/orphan.xml.rels: root is not <Relationships>"))` になる。ADR-0003 §4.3 は「every registry part **to change** is editable in place」と書いている。
- 提案: 対象を含まない `.rels` の parse 失敗と Replaced は、走査をスキップするか、`RemovePartRefused` に統一する。対象を含む Replaced な `.rels` は `RemovePartRefused` にする。

### m3 [Minor] `Err` でも part の状態が Raw から Parsed に変わる

- P5: 拒否された後、`_rels/.rels` と `xl/_rels/workbook.xml.rels` が `Raw` から `Parsed { dirty: false }` に変わっている。保存結果（L1）は変わらず、ADR-0003 §3.1 も「検査のための parse は許す」としているので、破損ではない。ただし `state()` から観測できる。doc の「nothing was changed」は「出力は変わらない」と言い換えるのが正確である。

### m4 [Minor] 「part 名は ASCII で大小文字を区別しない」と書いているが、実装は一部で区別する

- `package.rs:265` の doc はそう書いているが、`find`（`package.rs:140`）は完全一致である。`remove_part("XL/calcChain.xml")` は `PartNotFound` になる。
- 自分の `.rels` の有無の検査（`package.rs:286`）も完全一致である。P3: `xl/_rels/FOO.xml.rels` を持つ `xl/foo.xml` は `Ok` で削除され、孤立した `.rels` と、その参照先の `xl/bar.xml` が残る。
- doc を実装に合わせるか、自分の `.rels` の検査を `eq_ignore_ascii_case` にする。安全側に倒すなら後者がよい。

### m5 [Minor] 重複した Override は最初の 1 つしか消えない

- `package.rs:331` の `find_map` と `opc.rs:66-70` の `remove_override` は、最初に一致したものだけを消す。
- P6: `/xl/calcChain.xml` と `/XL/calcchain.xml` の 2 つがあると、後者が残る。保存された CT には、存在しない part への Override が残り、`content_type("xl/calcChain.xml")` も `Some("a/c")` を返し続ける。
- 不正な入力ではあるが、すべての一致を消すのは簡単である。

### m6 [Minor] テストの穴

- `_rels/.rels` 側の削除（`package_rels` キャッシュの再計算、`package.rs:352-355`）を通るテストがない。この分岐を `if false &&` にした mutant でも `cargo test -p ooxml-core` は全件 green だった。`content_types.remove_override` を消した mutant は既存テストで検出された。
- 次のケースも試験していない: 1 つの `.rels` に同じ part への関係が複数ある、複数の `.rels` から参照されている、M1 の 2 ケース、M2 のケース。

### m7 [Minor・任意] `RemovePartRefused.reason` が `String` である

- 呼び出し側（xlsx の validate）は、拒否の理由で分岐できない。理由を enum にする（`Registry`、`HasOwnRels`、`StillReferenced { source, id }`、`NotEditable`）と、format crate がエラーを対応づけやすい。`#[non_exhaustive]` の enum なので、後から変えても互換性は壊れない。

## 確認できた点（指摘なし）

- Default には触れない。外部関係（`TargetMode="External"`、似た Target を持つもの）は残る。残りの Id は振り直されない（`shared_formulas.xlsx` のテスト、P1、P6）。
- 削除するインデックスを降順に処理し、直前の空白テキストもまとめて消す。隣り合う要素でも、間に空白がある場合でも、index はずれない（`xml/mod.rs` の `remove_root_child_with_leading_ws` を読んで確認）。
- 触らなかった entry は raw-copy される（既存テストで name、method、CRC、圧縮バイト、順序を比較している）。
- ソース part が Replaced の場合は、置き換え後のバイトを走査して拒否する（P7）。ソースが XML でない場合は「cannot scan」で拒否する（安全側）。
- check と remove の間でパッケージが変わっても、`remove_part` は plan をやり直すので、古いインデックスは使わない。
- `ContentTypes` キャッシュを更新しないと既存テストが落ちる（mutant で確認）。

## 判定

**converged: no**。Blocker はない。Major は M1、M2、M3 の 3 件。M1 と M2 はコードとテストの局所的な修正で直る。M3 はマージ順の条件か、ADR-0001 への暫定 note で解消する。
