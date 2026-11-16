# ADR-0013 レビュー（Opus, round 2）

対象: PR #33 `docs/adr/0013-pptx-phase1b-tables-and-smartart.md` の revision 2（`1b27d5b`）と spike（`5c5f18c`）。
プローブは `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr33-r2/` に置いた（`edge.py`, `sa3.py`, `ids.py`, `inc.py`, `cm.py`、入力一覧 `files.txt`）。

`results/corpus-sha256.txt` の 631 件はすべてスクラッチから SHA-256 で特定できた。spike をその集合で再実行すると、`results/tables_smartart-619.txt` と件数がすべて一致した（`spike-out.txt`）。差は 2 つだけで、どちらも件数には影響しない。

- 重複スキップの行がない（入力を最初から一意にしたため）。
- 同数のヒストグラム項目の並び順が違う。

## 結論（先に）

round 1 の Blocker 2 件と Major 4 件は、方針としてはすべて解消した。edge form を正規形にした点、構造操作のロック、`a16` と core の前提条件、スライス単位の書式源、辺単位の罫線、いずれも妥当である。Minor 8 件も解消した。

ただし B1 の修正で書いた規則そのものに、Major の誤りが 1 つある。

- **縦結合の中に行を挿入するとき、結合属性を「上のセル」から写すと書いている。** 挿入位置が origin のすぐ下（`at = r0 + 1`）だと、上のセルは origin の行にある。そのため新しいセルが `rowSpan` を持つ 2 つ目の origin になり、表が壊れる（N1）。
- コーパスの縦結合は全部 `rowSpan="2"` なので、「結合の中」への挿入位置は `r0 + 1` しかない。つまりコーパスの全ケースでこの誤りに当たる。
- 「下のセル」から写せば、乱択で作った 80,165 ケースのすべてで論理オラクルと一致する。列方向（「左のセル」から写す規則）も同じ誤りで、「右のセル」から写せば直る。

ほかは Minor である。主なものは次のとおり。

- `drop_drawing_cache` での `remove_parts` の allowed types の読み違い（N2）
- 名前空間プリミティブの呼び順が core の `insert` と合わない（N3）
- SmartArt で run の子要素（`hlinkClick` など）の写し方の規則がない（N4）
- `deMkLst` の中に表のモニカーがある場合の穴と、`tblMkLst` の列挙漏れ（N5）

## 解消状況

| round 1 | 状態 | 確認内容 |
|---|---|---|
| B1 2-D 結合の edge form | **方針は解消。規則に誤りあり（N1）** | §2 で edge form を正規形とし、edge セルにも span の算術を掛け、well-formedness で edge span を検査する。ただし「上（左）のセルの結合属性を写す」は `at = r0+1` で 2 つ目の origin を作る。`delete_row` の規則は乱択の全ケースでオラクルと一致した（span が 1 になったら属性を外す前提。N6） |
| B2 コメントのアンカー | 解消（Minor N5） | `CommentAnchoredTable` を §2、§5、§7 T4 に追加した。セルの属性、幅、高さは許可。ADR-0005 §6 は `sldMkLst` と、オフセットのない `deMkLst` 以外をすべてロックする（default-deny）。そのため `tblMkLst`/`graphicFrameMkLst` も実質的に覆われる |
| M1 `a16` id の生成 | 解消（前提として。Minor N3） | 局所宣言を確認した（`rowId` 152 件すべて）。部分的な id、表内の重複、非数値、`≥2³²` はいずれも 0。`max ≥ 2³¹` の表は 32。id の規則（表内で未使用、セッション中に削除されていない、`max+1`、溢れたら最小の未使用値）は健全 |
| M2 複製と移動のプリミティブ | 解消（前提として。Minor N7） | `[detach]` に `a:tcPr`/`a:bodyPr`/`a:lstStyle` を追加し、`move_subtree` を前提にした。両方がそろうまで「Accepted にしない」と明記している |
| M3 SmartArt の書式源と run の対応 | 解消（Minor N4、N8） | 429/432 と 431/431 を再現した。destOrd の同値は 0（複数ソースの dest は 30 件で、`destOrd` 欠落はなし） |
| M4 共有罫線 | 解消（Minor N9） | `set_border(row, col, Edge, Line)` で両側のセルに書く。結合範囲では外周に書く |
| m1 Q1 の一次情報 | 解消 | Context、Q1、§7 の手動チェック (a)–(c) に反映した |
| m2 `drop_drawing_cache` | 一部（N2） | 手順は明記した。ただし core `RegistryPlan::remove_parts` の意味の読み違いがある |
| m3 built-in スタイル | 解消 | LibreOffice の `predefined-table-styles.cxx` の GUID は 74 個で一致した。コーパスの未定義 id 6 種はすべてこのリストにある |
| m4 空の origin | 解消 | python-pptx `append_ps_from` に合わせた |
| m5 spike | 解消 | 完全な検査、SHA で固定したコーパスを確認し、再現できた（上述） |
| m6 アニメーション | 解消 | — |
| m7 枠サイズ | 解消 | ADR-0016 §6 は未マージ（Proposed）なので、参照にそう添える |
| m8 プレースホルダー | 解消 | — |

## 新しい指摘

### N1（Major）結合の中に挿入するセルが、上（左）のセルの結合属性を写すと 2 つ目の origin になる

- §2 `insert_row(at)` は次のように書く。「Inside a vertical merge (strictly after its origin row, up to its last covered row) the new cells in the merged columns are covered (attributes copied from the cell above, so `vMerge`, plus `hMerge`/`gridSpan` as that cell has them)」。総論も「a newly created covered cell copies the merge attributes of its neighbour … (the cell above for a new row, left for a new column)」と書く。
- `at = r0 + 1` では、上のセルは origin の行にある。その行のセルは origin（`rowSpan`+`gridSpan`）か、edge セル（`hMerge`+`rowSpan`）である。どちらも `vMerge` を持たない。写すと新しい行に `rowSpan` を持つセルができ、origin が二重になる。
- 再現（`edge.py`、PowerPoint 製の `tbl-cell.pptx` slide2）:
  ```
  入力:      gridSpan=2,rowSpan=2 | hMerge=1,rowSpan=2
             gridSpan=2,vMerge=1  | hMerge=1,vMerge=1
  insert_row(1)、上から写す（ADR の文言どおり）:
             gridSpan=2,rowSpan=3 | hMerge=1,rowSpan=3
             gridSpan=2,rowSpan=3 | hMerge=1,rowSpan=3   ← 2 つ目の origin（double owner）
             gridSpan=2,vMerge=1  | hMerge=1,vMerge=1
  ```
- `sc066-001.pptx` の 1 次元縦結合 4 つ（隣り合う 2 列の結合を含む）でも、同じく double owner か範囲外になる。コーパスの縦結合は全部 `rowSpan="2"` なので、「結合の中」に入る挿入位置は `r0+1` しかない。§7 の Edge-form oracle を回せば即座に落ちる。
- 修正: 結合属性は **挿入位置にある既存の行（`at`、つまり挿入後の下のセル）** から写す。「結合の中」なら `at ≤ r0+R−1` なので、その行は必ず被覆行であり、写す属性は常に正しい（origin の列なら `vMerge`+`gridSpan`、それ以外の列なら `hMerge`+`vMerge`）。
  - 1–5 行 × 1–5 列、重ならない結合を最大 4 つ置いた表を乱択で作り、全挿入位置を試した。80,165 ケースで、下から写すと論理オラクルとの不一致は 0、上から写すと 8,759 だった。
  - 書式（`a:tcPr`、`a:bodyPr`、段落）は上のセルから写してよい。結合属性とは別の源と書き分ける。
  - 列は「右（`at`）のセル」から写す。

### N2（Minor）`drop_drawing_cache`: `remove_parts` の allowed types は「残るパートからの入力関係」の型である

- §4 は `remove_parts with the allowed types …/diagramDrawing, image, hyperlink` と書く。core（PR #38、`core-add-part` の `RegistryPlan::remove_parts`）では、`allowed_rel_types` は **残るパートから削除対象へ向かう** 関係の型である。削除対象自身の rels（描画の `image`/`hyperlink`）は「goes with the source's relationships part, unchecked」で、型の検査を受けずに一緒に消える。
- コーパス（`inc.py`）では、描画パートへの入力関係は 75/75 がスライドからの `diagramDrawing` 1 本である。描画パート自身の rels は `image` 7 件、`hyperlink` 1 件。
- 推奨:
  - allowed は `[diagramDrawing]` だけにする。
  - スライドの関係は `remove_parts` が自動で外すので、別手順は要らない（先に `remove_relationship` してもスキップされるので害はない）。
  - `remove_parts` は、別のスライドからの `diagramDrawing` も黙って外してしまう。そのため `SmartArtCacheShared` は、事前に `Package::incoming_relationships(drawing)` が 1 件であることで判定すると書く。
  - 戻り値の `kept_targets`（描画が参照していた画像）を孤児として残すのか消すのかを決める。
  - データパートの書き換えは `RegistryPlan::stage_part` で行うと明記する。

### N3（Minor）名前空間プリミティブの呼び順と、前提条件の参照先

- §2 は「`declare_namespace(prefix, uri)` on a node created in the same edit」と書く。しかし core の `LensMut::insert` は、作成の時点で接頭辞が束縛されていなければ `NamespaceNotInScope` で失敗する（`lens.rs`）。このため「作ってから宣言する」順序では呼べない。**宣言付きで作る**（insert-with-ns）という 1 つの操作にする必要がある。
- 前提条件の参照先も古い。いま core のブランチは `core/insert-with-ns` と `core/move-subtree` に分かれて作業中なので（worktree はどちらもまだ main と同じ）、§2 と表の「requested from the core PR on `core/add-part`」を書き換える。
- 実装に要る詳細も足す。
  - 作る要素: `a:tr/a:extLst/a:ext@uri="{0D108BD9-81ED-4DB2-BD59-A6C34878D82A}"`、`a:gridCol/…/a:ext@uri="{9D8B030D-6E8A-4147-A177-3AD203B41FA5}"`（コーパス 152/135 件で一致）、`a16:rowId`/`colId@val`。
  - allowlist: `[write]` に `a:tr`、`a:tc`、`a:gridCol`、`a:txBody`、`a:extLst`/`a:ext`、`a16:rowId`/`colId` を加える。§3 の allowlist の移動は属性系の要素しか挙げていない。
  - 「セッション」の定義: `Presentation` を開いてから保存するまで。削除した id の記録は表（graphic frame）ごとに持つ。
  - `rowId` と `colId` は別の値空間として扱う。

### N4（Minor）SmartArt: run の子要素の変更を写す規則がない（`hlinkClick` は写せない）

- §4 は「a data `a:rPr` **attribute** change is written onto the paired cache run」とだけ書く。
- `sa3.py` の結果: 対になる run で、データ側の `a:rPr` が子要素を持つものは 149 件ある。そのうち 148 件は、キャッシュ側に同じ子要素がそのまま入っている。残る 1 件は `hlinkClick` で、`r:id` がデータパートの rels と描画パートの rels で別の値になっている（`smartart-hprocess9-four-nodes.pptx`）。
- 推奨:
  - 書式の子要素（`solidFill`、`latin` など）の変更も、対になるキャッシュの run に同じように写すと書く。
  - `r:*` を含む子要素（ハイパーリンク）は写せないので、SmartArt では拒否する（`FormatNotCopyable`、または `SmartArtCacheInconsistent`）。
  - 431/431 のうち、データ側の属性が空でないのは 193 である。不変条件の母数として併記する。

### N5（Minor）表のアンカーの判定: `tblMkLst` の列挙漏れと、`deMkLst` の中の表モニカー

- Open XML SDK のスキーマ（`oac`）には `tblMkLst`、`graphicFrameMkLst`、`tcMkLst`、`trMkLst`、`gridColMkLst` がある。どれも中身は `xsd:any` である。§2 の列挙には `tblMkLst` がない。ADR-0005 §6 は default-deny なので実害はないが、列挙するなら全部書くか、「§6 がロックする frame」とだけ書く。
- §2 の「an anchor naming the frame」は、§6 で **ロックしない** `deMkLst`+`graphicFrameMk`（表全体へのアンカー）まで含むように読める。表全体へのアンカーは構造操作で宙に浮かないので、ロックしないと明記する。
- 穴: `deMkLst` の中身は `any` である。`tcMk`/`trMk`/`gridColMk`（`rowId`/`colId`）を含んでいても、`txMk` と `cp`/`len` がなければ §6 はロックしない。構造操作に限っては「リストの名前にかかわらず、`rowId`/`colId` 属性か `tcMk`/`trMk`/`gridColMk`/`tblMk` 要素がアンカーの下にあればロック」を足す。コーパスで表にアンカーしたコメントは 0 件で（`cm.py`。modern comment は 1 ファイルだけ）、T4 は合成入力に頼ることになる。

### N6（Minor）span が 1 になったときの扱い

`delete_row` は「origin and edge `rowSpan − 1`」と書き、edge だけは「drops its edge `rowSpan`s」としている。origin の `rowSpan` が 1 になったとき（1 次元の縦結合で被覆行を消す場合や、2 次元結合が 1 行に縮む場合）は書いていない。「span が 1 になったら、origin と edge のどちらでも属性を外す」と書く（`edge.py` の delete 側の検証はこの前提で、不一致 0）。列も同じ。

### N7（Minor）`[detach]` 追加と `move_subtree` の仕様の粒度

- 健全性: id は `a:tr`/`a:gridCol` の `extLst` にあり、`a:tcPr` の中にはない。§2 は `a:tr` を丸ごとではなく `@h` だけ写すので、`a:tcPr` の複製で id が重複することはない。
  - `a:tcPr` 内の `blipFill` の `r:embed`、`a:lstStyle` の `defRPr/hlinkClick`、`buBlip`、`extLst` は、`detach_as` が拒否する。これは健全な拒否である。
  - ADR-0002 の注記どおり「到達する complex type のレビュー」が要る。対象は `cell3D`、`headers`、`bodyPr` の `scene3d`/`sp3d`/`normAutofit`、`lstStyle` の 9 レベルなど。追加の PR でその一覧を出すと書く。
- `move_subtree` の検査には次も含める。
  - 移動先が移動元の子孫でないこと
  - `insert_detached` と同じ `IgnorableNamespace`/`InheritedXmlSpace` の対称検査
  - 移動したノードの扱い（元のバイトを保つのか、created として再描画するのか）
  - 既存のパスが無効になること

### N8（Minor）SmartArt の「空スライス」と、空の run による拒否

- 「`dgm:t` の欠落は空段落 1 つ」とし、`CT_TextBody` は `a:p` を 1 つ以上持つ。このため「empty slice」が起こるのはスキーマ違反の入力だけになる。`SmartArtCacheInconsistent` に一本化するか、何を指すのかを明記する。
- 不変条件 (3) で拒否されるのは 2 段落で、どちらも `tdf152606.pptx` である。データ側に空の run（`<a:r><a:t/></a:r>`）があり、キャッシュ側には run がない。空のノードに文字を入れるという典型的な操作が拒否される。「テキストが空の run は対応付けで無視する」とするか、拒否されることを Consequences に書く。

### N9（Minor）罫線 API の細部

- `set_border(row, col, …)` で `(row, col)` が covered セルのとき、結合範囲として扱うのか、`MergedCell` で拒否するのかを決める。
- 結合範囲の外周に書くときは、隣のセル（範囲外）の対になる線も同じ値にすると明記する。
- §7 の「after any `set_border`, no shared edge differs」は、もともと食い違いがある他社製の表（5 辺）では偽になる。「書いた辺」または「入力で食い違いのない表」に限定する。
- GUID の比較で大文字小文字を区別しないこと（built-in リストとの照合）。

## 判定

**Minor の修正だけで Accepted: no。**

N1（縦結合への挿入で結合属性を「上（左）のセル」から写す誤り）は、ADR の文言どおりに実装するとコーパスの全縦結合で表を壊すので Major とする。修正は 1 文（「結合属性は挿入位置の既存行（列）から写す。書式は上（左）から」）で、乱択と PowerPoint 製ファイルで検証済みである。

次の改訂で N1 を直し、N2–N9 を反映すれば、round 3 は N1 の確認だけで収束する見込み。round 1 の B1、B2、M1–M4、m1–m8 は（N1 を除き）解消済みである。

出典: core `RegistryPlan::remove_parts`/`stage_part`/`incoming_relationships`（`/home/cuzic/ooxml-rs-wt/core-add-part/crates/ooxml-core/src/package/add.rs`）、core `LensMut::insert`（`crates/ooxml-core/src/lens.rs`）、ADR-0002 の detach/insert 注記（`core-detach-insert`）、ADR-0005 §6、Open XML SDK data `schemas_microsoft_com_office_drawing_2013_main_command.json`、LibreOffice `oox/source/drawingml/table/predefined-table-styles.cxx`（GUID 74 個）。
