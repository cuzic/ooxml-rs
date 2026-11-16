# ADR-0013 レビュー（Opus, round 3）

対象: PR #33 `docs/adr/0013-pptx-phase1b-tables-and-smartart.md` の revision 3（`c09e8b8`）。round 2（`3cd4aa3`）からの差分は ADR 本文だけである。
プローブは `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr33-r3/` に置いた（`rev3.py`、`flagonly.py`、`dd.py`、`sref.py`、`sa4.py`）。round 2 の `edge.py` と入力一覧 `files.txt` を再利用した。

## 結論（先に）

N1（Major）は解消した。revision 3 の規則をそのまま実装したモデルは、論理オラクルとすべて一致した。

- 「結合属性は挿入位置の既存セル（行なら下、列なら右）から写す。書式は上（左）から写す。span が 1 になったら属性を外す」という規則で検証した。
- round 2 の乱択（seed 1）で、`insert_row` 80,165 件の不一致は 0 だった。
- 今回は次の 3 つも、行とは独立に実装して確かめた。いずれも不一致は 0。
  - `insert_column` 80,048 件
  - `delete_row` 56,110 件、`delete_column` 56,119 件
  - 2 次元結合を flag-only 形にした入力 160,090 件（edge form への変換を含む）
- PowerPoint 製の `tbl-cell.pptx` と `sc066-001.pptx` で、結合の内側に入るすべての挿入位置を試した。行も列も well-formed で、オラクルとも一致した。
- §7 には、挿入位置 `at = r0+1` と乱択の property test が入った。

N2–N9 も、文面のうえではすべて反映されている。#44 と #45 の参照（API 名、拒否の一覧、バイトを保つこと、パスが古くなること）は、ブランチの実装と ADR-0002 の注記に正確に一致する。ADR-0012 §3 の「session」の定義も、ADR-0012 側の記述と一致する。

新しい指摘は Minor 3 件と nit である。

- **P1**: `a16:rowId`/`colId` を型付きで挿入するための `ChildOf<a:ext>` が、コード生成で作られない。#45 だけでは足りず、ADR-0002 への追記が要る。
- **P2**: `drop_drawing_cache` は #38 の挙動に依存する。#38 の round 1 で出た M3、m6、M6/m10 が未解決のままである。
- **P3**: N8 の「空の run は対応付けで無視する」がデータ側だけに掛かっている。このため、run を空にする編集の次の編集が `SmartArtCacheInconsistent` で拒否される。

## 解消状況

| round 2 | 状態 | 確認内容 |
|---|---|---|
| N1 結合属性を上（左）から写す誤り（Major） | **解消** | §2 の総論と表の 3 行、Q3、§7 を直した。乱択のモデルと PowerPoint 製の 2 ファイルで、オラクルとの不一致は 0（上述）。文言に nit が 1 つある（P4-a） |
| N2 `remove_parts` の allowed types | 解消（P2 あり） | `[diagramDrawing]` だけになった。`incoming_relationships` で先に確かめる。`stage_part` を使い、`kept_targets` も扱う。#38 の `remove_parts` の doc（関係が「残るパートから集合へ向かう」こと、集合の中から出る関係は検査なしで一緒に消えること）とも一致する |
| N3 名前空間プリミティブ | 解消（P1 あり） | `insert_with_ns` と `NsDecl { prefix, uri }` は #45（`2a76520`）の実際の API である。作る要素、`a:ext@uri`、`[write]` の追加、session、`rowId`/`colId` を別の値空間とすること、いずれも書かれている |
| N4 run の子要素 | 解消（P4-b） | 書式の子要素も写す。`r:*` を持つ run は拒否する。母数 193/431 も書いた（`sa4.py` で再現した） |
| N5 表のアンカー | 解消 | `*MkLst` を 5 つとも挙げた。「リストの名前にかかわらず、`rowId`/`colId` か `*Mk` を含めばロック」も入った。フレーム全体のアンカーはロックしない。T4 に合成入力 2 種を足した |
| N6 span が 1 になったとき | 解消 | origin と edge のどちらでも属性を外す。delete の乱択で不一致 0 |
| N7 `[detach]` と move | 解消 | 到達する complex type の一覧は後続 PR で出す。#44 の拒否（`IntoItself`、`NamespaceBinding`、`IgnorableNamespace`、`XmlSpace`、`TooDeep`）、バイトを保つこと、パスが古くなることは、`move_subtree.rs` と ADR-0002 の注記（`a743d9f`）に一致する。§5 には `NotMovable` が入った |
| N8 空のスライス、空の run | 解消（P3） | 空のスライスは `SmartArtCacheInconsistent` にまとめた。データ側の空の run を無視すると 431/431 段落になる（`sa4.py`） |
| N9 罫線 API | 解消 | covered セルは結合範囲として扱う。範囲外の隣のセルの線もそろえる。オラクルは書いた辺に限る。GUID は大文字小文字を区別せずに比べる |

## 新しい指摘

### P1（Minor）`a16:rowId`/`colId` を入れる `ChildOf<a:ext>` が生成されない

- §2 と §3 は、`[write]` に `a16:rowId`/`a16:colId` と `a:extLst`/`a:ext` を足し、`insert_with_ns` で作ると書いている。#45 の `*_with_ns` は、どれも `C: ChildOf<E>` を要求する。
- ところが SDK データの `a:CT_OfficeArtExtension/a:ext` は、particle が `Any` 1 つだけで、`Children` は空である（`schemas_openxmlformats_org_drawingml_2006_main.json`）。
  - ADR-0002 の注記「wildcard parents」は、親の SDK `Children` から `ChildOf` を作る規則である。`Children` が空なので、`a:ext` については何も生成されない。`a:graphicData` の `Children` には `a16:CT_Identifier/a16:rowId` があるが、`a:ext` の方にはない。
  - `overrides.toml` の `add_child` は、particle の中で名前の付いた項目を `after`/`before` の基準にする（`xtask/src/model.rs`）。`Any` しかない particle には使えない。
  - #45 のテストは、手書きの `ElementSpec` を使っている。生成コードの経路は試されていない。
- 推奨:
  - §2 に、コード生成への追加（たとえば、wildcard の親に子を足す override。根拠はコーパスの 152/135 件）を ADR-0002 への追記として requested と書く。
  - `RowIdNotSupported` を外す条件を「#45 とこの追加」にする。
  - `a:ext@uri` で中身が決まる（`uri` ごとに子が 1 つ）ことも、そこで書く。

### P2（Minor）`drop_drawing_cache` が #38 の未解決の指摘に依存している

#38 の round 1 レビュー（`core-add-part/docs/reviews/pr38-round1.md`）のうち、§4 の手順に関わるものは次のとおり。

- **M3**: 読めない `.rels` を `remove_parts` が「言及なし」とみなす（fail-open）。
- **m6**: `Package::incoming_relationships` は、読めない `.rels` を黙って飛ばす。そのため手順 1 の「ちょうど 1 本」は、読めない `.rels` が描画を指していても成り立ってしまう。`SmartArtCacheShared` の判定が fail-open になる。
- **M6/m10**: fragment や解決できない target を扱えない。`kept_targets` に偽の項目（フォルダ名）が入る。このため、手順 3 の「`kept_targets` は今も参照されているパートであり、孤児は残らない」は言い過ぎになる。
- **M2**: dirty でない木を `stage_part` すると、元のバイトが書かれる。手順 2 は clone を lens で編集するので dirty になり、これには当たらない。

コーパスでの確認（`dd.py`、`sref.py`）:

- SmartArt の描画を持つファイルに、読めない `.rels` は 0 件。
- 描画パート自身の Internal 関係 7 件は、すべて画像である。7 件ともデータパートからも参照されている。つまり、削除の集合は常に描画パート 1 つで、`kept_targets` は 7 件になる。
- target が存在しないものは 0 件。
- スライドの `diagramDrawing` 関係 78 件のどれも、スライドの属性からは参照されていない。そのため `StillReferenced` に当たることはない。

推奨:

- 「Depends on」と §4 に PR #38（Open）を書き、`drop_drawing_cache` の実装を #38 のマージ後とする。
- 手順 1 で、読めない `.rels` が 1 つでもあれば拒否すると書く（#38 の m6 が直るまでの安全側の規則）。
- 手順 3 に加える target は「存在するパート」に限る。
- 「nothing is left orphaned」は「#38 の M6/m10 の修正後」と限定する。

### P3（Minor）空の run を無視する規則がデータ側だけなので、次の編集が拒否される

- §4 は、テキストが空のデータ run（`<a:r><a:t/></a:r>`）を対応付けで無視すると書く。キャッシュ側の空の run については書いていない。
- たとえば run のテキストを `""` にする編集をすると、データ側の run とキャッシュ側の対の run がどちらも空になる。次の編集では、データ側は n−1 本、キャッシュ側は n 本と数えられ、不変条件 (3) で `SmartArtCacheInconsistent` になる。
- 推奨:
  - 空の run は両側で無視する。`sa4.py` では、両側で無視しても結果は 431/431 で変わらない。
  - 対応付けの添字は「空でない run の中での添字」と書く。
  - 空の run に文字を入れたとき、キャッシュのどこに新しい run を作るか（直前の空でない run の後ろ）を書く。

### P4（nit）

- (a) §2 の「the copy is `vMerge` + `gridSpan` in the origin's column」は 2 次元結合の場合である。1 次元の縦結合では `vMerge` だけになる。「`gridSpan` as the cell has it」とする。
- (b) §4 の「A run whose `a:rPr` holds an `r:*` attribute」について。`r:id` は `a:rPr` 自身ではなく、子の `a:hlinkClick`/`a:hlinkMouseOver` にある。「`a:rPr` の部分木に」と書く。拒否の範囲（テキストだけの編集も拒否するのか、書式の写しだけを拒否するのか）も書く。
- (c) §5 の `Core(…)` に、#45 の `InvalidNamespaceDeclaration` を足す。

## 判定

**Minor の修正だけで Accepted: yes。**

唯一の Major だった N1 は、規則・表・§7 のすべてで解消した。乱択（行・列の挿入と削除、edge form と flag-only form）でも、PowerPoint 製のファイルでも、オラクルとの不一致は 0 だった。P1–P3 はどれも局所的な追記で直る。P1 と P2 は、前提条件と依存関係の書き漏れである。ADR 自身が「#44、#45 と `[detach]` の追加がそろうまで Accepted にしない」と書いているので、その条件に P1 のコード生成の追加と、`drop_drawing_cache` についての #38 を加えれば足りる。

出典: #44 `core/move-subtree`（`b31c786`、`a743d9f`: `crates/ooxml-core/src/lens/move_subtree.rs`、`error.rs` の `MoveRefusal`）、#45 `core/insert-with-ns`（`2a76520`: `lens.rs` の `NsDecl`/`*_with_ns`、ADR-0002 の注記。worktree はマージ途中で衝突しているので、コミットから読んだ）、#38 `core/add-part`（`bdc133d`: `package/add.rs` の `incoming_relationships`/`remove_parts`/`RemovedParts::kept_targets`、`docs/reviews/pr38-round1.md`）、ADR-0012 §2/§3（`impl-pptx-phase2`）、SDK データ `schemas_openxmlformats_org_drawingml_2006_main.json`（`a:ext`、`a:graphicData`）、`xtask/src/model.rs`（wildcard の子、`add_child`）。
