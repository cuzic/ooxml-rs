# PR #63 レビュー（Opus, round 3）

対象: impl/pptx-phase1b-smartart @ 6ad83d5

## 結論（先に）
converged: yes。M1 は解消している。main にあった生成物の公開名は 1 つも消えておらず、改名もない。新しい prefix 規則は `dml` の `dgm:`/`dsp:` だけに適用される。
merge の解消では、main の pptx の挙動（resolver、表、Phase 2 の slide 操作）と #63 の挙動が両方残っている。
m1 と m2 も実質的に直っている。round 2 で生き残った変異 Y5〜Y8 と Y10 は、いずれも新しいテストで落ちる。残りは nit 2 件だけ。

## 前ラウンドの解消状況
- M1（main と衝突、既存名 14 件の改名）: 解消。`git diff origin/main...6ad83d5` で dml.rs の `pub` 行を数え比べた。main にある行は 1 本も減っておらず、追加は SmartArt の 40 項目（`Dgm*`/`Dsp*`、`DataModelExtension`、`PointValues`、`NS_DGM`/`NS_DGM14`/`NS_DSP`）だけ。既存の `TextBody`/`TextBodyView`/`TextBodyViewMut` の impl は main と byte 一致する。dml 以外の生成物（pml.rs など）は変わっていない。sml.rs の差分は #61 による main 側の変化。
- M1 の ADR-0002 note: round 1 の「グループ外の namespace は常に prefix」という一般規則は取り下げられた。代わりに「SmartArt の `dgm`/`dsp` だけ prefix を付け、既存名を保つ」と書かれ、`xtask/src/emit.rs` の実装（`g == "dml"` かつ prefix が `dgm`/`dsp` のときだけ prefix を付け、衝突数の数え上げからも外す）と一致する。
- m1（hyperlink 拒否の 3 経路と drop の型チェック）: 解消。手で入れた変異の結果は次のとおり。いずれも適用してから元に戻した。
  - Y5（data 側 `get(r)`→`get(k)`）: `writing_into_an_empty_linked_data_run_is_refused` が落ちる（r=2, k=1 に直された）。
  - Y6/Y7（`set_paragraph_text` の data 側/cache 側の判定を `false` にする）: `paragraph_text_refuses_links_in_either_part` が落ちる。
  - Y8（`was_text` の cache run の判定を `false` にする）: `run_text_refuses_a_link_on_the_paired_cache_run` が落ちる。
  - Y10（`incoming[0].rel_type` の判定を外す）: 新しい unit テスト `drop_refuses_when_the_only_incoming_relationship_has_the_wrong_type` が落ちる。
- m2（oracle の drop 判定が甘い）: 解消。`check_drop` は次を確かめるようになった。
  - 消えた part は、drawing とその rels のほかは `ppt/media/` 配下に限る。しかも drawing だけが参照し、ほかの rels から参照されないものに限る。
  - slide rels は、入力から該当 `Id`（型 `diagramDrawing`）を 1 本だけ除いたものと構造が一致する。
  - `[Content_Types].xml` は、消えた part の Override を除いたものと一致する。
  - data は、`dsp:dataModelExt` を持つ `a:ext` を除き、空になった `dgm:extLst` も除いたものと一致する。
  - それ以外の part は byte 一致する。

## merge 解消（`git show --remerge-diff 142135d`）
- `presentation.rs`: main の `slides`/`main`/`tables` フィールド、`table_style_ids`、`deck()` が残っている。#63 が切り出した `comment_refs` を `slide_mut` と `smart_art_mut` が共用し、`slide_mut` は `table_styles` と `tables` を従来どおり渡している。
- `write.rs`/`lib.rs`/`error.rs`: `mod table` と `mod smartart`、両方の re-export、両方の Error variant が残っている。SmartArt の `Guard` には `table: false` が足された（SmartArt frame は表ではないので妥当）。
- `names.rs`: main の `REL_DIAGRAM_DATA`（`od_rel!` マクロ）と `REL_DIAGRAM_DRAWING` を使い、smartart.rs のローカル定数を削った。値は同じ。
- CI、README、manual_check、ADR-0013 の note、allowlist: 両側を連結している。#63 の allowlist にあった `a:bodyPr`/`a:lstStyle` は main 側にすでにあるので落とした（重複回避）。
- `cargo test -p ooxml-pptx --test smartart` は 21/21、`--lib` は 10/10 が通った。CI（6ad83d5）は両方の job が緑。
- main はその後 #61 が入って進んでいるが、`git merge-tree` で 6ad83d5 と衝突しないことを確認した。

## Findings
### n1（nit）: ADR-0002 note の適用範囲の書き方
- 根拠: note には「SmartArt elements in the diagram and diagram drawing namespaces get `Dgm` and `Dsp` prefixes」とある。しかし実装の条件は「`dml` の `dgm`/`dsp` entry すべて」で、SmartArt に限らない。また、SmartArt 用でも `a:` の entry（`a:CT_DataModelExtension/a:ext` → `DataModelExtension`）と単純型の enum（`PointValues`）は従来規則のままで、prefix が付かない。日付と「no change to the decisions」の定型も、ほかの note と揃っていない。
- 修正案: 「`dml` の `dgm:`/`dsp:` entry は常に prefix を付け、衝突数の数え上げから外す。`a:` entry は従来規則のまま」と 1 文で書く。

### n2（nit）: open PR との干渉（参考）
- 根拠: #77 は `xtask/src/emit.rs` の同じ箇所（名前を決める行）を書き換えていて、後からマージする側で衝突する。#66 は `codegen/allowlist/dml.toml` と `report.txt` で、#76 は `lib.rs` と `manual_check.rs` で、それぞれ文面が衝突する。どれも両側を残せば済む。ただし #77 の `views` による検査は、`smartart` の prefix を付けた後の名前に対して行う必要がある。
- 修正案: 後からマージする側で、上の点を確かめてから `codegen --check` を通す。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）
