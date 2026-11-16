# PR #45 レビュー（Opus, round 1）

対象: `core/insert-with-ns` @ `2a76520`（`NsDecl`、`*_with_ns` insert、check twin、ADR-0002 note）。
検証は作業用 clone（scratchpad `review-pr45/wt`）で行った。プローブは `review-pr45/probe_pr45.rs`、mutant は `lens.rs` への一時置換で作った。

## 結論（先に）

設計は妥当である。新しい要素だけに宣言を置くこと、束縛済み URI を再宣言しないこと、既存の束縛を決して覆い隠さないこと、の 3 点はいずれも正しい。プレフィックス選択、default namespace、予約プレフィックス、URI のエスケープ、T2 局所性、Err ⇒ 不変、check と apply の一致は、いずれもプローブで確認した。

ただし次の 2 点が残る。

- **M1:** NCName 検査が XML の Name 文字より緩い。そのため整形式でない part を書けてしまう。
- **m1:** ADR-0011 rev 3 が求める「check twin が実際に使うプレフィックスを返す」を満たしていない。

このほか、main（#40）とのコンフリクトがあり、mutant が 2 体生き残る。**未収束**とする。どの修正も小さいので、round 2 で収束する見込みである。

## Findings

### M1（Major）`is_ncname` が XML の NameStartChar/NameChar ではなく Unicode の alphabetic/alphanumeric で判定している。整形式でない出力が `Ok` になる

- 該当箇所: `crates/ooxml-core/src/lens.rs` の `is_ncname`（L228-232）。
- 根拠（`probe_ncname`）: 次のプレフィックスで `insert_with_ns` が `Ok` になる。
  - `"a²"`、`"ª"`、`"µ"`、`"Ⓐ"`、`"a¼"`
  - 出力例は `<a²:graphic xmlns:a²="urn:a"/>`。
  - 厳格なパーサー（Python expat）は、これらをすべて `not well-formed (invalid token)` で拒否する。
  - XML 1.0 第 5 版でも次の理由で不正である。
    - U+00AA/B2/B5/BC は NameStartChar（`[#xC0-…]` から始まる）にも NameChar にも入らない。
    - U+24B6 は除外帯 `#x2190-#x2BFF` に入る。
- 自前のパーサーはこれらを受理する。そのため、テストの「`Ok` の出力が再パースできる」というオラクルでは検出できない。
- README は「no primitive can write a part that is not well-formed」と保証している（`InvalidXmlChar` の項）。M1 はこの不変条件の穴である。実際の呼び出し側（`a`/`pic`/`a16`）は定数を渡すが、公開 API が NCName を検査すると明言している以上、修正が必要である。
- 推奨（どちらか）:
  - (a) ASCII の `[A-Za-z_][A-Za-z0-9_.-]*` に限る。Office のプレフィックスはすべて ASCII なので、これで足りる。
  - (b) XML 1.0 第 5 版の NameStartChar/NameChar の範囲表を実装する。
- テストには `"µ"`、`"a²"` の拒否を加える。

### m1（Minor）check twin が `Result<()>` を返す。ADR-0011 rev 3 §「What else core needs」1 の要件「returns the prefixes actually used」と合わない。入れ子の dry run もできない

- 根拠: ADR-0011（plan-docx-phase2）L208-210 は、`check_insert_*_declaring` が slot、order、limits、depth と prefix choice を検査し、さらに実際に使うプレフィックスを返すことを求めている。PR の `check_*_with_ns` は `Result<()>` を返す。
- 実害は小さい。子孫のプレフィックスは `choose_element_prefix`/`choose_attr_prefix` が自動で選ぶので、docx 側がプレフィックス名を知らなくても `r:embed` は書ける（`probe_unused_decl_and_attr_later`）。ただし ADR との食い違いは、どちらかに寄せて解消する必要がある。
- プレフィックス選択の規則も ADR-0011 の文面と違う。
  - ADR-0011 は「preferred が未束縛ならそれを宣言する」と書く。この規則では、URI が別のプレフィックス `x` で束縛済みでも `xmlns:a` を追加することになる。
  - PR は ADR-0002 §3 に従い、束縛済みの URI を再利用する。テストでは `<x:graphic/>` になる。
  - PR の挙動のほうが良い（Word も、ルートに `a` があれば再宣言しない）。ADR-0011 の文面を PR に合わせるべきである。
- ADR-0011 L327-328 は「Every insert of the operation is dry-run with the check twin before the first write」と書く。しかし `a:graphic` や `pic:pic` は、まだ存在しない `wp:inline` の下に作る。lens の check twin では、外側の 1 段しか事前検査できない。
- 推奨:
  - (a) 戻り値を `Result<Vec<(String /*prefix*/, String /*uri*/)>>`（実際に書く宣言、または各 `NsDecl` に対応する採用プレフィックス）にする。`InsertPlan.declarations` を返すだけで済む。または ADR-0011 の要件を「`Result<()>`、プレフィックスは core が子孫で自動選択する」と改める。
  - (b) ADR-0002 note か README に、「入れ子の作成の dry run は `XmlDocument` の clone（`#[derive(Clone)]`）上で全手順を実行して行う」と書き、ADR-0011 側でもそれを参照する。

### m2（Minor）main とのコンフリクト（`mergeable: CONFLICTING`）

- 根拠: #40 のマージ後、`crates/ooxml-core/src/lib.rs`（`WellFormedness` と `NsDecl` の re-export）と `crates/ooxml-core/README.md`（`NamespaceNotInScope`/`NamespaceDeclaration` の段落）が衝突する。
- 解消は機械的で、両方を残せばよい。
- 試験的にマージしたものの結果は「検証」節に記す。
- README では、#40 の文言「Phase0 never declares namespaces」を捨て、PR 側の文言と `NamespaceDeclaration` の説明の両方を残す。
- 方針どおり `origin/main` をマージし、通常の push で更新する。

### m3（Minor）mutation 耐性: 2 体が生き残る

- **生存 1:** `plan_declarations` の `bound` 判定から `effective(&scope, *p) == Some(d.uri)` を外した版（覆い隠された束縛も「束縛済み」とみなす）でも、全テストが通る。
  - 結果は誤出力ではなく、誤った拒否（`NamespaceNotInScope`）になる。
  - 追加テストの例（`probe_shadowed_binding`）: `<wp:inline xmlns:x="urn:pic"><a:graphic xmlns:x="urn:z"><a:graphicData/>` の `graphicData` に `insert_with_ns::<Pic>([p=urn:pic])` を行うと、`<p:pic xmlns:p="urn:pic"/>` になる。現行実装ではこのとおりになることを確認した。
- **生存 2:** `bound → continue` を検証の前に移した版（束縛済み URI の要求では、プレフィックスを検証しない）でも、全テストが通る。
  - parity テストでは check と apply が同じように変わるので、検出できない。
  - 拒否テストの文書はどれも `urn:a` を束縛していない。
  - 追加テストの例: `xmlns:a="urn:a"` を持つ文書で `NsDecl::new("xmlx", A)` を渡すと、`InvalidNamespaceDeclaration` になること。
- **殺された mutant:** 次の 3 体は既存のテストで検出された。PR 本文の 5 体と合わせて妥当である。
  - URI の重複検査を外した版
  - NCName の先頭に数字や `-` を許した版
  - 要素の束縛がないとき `new.first()` に fallback する版
- 再パースのオラクルが緩い点は M1 に書いた。

### n1（Nit）`get_or_insert_with_ns` は、既存の子があると不正な `decls` でも `Ok` を返す

- `probe_get_or_insert_invalid_decl_existing`: `NsDecl("xml", "")` でも、check と apply の両方が `Ok(())` になる。
- check と apply は一致しており、doc の「decls apply only when inserted」とも矛盾しない。
- それでも、呼び出し側のバグが文書によって出たり出なかったりする。先に検証するか、doc に「既存の子があるときは検証しない」と明記するとよい。

### n2（Nit）fallback プレフィックスが要求の順序に依存する

- `[a=A, p1=urn:r, p=urn:pic]` を `xmlns:p` が束縛済みの文書に渡すと、`p1=urn:r`、`p2=urn:pic` になる（`probe_order_dependence`）。
- 順序を入れ替えると、PR のテストのとおり `p1=urn:pic`、`p11=urn:r` になる。
- 決定的なので害はない。doc に「要求順に決める」と 1 文足せば足りる。

### n3（Nit）`mc:Ignorable` について

- 本 PR の用途では `mc:Ignorable` は不要である。
  - **ADR-0011:** `a`/`pic`/`wp`/`r` は ECMA-376 の標準名前空間なので、Ignorable の対象ではない。
  - **ADR-0013:** `a16:rowId` は `a:extLst/a:ext` の中にある。拡張リストは `uri` で読み飛ばされるので、Ignorable は要らない。
  - コーパスでの実測（`a16:rowId` を含むスライド 18 枚）: 18/18 が `a16` を要素上でだけ宣言し、ルートに `mc:Ignorable` 自体がない（0/18）。
- 汎用 API としての注意点もある。`w14`/`w15` のように拡張リストの外に置く非標準の名前空間を宣言する場合、旧版 Office との互換には、祖先の `mc:Ignorable` にそのプレフィックスを載せる必要がある。これは ADR-0015 m7 と同じ問題である。
- 本 API は Ignorable を扱わない。ルートの開始タグを汚せないので、扱えない。ADR-0002 note に「宣言しても ignorable にはならない。拡張リストの外で Office 2007 が知らない名前空間を使う場合は呼び出し側の責任（該当するなら拒否）」と 1 文足すとよい。
- 既存束縛を覆い隠さないので、祖先の `mc:Ignorable` が参照するプレフィックスの意味が変わることはない。これは確認した。

### n4（Nit）他の PR との相互作用（本 PR の欠陥ではないが、note に書く価値がある）

- **#18 `detach_as`:** 部分木の中に `xmlns` 宣言があると `DetachRefusal::NamespaceDeclaration` で拒否する。そのため、本 API で作った部分木や、Office 自身の `a16:rowId` ext は複製できない。ADR-0013 の行複製は `a:extLst` をスキップし、`insert_with_ns` で作り直すことになる。ADR-0002 note の「#18 extension point」は insert 側にしか触れていないので、この点を書き添えるとよい。
- **#44 `move_in`:** 移動元と移動先の親スコープで、部分木が使うプレフィックスの束縛を比べる。部分木の根が自分で `xmlns:a` を宣言していても `a` を「使用」に数える。そのため、移動先に `a=同じ URI` があるだけで `NamespaceBinding` になり、誤って拒否される。安全側なので、#44 側の課題として記録する。
- **#26 `insert_at`:** 同じ plan（`plan_insert` に slot と index を渡す形）で `_with_ns` の twin を足せる。構造上の障害はない。

## 確認して問題がなかった点

- 宣言は新しい要素の開始タグだけに書かれる。親の `own_dirty` の規則は従来どおりである。T2 については、挿入点より前のバイトが同一であることを PR のテストで確認した。
- 覆い隠された束縛では正しく新規宣言する（`probe_shadowed_binding`）。default namespace だけで束縛されている場合は、要素を無接頭辞にしたうえで、属性用にプレフィックス付きの宣言を追加する。
- `xml`/`xmlns` の URI、`xml*` のプレフィックス（大小文字を問わない）、空のプレフィックス、空の URI は、すべて拒否される。
- URI 中の `" & < > ' \t \n \r` は `&quot;&amp;&lt;…&#9;&#10;&#13;` とエスケープされ、再パースすると元の値に戻る（`probe_uri_escape3`）。
- check と apply は同じ `plan_insert` を通る。brute force の 294 件で一致した。`Err` の場合は `is_dirty() == false` で、バイトも変わらない。
- `set_attr`/`remove_attr` で `xmlns` を書けないことは、#40 の `check_attr_write` で担保されている。マージ後もこの API が唯一の宣言経路である。
- `Error` は `#[non_exhaustive]` なので、新しい variant を足しても破壊的変更にならない。

## 検証

- PR head: `cargo test --workspace --all-features` は 199 件すべて通過した。
- `origin/main` を試験的にマージし、コンフリクトを両方残す形で解消した。`cargo test --workspace --all-features` は 209 件通過、失敗 0 だった。

## 判定

converged: **no**。M1（NCName 検査）、m1（check twin の戻り値と ADR-0011 の文面の整合、入れ子の dry run の方針）、m2（main のマージ）を直し、m3 のテスト 2 件を足せば、round 2 で収束する見込みである。
