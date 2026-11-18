# PR #63 レビュー（Opus, round 2）

対象: impl/pptx-phase1b-smartart @ 3f303794ddb7ccad9c5a6112772e0b738b9d8b72

## 結論（先に）

round 1 の M1〜M3 と m1〜m6 は、挙動で見てすべて直っている。
SmartArt の data と cache の整合、hyperlink の拒否、Err ⇒ byte-identical、局所性、`drop_drawing_cache` の rels と content type の後始末に、新しい Blocker と Major はない。
残る Major は 1 件で、コードの欠陥ではない。PR が今の main と 12 ファイルで衝突していて、このままではマージできない。
さらに、解消すると main の公開型 14 個の生成名が変わる（ADR-0002 の note の記述が偽になる。#51 はコンパイルできなくなる）。
**converged: no**。main をマージして note を直せば、round 3 は軽い確認で済む。

## 前ラウンドの解消状況

- M1（空の data run が hyperlink の拒否をすり抜ける）: 直った。データ側は `a:r` の index `r` で調べる。空の plain run の後ろにある空の linked run（r=2, k=1）でも `FormatNotCopyable` になり、byte-identical だった（プローブ ZP3）。
- M2（編集後に cache が不整合になる）: 直った。編集した clone で全 node の pairing を前後比較し、崩れれば `SmartArtCacheInconsistent` で拒否する。変異「この比較を外す」はテストで落ちる。
- M3（ガードが未テスト）: X1〜X3, X5〜X7 にテストが付いた。X4 と X8 は同等のテストで代替している。新しい生存変異は m1 に記す。
- m1（同じ node が 1 shape に 2 回現れる）: M2 の事後検査で拒否される（テスト `a_node_shown_twice_in_one_shape_is_refused`）。
- m2（関係型を確かめない）: 直った。`diagramData`、`diagramDrawing`、root の `dsp:drawing` を確かめる。
- m3（codegen の命名）: 直った。home 以外の namespace の要素には常に prefix が付く（`DspShape`, `DgmPoint` など）。main との関係は M1 に書く。
- m4（oracle が弱い）: 改善した。part の増減、node を表示しない shape が不変か、run の pairing、`phldr`、全 setter、drop を見るようになった。残る穴は m2 に書く。
- m5（§7 の未実施項目）: SA4 を追加し、O3 は "Not in this PR" に書かれた。
- m6（API の差）: ADR-0013 の post-acceptance note に明記された。

## Findings

### M1（Major）: main と衝突してマージできず、解消すると main の公開型 14 個が rename される

- 根拠:
  - `git merge-tree --write-tree HEAD origin/main`（main は 8610086）で 12 ファイルが CONFLICT になる。
    - 対象: `ci.yml`, `dml.toml`, `report.txt`, pptx の `README.md`, `manual_check.rs`, `error.rs`, `lib.rs`, `presentation.rs`, `write.rs`, `generated/dml.rs`, ADR-0002, ADR-0013。
    - 原因は、merge-base 1833145 のあとに main へ入った #56、#57、#58 など。
    - GitHub の mergeable は UNKNOWN。
  - main に #63 の `emit.rs` の規則だけを当てて `cargo xtask codegen` を実行し、`pub struct/enum` の名前を比べた。次の 14 型が、View と ViewMut を含めて改名される:
    - dml: `RowIdIdentifier` → `A16RowIdIdentifier`、`ColIdIdentifier` → `A16ColIdIdentifier`（#57）
    - pml: `Section` / `SectionList` / `SectionSlideIdList` / `SectionSlideIdListEntry` → `P14*`
    - pml: `Author` / `AuthorList` / `CommentReply` / `CommentReplyList` / `Point2DType` / `TextBodyType` → `P188*`
    - pml: `PComment` / `PCommentList` → `Comment` / `CommentList`
  - main の crate コードは、これらの生成名を使っていない。ただし **#51** は `pml::SectionList`、`pml::Section`、`pml::SectionSlideIdListEntry` を使っている（`slides.rs:934,946,987,990`）。#63 が先に入ると、#51 はコンパイルできなくなる。
  - ADR-0002 の note は「On `main` at the time of writing no existing name changes」と書いている。main をマージした時点で、これは偽になる。
  - 良い点: main の allowlist と #63 の dgm/dsp 項目の和で codegen を回すと、名前の衝突は出ない。`a:ext` → `Extension`、`dgm:extLst` → `DgmDataModelExtensionList` になる。
- 修正案:
  - `origin/main` をマージする（rebase しない）。`cargo xtask codegen` で `dml.rs`、`pml.rs`、`report.txt` を再生成する。
  - ADR-0002 の note を「この PR のマージで上の 14 型が一度だけ rename される（0.1.0 未公開）」に書き換える。
  - #51 の作者に、prefix 付きの名前（`pml::P14SectionList` など）へ合わせるよう伝える。

### m1（Minor）: hyperlink 拒否のうち 3 経路と、drop の型チェックが未テスト（挙動は正しい）

- 根拠: 手で入れた変異 11 個のうち、生き残ったものは次のとおり。
  - Y5: data 側を `get(r)` から `get(k)` にする。M1 の回帰テストは fixture で r == k なので区別できない。
  - Y6, Y7: `set_paragraph_text` の data 側と cache 側の link 判定を無効にする。
  - Y8: `set_run_text` で、text を持つ cache run の link 判定を無効にする。
  - Y10: `drop_drawing_cache` の `incoming[0].rel_type` 判定を外す。
  - Y2、Y3、Y4（関係型と root の検査）もそれぞれ単独では生き残る。ただし互いに重なる多重防御で、どれか 1 つでも残れば拒否される。
- プローブ ZP1〜ZP4 で、実装が `FormatNotCopyable` を返し byte-identical なことは確認した。
- 修正案: ZP3（空の plain run の後ろの空 linked run）、`set_paragraph_text` で data に link がある場合、`set_run_text` で cache だけに link がある場合の 3 テストを足す。

### m2（Minor）: oracle の drop 判定が独立検証として甘い

- 根拠（`tests/oracle/smartart.py` の drop 分岐）:
  - 消えた part について `ppt/media/` 配下を無条件に許す。slide と共有する画像を誤って消しても通る。
  - `[Content_Types].xml` の Override、slide の `.rels` の `diagramDrawing`、それ以外の part の不変を見ていない。
  - Rust テストは見ているので、実装は正しい。
- 修正案: 消えた media について、入力でほかに参照する part がないことを判定する。content types と slide rels と、それ以外の part の byte 一致を加える。

## 確認して問題なしとしたもの

- **テストと CI**:
  - `cargo test -p ooxml-pptx --test smartart` は 19/19 通った。
  - CI（3f30379）は test、clippy、`codegen --check`、python-pptx T3、SmartArt oracle（16 cases）がすべて緑。
- **sweep と oracle**:
  - 手元のコーパス（102 decks）で sweep を実行した。18 SmartArts、229 nodes、229 consistent、261 paired runs で、`set_text` は 229 件すべて ok。
  - その出力に oracle を実行すると 18 cases ok だった。
- **エスケープと再編集**: `& < " _x0041_ \t \n` を入れても、data と cache が一致し、`cache_report` は consistent 5/5 のまま。続けて `set_run_text` しても ok（ZP5）。
- **namespace**: `dsp` = `http://schemas.microsoft.com/office/drawing/2008/diagram`、`dgm` = drawingml/2006/diagram で、取り違えはない。cache は slide の rels から解決し、`relId` も slide の rels で引いている。
- **トランザクション**:
  - 事後の整合検査は `stage_part` より前にある。拒否はすべて byte-identical だった（既存テストとプローブ）。
  - cache の part は、slice があるときだけ stage する。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 2 / nit 0）

M1 は main のマージと再生成、note の修正で閉じる。コード側の Blocker と Major はない。round 3 では、マージ後の `codegen --check`、生成名の差分が上の 14 型に限られること、テストの緑を確かめれば足りる。
