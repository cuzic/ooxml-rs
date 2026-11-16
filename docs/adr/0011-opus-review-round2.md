# ADR-0011 レビュー（Opus, round 2）

- 対象: PR #31 `docs/adr/0011-docx-phase2-structure.md` revision 2（7f69749）
- 照合先: `core/add-part`（PR #38, 31906a9, `crates/ooxml-core/src/package/add.rs`, `package.rs`）、`core/insert-at`（PR #26, 631fc90, `lens.rs`）、ADR-0007 rev 2（`plan/docx-phase1`）、ADR-0009 rev 2（94f8783）、ADR-0012 rev 2（4766425）
- プローブ: `scratchpad/review-25r2-31r2/31/probe2.py`（zipfile + ElementTree、`python3 -I`、DTD/実体宣言は拒否、解析のみ）。出力は同じディレクトリの `probe2.out` と `probe2-ns.out`

## 結論（先に）

round 1 の B1、M1、M3〜M6 は解消している。m1〜m12 も大半が解消した。core API を `core/add-part`（PR #38）に一本化した点、レジストリの範囲を「定義 ∪ 参照」にした点、セクション区切りで `w:type` を後半に書くようにした点は、いずれも妥当。

ただし、**M2 の修正（名前空間のローカル宣言）が、core の挿入プリミティブの仕様と噛み合っていない**（N1, Major）。core の `insert_by`/`insert_at` は、要素を作る時点で親のスコープから prefix を決め、見つからなければ `NamespaceNotInScope` で拒否する。一方、ADR が提案する `declare_namespace` は「作成済みノード」に対する操作である。そのため、`a:graphic` や `a:graphicFrameLocks` は、宣言を付ける前に作成そのものが失敗する。文書ルートで `a` を宣言していないファイルは POI 112/113、SDK 440/442 あり、`insert_image` はほぼ全ファイルで動かない。

N1 を直せば、残りは Minor のみになる。

## 解消状況

| ID | 指摘（round 1） | 状況 | 根拠（rev 2 の行） |
|---|---|---|---|
| B1 | §1 の core API が `core/add-part`・ADR-0009・ADR-0012 と矛盾 | **解消**（残りは N2・N5 の Minor） | L13-21, L101-157。`PartPlan` を撤回し、PR #38 を single source of truth とした。`Numbered` の規則（max + 1。dangling target、Override、削除済みの名前、計画済みの名前を避ける）は `add.rs` の `numbered`/`names_in_use` と一致する。Default を追加しない方針は ADR-0009 と一致する。ADR-0012 は `add_default_content_type` を使うが、これは core が両方を提供しているので形式ごとの方針差にとどまり、矛盾ではない |
| M1 | `w:type` を書く `sectPr` が逆。restart 設定が重複する | 解消 | L346-363。後半に `w:type` を書き、`pgNumType/@start` を外し、`lnNumType` を `continuous` にする。manual check（L622-624）にも追加された |
| M2 | `a:graphicFrameLocks` で名前空間 `a` が未宣言になる | **規則は解消。実装経路が未解決**（N1） | L241-258, L161-166 |
| M3 | 「fresh」が定義しか見ていない | 解消（rId は残りあり → N4） | L190-203, L177-179, L600-601 |
| M4 | 非 ASCII のスタイル名から空の ID ができる | 解消（表記に残りあり → N6） | L479-490 |
| M5 | `insert_column` が `gridSpan`/`vMerge` を複製する | 解消 | L442-455。列 0 では右隣から複製し、`hMerge` を含む表と `RowWouldBeEmpty` は拒否する |
| M6 | 行削除で注参照や revision が消える | 解消 | L385-395, L411-413 |
| m1 | 描画 ID の走査範囲。上限を超えたときの扱い | 解消 | L198 |
| m2 | 注釈 ID の和集合 | 解消 | L199。`sdtPr/w:id` を除外し、数値でない値も無視する |
| m3 | numbering（`numIdMacAtCleanup`、挿入位置、`lvlOverride`） | 解消（allowlist に漏れ → N7） | L201, L315, L325-332 |
| m4 | ブックマーク名 | 解消 | L200, L296-299 |
| m5 | 画像の dedup と拡張子 | 解消 | L148-149, L225-236 |
| m6 | ハイパーリンク | 解消 | L269-283 |
| m7 | `tblHeader` のコピー、`merge_cells` の `tcW` | 解消 | L404, L461 |
| m8 | スコープのずれ | ほぼ解消（manual check に残りあり → N8） | L91-97 |
| m9 | `split_run` のゲート分類 | 解消 | L541 |
| m10 | T-OPC の差分判定 | **一部**（N3） | L589-594。content type は差分判定になったが、target の存在チェックは全件判定のまま |
| m11 | 名前空間のプレフィックス衝突 | 解消 | L164 |
| m12 | ブランチを `origin/main` に追従させる | 解消 | rev 2 の時点で merge 済み（`crates/ooxml-docx` あり）。その後に main へ #22、#24 が入ったので、マージ前に再度 merge する |

## 新規指摘

### Major

**N1. 名前空間のローカル宣言（L161-166, L248-258）が、core の挿入 API では実現できない。誰が実装するかも決まっていない**

- 根拠:
  - core の `LensMut::insert_by`/`insert_at`（`core/insert-at` の `lens.rs` 413-458）は、新しい要素の prefix を `choose_element_prefix(root, parent_path, uri)` で親のスコープから決める。該当する宣言がなければ `Error::NamespaceNotInScope` を返す（`lens.rs` 676-708）。`check_insert_at` も同じ検査をする。
  - ADR が提案する `LensMut::declare_namespace(prefix, uri)` は、すでに作成されたノードに対する操作である。そのため、`a:graphicFrameLocks`、`a:graphic`、`pic:pic`、`wp:inline` のように**要素名そのもの**がスコープ外の名前空間に属する場合、宣言を付ける前に作成が失敗する。
  - プローブ（`probe2-ns.out`）で `word/document.xml` のルートに宣言がない件数:

    | 名前空間 | POI | SDK | tpl | d4j |
    |---|---|---|---|---|
    | `a` | 112/113 | 440/442 | 208/210 | 12/12 |
    | `pic` | 109/113 | 440/442 | 199/210 | — |
    | `wp` | 6/113 | 6/442 | — | — |

    したがって、仕様どおりに実装すると `insert_image` はほぼすべてのファイルで `NamespaceNotInScope` になる。
  - §1 の「What else core needs」は「ADR-0002 への additive note」と書いているだけで、担当 PR も、ADR-0007 の dry run（`check_insert_at`）との関係も決まっていない。ADR-0009 と ADR-0012 は新しいパーツを丸ごとのバイト列で作るので、この API を必要としていない。この API を必要とするのは ADR-0011 だけになる。
- 修正案（どちらかを選び、core の担当 PR を指定する）:
  - (a) core に「宣言しながら挿入する」API を追加する。例: `insert_by_declaring::<C>(prefix)` / `insert_at_declaring::<C>(index, prefix)`。新しい要素自身に `xmlns:prefix` を付け、その prefix で要素を作る。`check_*` twin も用意し、prefix の衝突（m11 の `a1` など）は twin の中で解決する。Word の出力形（`a:graphicFrameLocks` と `a:graphic` のそれぞれに `xmlns:a`）を再現できるのはこの案だけである。
  - (b) 宣言する場所を「作成した最も外側の祖先」にする。`insert_image` なら作成した `w:r` か `w:drawing` に、`wp`/`a`/`pic` をまとめて宣言する。こうすると、その後の子の作成は既存の `choose_element_prefix` で通り、必要なのは「作成ノードに属性として `xmlns:*` を足す」API だけになる。Word の形とは異なるが、名前空間として整形式であり、意味も同じになる。
  - いずれの案でも、L248 の規則文と L256-258（`wp` を `wp:inline` に宣言する）を、選んだ方式に合わせて書き直す。属性だけで使う `r`（`a:blip/@r:embed`）は `choose_attr_prefix` の対象で、文書ルートでほぼ常に宣言されているが、規則の対象に含めるかどうかを明記する。

### Minor

- **N2. §1 要件 1（L138-141）の手順をそのまま実装すると、`apply` が必ず `StalePlan` になる。**
  - 「plan は `&Package` を借用するので、保持したまま Layer 2 の dry run を実行する」と書かれているが、実際の `RegistryPlan::new` は借用しない。package の id と generation を記録するだけである（`add.rs` の `RegistryPlan::new`, `check`）。
  - `Package::xml_part_mut` は呼ぶたびに generation を増やす（`package.rs` 374-377）。ADR-0007 の dry run は `LensMut::check_insert_at` を使うので `xml_part_mut` が必要になり、plan が古くなる。
  - 修正案: 手順を「(1) story の dry run（`xml_part_mut`）→ (2) `RegistryPlan` の構築（rId やパーツ名が決まる）→ (3) `Package::apply` → (4) ツリーの編集」に直す。あわせて、ADR-0012 の方式（clone したツリーを `stage_part` で同じ plan に載せ、1 回で apply する）を採らない理由も 1 行書いておく。たとえば「大きな `document.xml` を clone したくないから。(4) は dry run 済みなので失敗しない」。
- **N3. T-OPC「every relationship's internal target exists」（L588）が、入力に含まれる dangling target で失敗する。**
  - ADR 自身が、SDK 19 ファイルに 26 本の dangling target があると書いている（L156）。プローブでも、SDK `TestFiles__InvalidDocProps.docx` に 2 本あった。
  - 修正案: content type と同じく差分判定にする（「この操作で追加された relationship の target が存在する」）。「relationship Id は大文字小文字を無視して一意」も、入力に対する差分として書く。
- **N4. rId の「参照も避ける」が、要件なのか提案なのかがはっきりしない。**
  - L152-154 は「should」とし、core の rules table に委ねている。しかし T-REG（L600）は §2 の全レジストリについて「定義と参照のどちらにもない」ことを検証すると書いており、§2 の rId 行は「core」としている。
  - PR #38 の `next_relationship_id` は `.rels` の Id しか見ていない。
  - 修正案: どちらかに決める。
    - Phase2 では要件にせず、T-REG の対象から rId を外す（実測では 0 件）。
    - ooxml-docx 側で、割り当てられた Id がソースパーツの `r:*` 属性値と一致したら拒否する。
  - あわせて、L154 の「core compares case-sensitively」は古い。現在の core は `eq_ignore_ascii_case` で比較し、大文字小文字だけが違う Id も避けている（`add.rs` の `next_relationship_id`）。この記述は削除する。
- **N5. PR 番号が古い。** L13-14、L116、L697、L748 が「PR pending」のままになっている。PR #38 はすでに open で、ADR-0009 rev 2 も「PR #38」と書いている。番号に置き換える。
- **N6. 「`a` + 最小の空き番号」が Word の形だという記述（L482-485）は正確ではない。**
  - Word は数字のほかに英字の接尾辞も使う。プローブでは `aa`/`ab`/`ac` 形の styleId が POI 33、SDK 27 あった（POI `bug65649.docx`、SDK `Comment015.docx`）。
  - 衝突は §2 の範囲で検査しているので、正しさには影響しない。「Word に似た形（Word 自体は `a0`…`a9`, `aa`… も使う）」と表現を弱める。
- **N7. `restart_list` は `lvlOverride` を `detach_as` でコピーする（L325）が、§11 の `[detach]` 追加（L564）に `w:CT_NumLvl/w:lvlOverride` がない。** 追加するか、コピーの手段を変える。
- **N8. §12 の manual check に「a new header on section 2 of 3」（L626）が残っている。** L91-97 と Q6 で Phase2 の対象外にした操作なので削除する。
- **N9. セクション区切りの後半は、S と同じ見た目を保てない場合がある。** L350 が保証しているのは前半だけで、後半について次の 2 点を明記しておく。
  - **`w:titlePg`**: 後半にも残るので、区切りの直後のページに先頭ページ用のヘッダーが出る（Word で区切りを入れた場合と同じ動作 *(u)*）。`titlePg` を持つ `sectPr` は POI 6、SDK 71、tpl 12 ある。
  - **注の番号の振り直し**: `numRestart=eachSect`/`eachPage`（SDK の `sectPr` 20、settings 3）があると、区切りの後で脚注の番号が変わる。structure 列（L532）は `NOTEREF` を許しているので、このとき `NOTEREF` の結果が古くなる。コーパスに `NOTEREF` は 0 ファイルなので、structure 列から `NOTEREF` を外すか、注の番号を振り直す設定がある場合に限って外すのが安全側である。

## 判定

**Minor のみなら Accepted: no.** N1（名前空間宣言の core API の形と担当）を決めれば、残りは Minor（N2〜N9）だけなので、修正後に Accepted にできる見込み。N1 の (a)/(b) どちらを選んでも、ADR-0011 の他の節への影響は L161-166 と L248-258 に収まる。
