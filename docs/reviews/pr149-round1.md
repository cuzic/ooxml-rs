# PR #149 レビュー（Opus, round 1）

対象: `impl/core-ac-rank-u1` @ 9f122a74c9b0a1b9a07970c4964f9acc9f50d0d5

## 結論（先に）
core の区間の規則（§3）、append と位置指定の区別（§4.1–§4.2）、`insert_at` の読み方（§4.3）、上限の数え方（§5）は ADR のとおりに実装されている。ランダムに作った AC 入りの親 4 万件で全分岐の組み合わせを総当たりしたが、`Ok` で順序や上限が壊れる例と、`Err` で出力が変わる例は見つからなかった。AC ビット、公開 API、生成物、§4.4 の呼び出し元のテストも ADR の要求を満たしている。
Blocker と Major は無い。Minor 3 件と nit 2 件は、どれも挙動を壊さない。
converged: yes。CI は 5 ジョブとも緑（test/clippy/codegen、LibreOffice、python-docx、python-pptx、openpyxl）。

## 前ラウンドの解消状況
初回のレビューなので対象なし。

## 検証したこと

### 1. 配置の正しさ（総当たりのオラクル）
`tests/lens.rs` と同じスロット表 `head?(0) (a|b){0..2}(1) item*(2) ext?(3) ##other*(4)` で、使い捨てのテストを書いた（コミットしていない）。
- 生成した入力: 親の子は 0〜4 個。子は直下の要素、コメント、`zz`（未登録）、`o:z`（ワイルドカード）、AC のいずれか。
- AC: `Choice` を 1〜2 個持つ。`Fallback` は無し、空、要素ありのどれか。分岐の中身は 0〜2 個で、1 段だけネストした AC を含む。
- `Requires`: 束縛済みの `t`、`mc:Choice` 自身で宣言した `s`、未束縛の `u`、属性なしのいずれか。
- 試した操作: `insert`/`check_insert`、すべての index への `insert_at`/`check_insert_at`、`insert_by(|i, _| i == k)`（k = 0..3）。挿入する子は `head`/`a`/`b`/`item`/`ext`。
- 判定: 分岐の選び方の全組み合わせを展開し、順序と上限を確かめた。append と callback では、未登録の要素は直前の要素に付くものとして扱った（ADR-0002 §4.3）。
- 結果: 40,000 回の試行で問題は 0 件。
  - 元の文書が全組み合わせで正しいのに、`Ok` の結果が不正になる例は無かった。
  - `Err` で出力が変わる例、`check_*` と本体の判定が食い違う例、形の崩れた AC（未束縛の接頭辞、`Requires` なし）で `Ok` になる例も無かった。
  - append の成功は 4,993 件。位置を決められるのに append が拒否した例は 1,274 件あった。そのうち `zz` を含まない例は、すべて ADR §3 の unranked（空のネスト AC だけを持つ分岐）だった。`zz` を含む例は、先頭の未登録要素で `lo = cur` になる設計どおりの拒否。
- 既存の `alternate_content_placement_agrees_with_expanding_each_branch` も同じ種類のオラクルで、決め打ちの 8 ケースを確かめている。

### 2. 上限
分岐の中は合計し、分岐をまたいでは最大を取る。ネストした AC は再帰的に数える（`alternate_content.rs` の `branch` と `read`）。総当たりでは `head`/`ext` の上限 1、`a`/`b` の合計 2 を含めて違反は無かった。`bug68703.pptx` の `get_or_insert::<Transition>` は `AlreadyPresent` を返し、出力は byte-identical（`deckread.rs`）。

### 3. 位置指定の拒否
- §4.4 の grep を再実行した: base では 86 か所、head では 70 か所。差の 16 か所は `check_insert` への切り替えで、§4.4 の表と一致する。
- 呼び出し元を関数単位にまとめ、PR のテストと突き合わせた。各関数に、同じスロットに AC を置いて拒否させ、byte-identical を確かめるテストがある。
- docx の slot をまたぐ 2 例は、`insert_image` と `insert_hyperlink` の両方でテストされている（`images.rs`）。
  - `[pPr, r0, AC{Choice/}, r1, r2]` / `After(r1)`
  - `[AC{w:pPr}, r0, r1]` / `After(r0)`
- 全分岐が空の AC は、core のテストでは位置指定の全形式で拒否される（`alternate_content_position_modes`）。append は通る。

### 4. AC ビット
- 保持: `insert_child`、`splice_children`（`inspect` で追加する要素ごと）、`set_children` が立てる。パーサーは `close` の後に `set_children` を呼ぶ（`parse.rs:201-205`）。`close` は `FLAG_BIT` を残して offset だけを書く。
- マスク: `close_start` を読むのは `CompactSpan::expand` だけで、`& MAX_SPAN_OFFSET` している。作成したノード（`close_end == 0`）にビットが立っても、`close` は `None` のまま。`structurally_eq` は span を見ない。
- サイズ: `size_of::<RawNode>() == 64` の assert がある（64-bit）。
- 古い 0 が残る経路: 子を追加する経路は上の 3 つしかない。`child_mut`/`children_slice_mut` で丸ごと置き換える経路も、AC へ改名する経路も無い（`apply_prefixes` は接頭辞だけを変える）。子を切り離したコピーと移動は、AC を含むと拒否される。
- 古い 1 が残る経路: `insert_at` は全体を走査する側に回る。AC が無ければ、遠くの順序違反は無視する（`disorder`）。`removing_alternate_content_restores_local_unlimited_slot_checks` がこれを確かめている。
- `debug_assert` は fast path にある。

### 5. Err ⇒ byte-identical と T2
- core の拒否（straddle、unranked、malformed、同じスロットへの位置指定、分岐を通した上限）はすべて計画の段階で決まる。テストでも総当たりでもバイト一致した。
- T2: `alternate_content_interval_boundaries_and_locality` が、AC と後ろのコメントのバイトが変わらないことを確かめている。
- docx `insert_column` は、ステージした複製に書き、全行が成功したときだけ置き換えるようになった。base には、途中の行で拒否されるとグリッドが書き換わったまま残る経路があり、これが塞がった。

### 6. 書き換えた既存テスト
PR 本文に挙がった 8 件を 1 件ずつ見た。緩めただけのものは無い。
- `malformed_…`、`insert_at_places_…`、`insert_at_limits_…`: `<mc:AlternateContent/>`（Choice なし）は malformed。§4.3 の 1 により、離れた位置にあっても拒否が正しい。
- `p7_calc_pr_…`、`r9_…`: 拒否の理由が未束縛の `Requires` に変わった（m1 を参照）。
- `refusals_preserve_every_byte`: `extLst` が分岐の中にあるので `AlreadyPresent` が正しい（§5）。
- `f6_…`（c）: 以前の入力は `Fallback` だけで、新しい規則では malformed になる。正しい形の `Choice` に直したうえで、拒否する操作を増やしているので、テストは強くなった。
- pptx `unsupported_paragraph_content_is_refused`: AC のケースを追加しただけ。

### 7. API と生成物
- `crates/*/src` と `xtask` の差分に、`pub` の追加や削除は無い。
  - 新しい `InsertPosition`、`APPEND`、`alternate_content` モジュールは非公開。
  - `may_have_alternate_content` と xlsx の `refuse_alternate_content` は `pub(crate)`。
- `crates/ooxml-schema` に差分は無い。CI の `codegen --check` は緑。
- `mce_wrapped`: 名前をスロットに残し、`child_names` からは外す。xtask のテストは absPath に書き直され、スロットに名前が残ることを確かめている。

### 8. 他の open PR
- #144: `lens.rs` と `move_subtree.rs` で衝突する（`git merge-tree`）。§8.1 のとおり、この PR の後で作り直す予定なので問題ない。
- #140: きれいにマージでき、新しい位置指定の呼び出し元も足していない。

### 9. 機械的な検証
- `spikes/ac-rank/check_core.py reference/fixtures` を実行し、全ペアが一致した。
  - docx: 201 ペア、append 可 191
  - pptx: 65 ペア、append 可 27、位置指定可 21
  - pptx の wildcard だけの親: 640 ペア
  - xlsx: 120 ペア、append 可 53、位置指定可 47
- POI 全体（256/411/41）は再実行していない。PR 本文の数値を採用した。
- 自分で入れた変異は 3 件。
  - neighbour 検査の削除と straddle 検査の削除は、どちらもテストが落ちた。
  - `place_skipping` の「AC の前に順序違反があれば拒否」を消す変異は生き残った（n1）。

## Findings

### m1（Minor）: U1 で xlsx の一部の拒否が実際には外れるが、PR 本文に書かれておらず、正の側のテストも無い
- 根拠: `check_insert_by(|_, _| false)` を `check_insert` に切り替え、core の一律の拒否も無くなった。このため、正しい形の AC が隣にある workbook や worksheet への append は、U1 の時点で成功する。
  - プローブ: `p7_calc_pr_next_to_alternate_content` の `x15` を `xmlns:x15` で束縛すると、`set_number` は `Ok` になり、`calcPr` が書かれる。
  - `definedNames` の作成（`set_autofilter`）や `mergeCells` などの worksheet への append も同じ経路を通る。
  - 一方、docx の `append_paragraph` は、U3 まで拒否を保つ format 側の事前検査が足された（`document.rs:660`）。扱いが揃っていない。
  - 書き換えた `p7`/`r9` は、未束縛の接頭辞による malformed の拒否しか確かめていないので、外れたことがテストから見えない。
  - 出力自体は正しいと考える。総当たりのオラクルと、§5 の分岐を通した `AlreadyPresent` で裏付けた。
  - r9 で束縛すると、xlsx の index 側の検査により、従来どおり `UnresolvedSibling` になった。
- 修正案: PR 本文の「実装時の判断」に、U1 で外れる xlsx の拒否（§9 の xlsx Lift 行）を明記する。`p7` には、束縛した `x15` で `Ok` になることを確かめる 1 ケースを足す。docx と同様に U2 まで止める方針なら、その旨を書く。

### m2（Minor）: §4.4 の一部の呼び出し元は、テストの拒否が core の位置指定より前の format 側の検査で起きている
- 根拠: 次のテストは byte-identical を確かめているが、core の `insert_by` や `insert_at` には届いていない。
  - pptx `insert_into_sections`: `SectionsInconsistent`
  - `complete_adjustments`: `InsideAlternateContent`
  - SmartArt `insert_run_after`: `UnsupportedContent | SmartArtCacheInconsistent` の 2 つを許す
  - docx `insert_section_break` の 2 件目: `NotDetachable`
- オラクル 3 の文面（拒否してバイト一致）は満たしている。ただし、将来その format 側の検査を外すと、core の拒否を守るテストが残らない。
- 修正案: その呼び出し元に届く入力を作れるなら、core の `UnresolvedSibling` を期待するケースを足す。作れないなら、「format 側で先に拒否される」とテストにコメントで残す。SmartArt はどちらのエラーかを固定する。

### m3（Minor）: docx `insert_column` が呼び出しごとに本文 part の `XmlDocument` 全体を複製する
- 根拠: `tables.rs:706` の `let mut staged = xml.clone();`。途中で拒否されたときのバイト一致は正しく直った。ただし、大きな文書では列を 1 本足すたびに O(文書サイズ) の複製が起きる。
- 修正案: 各行の `insert_at::<TableCell>` と `get_or_insert::<TableRowProperties>` を、先に `check_*` で dry run する。または、複製を表の部分木だけにする。急がないので follow-up でよい。

### n1（nit）: AC を含む親で、AC より前の順序違反を `insert_at` が拒否する規則がテストされていない
- 根拠: `lens.rs:759` の `if disorder { return Err(…) }` を消す変異が、core のテストと総当たりの両方で生き残った。ADR §4.3 の 1 は、この拒否を列挙していない（malformed、unranked、同じスロット、全分岐が空）。安全側の追加の拒否である。
- 修正案: `[item, head, AC{ext}]` に `insert_at::<Item>(0)` が拒否されるテストを足すか、ADR §4.3 に 1 行足す。

### n2（nit）: `insert_detached` と `insert_same_part` は append だが、位置指定として扱われる
- 根拠: 両者は `…_by(|_, _| false)` を呼ぶので、同じスロットに AC があると拒否される。ADR §4.1 の append の一覧には入っていないので規則違反ではなく、安全側。
- 修正案: doc に「同じスロットの AC では拒否される」と書くか、U4 以降で append モードに揃える。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 3 / nit 2）
