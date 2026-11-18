# PR #63 レビュー（Opus, round 1）

## 対象/方針

- 対象: PR #63 `impl/pptx-phase1b-smartart` の `035c468`（ADR-0013 §4 SmartArt text、Accepted revision 4）。
- 照合した仕様:
  - ADR-0013 §4・§5・§7、revision 4 の表（P2・P3・P4b）
  - ADR-0002 の新しい post-acceptance note
  - `RegistryPlan::stage_part` / `remove_parts` の doc（#38 round 3 の staging 規則）
- 方法:
  1. `git clone` したコピー（scratchpad）にチェックアウトし、コードを読んで ADR の各規則と対応させた。
  2. 合成入力のプローブ（`poi/SmartArt.pptx` を zip 上で書き換えたもの）で挙動を確かめた。
  3. コーパスの sweep と Python oracle を再実行した。
  4. 自分で選んだ変異を `tests/smartart.rs` に当てた。
- PR の worktree はこのファイル以外変えていない。
- 重大度:
  - Blocker/Major: 黙ったデータ不整合、ADR が明示する拒否の欠落、データ損失を防ぐガードが未テストのもの。
  - それ以外は Minor。

## 結論（先に）

設計の骨格は ADR どおりで、堅い。

- data↔cache の対応（`presOf` の destination = `dsp:sp@modelId`、`destOrd`→`srcOrd`、`dgm:t` なし = 空段落 1 つ）は正しい。空 run を両側で無視する規則も正しい。
- 変更前の 3 不変条件、`phldr` の除去（`phldrT` は残す）も正しい。
- clone 2 つを 1 つの `RegistryPlan` でまとめて入れる原子性も正しい。
- `drop_drawing_cache` の順序（読めない `.rels` → `incoming_relationships` → `stage_part` + `remove_parts`）も正しい。
- 再現結果:
  - コーパス数値は **431 paired runs / 587 成功 / 61 拒否**（48 unresolved + 12 inconsistent + 1 hyperlink）がそのまま再現した。
  - oracle は 97 + 5 件 ok、`cargo test --workspace` は緑、CI も緑。
  - main を試しにマージした木でも `cargo xtask codegen --check` は通る。
  - 生成コードの公開名は merge-base から一つも消えていない（semver 上の rename なし）。

ただし Major が 3 件ある。

- **M1**: 空の data run に書くとき、hyperlink の拒否をすり抜ける（ADR §4 P4b 違反）。
- **M2**: 同じ経路で、編集が成功したあと cache が不整合になりうる。その後の編集はすべて拒否される。
- **M3**: 拒否やデータ保全のガードを外す変異 8 件が、既存テストをすべて生き残る。

判定は末尾。

## Findings

### Major

**M1 — 空の data run への `set_run_text` が hyperlink run の拒否をすり抜ける**（`write/smartart.rs` L319–L335, L544–L548）

- 原因: `check_links` の `Some((p, Some(k)))` は「text を持つ run の k 番目と k-1 番目」を調べる。ところが `was_text == false` のとき、実際に書かれる data run は `a:r` の r 番目（空 run）で、k 番目は**その次の**text run になる。
- 再現: `rPr` に `a:hlinkClick r:id` を持つ空 run（`<a:t/>`）に `set_run_text(NODE, 0, 1, "LINK")` を実行した。
  - 結果は `Ok`。data にはリンク付きで "LINK" が入り、cache にはリンクなしの run が足される。
  - `cache_report` は consistent のまま（不変条件 (3) は属性だけを比べ、`hlinkClick` は子要素なので）。
- ADR §4 は「書き込む run の `a:rPr` subtree に `r:*` があれば拒否」と定めている。
- 修正案: `which` に data の run index `r`（全 `a:r` 中）も渡し、data 側は `runs[r]` を調べる。cache 側は従来どおり、format を写す元の run（k-1 番目、k=0 なら先頭）を調べる。テストも足す。

**M2 — 空の data run を埋めると、cache が黙って不整合になりうる**（L329–L335）

- 原因: 新しい cache run は、ADR どおり直前の cache run の書式を写す。しかし data 側の空 run は**自分の** `rPr` を保つ。だから、空 run が直前の run にない属性を持つと、編集直後に不変条件 (3) が破れる。
- 再現: data が "A" + `<a:r><a:rPr lang="en-US" b="1"/><a:t/></a:r>` の状態で `set_run_text(NODE,0,1,"B")` を実行した。
  - 結果は `Ok`。その後の `cache_report` は consistent 5→4 になる。
  - 次の `set_text` は `SmartArtCacheInconsistent("a data run's format is not on its cache run")` で拒否される。
- ADR §7 の SmartArt oracle は「編集後、すべての shape で cache 不変条件が成り立つ」ことを要求している。sweep は `set_text` しか試さないので、ここは検出されない。
- 修正案（どちらか）:
  - (a) staging の前に、編集済みの clone で `slices()` を全 node について再実行し、不整合なら拒否する。これは m1 も同時に防ぐ。
  - (b) 写す元の cache run の属性に、空 data run の非 proofing 属性が含まれていなければ `SmartArtCacheInconsistent` / `FormatNotCopyable` で拒否する。
- ADR の P3 規則（直前の run の書式を写す）自体の盲点でもあるので、post-acceptance note に一行書くのがよい。

**M3 — 拒否やデータ保全のガードの多くが未テスト**（変異 8/8 が生存）

PR の 14/14 とは別に、私が選んだ変異を `tests/smartart.rs` に当てた（ハーネスは、`clear_placeholder` を除く既知の変異が落ちることで確認済み）。結果は全部生き残った。

| # | 変異 | 守っているもの |
|---|---|---|
| X1 | `check_links` の k-1（format を写す元の run）の判定を削除 | P4b の「format を写す元」の拒否 |
| X2 | `drop_drawing_cache` で、drawing だけが参照する part を集めない | ADR §4 step 3（drawing 専用の画像が孤児になる）。プローブで実装が正しいことは確認済み |
| X3 | `remove_data_model_ext` で常に `dgm:extLst` ごと消す | **他の `a:ext` の黙った損失**。プローブで実装は正しい |
| X4 | `incoming[0].source` / `rel_type` の判定を削除 | `relId` が diagramData など drawing でない関係を指すときのガード（現状は core の `RelationshipTypeNotAllowed` が二段目で止める） |
| X5 | `clear_placeholder` を空 text でも実行 | ADR m8（空 text では `phldr` を触らない） |
| X6 | `set_text_range` の挿入位置を body の末尾にする | aggregate shape で、node の後ろに他の node が続くときの段落の位置 |
| X7 | `insert_run_after` の k=0 で format を写さない | 「段落の先頭 = 先頭 cache run の書式」 |
| X8 | `start.is_none()` を外す（同じ node の最後の出現を使う） | m1 と関連 |

- 最低限 X1–X5 には fixture テストが必要。
- 特に X3 は、他の拡張を持つ `dgm:extLst` に対する `drop_drawing_cache` で、拡張が残ることを確かめるべき。
- X6 は、3 source の aggregate で、中央の node に 2 行を入れるテストで押さえられる。

### Minor

- **m1 — 同じ node が 1 つの shape に 2 回現れる場合**（`presOf` が 2 本で `destOrd` が別）
  - `slices()` は最初の出現だけを slice にする。編集は `Ok` で、2 回目は古いまま残る（consistent 5→4）。
  - コーパスにはない合成入力だが、黙った不整合になる。拒否するか、全出現を編集すべき（M2 (a) でまとめて防げる）。
- **m2 — `dataModelExt@relId` の関係型と part を確かめていない**（L227–L230）
  - `relId` が slideLayout や data part 自身を指しても、"cache" として解決される。
  - `dsp:sp` がないので、編集は data だけに入って `Ok` になる（stale cache と同じ状況を黙って作りうる）。
  - `rel_type == diagramDrawing` と root `dsp:drawing` を確かめ、外れたら `SmartArtCacheUnresolved` にすべき。`dgm:relIds@r:dm` の型も同様（root の検査はあるので軽微）。
- **m3 — codegen の命名規則は rename 問題を先送りしただけ**
  - 衝突がないため、今回 `dsp:` の要素が `dml::Shape` / `ShapeTree` / `GroupShape` / `Drawing` という素の名前を取った。
  - 後で `a:sp` / `a:grpSp`（SDK の `Drawing.Shape` / `Drawing.GroupShape`、lockedCanvas）などを allowlist に足すと、「home namespace が素の名前」規則によって `dml::Shape` → `DspShape` の rename が起きる。ADR-0002 の note が防いだと言う問題そのものである。
  - 名前も紛らわしい（`dml::Shape` が `dsp:sp`）。
  - 公開直後の今のうちに、home 以外の namespace の要素は常に prefix を付ける（`DspShape`, `DspShapeTree`, `DspGroupShape`, `DspDrawing`、`Dgm*` も）ことを勧める。そのうえで note の記述を正確にする。
  - 既存の公開名の変化は、上記のとおりない。
- **m4 — Python oracle が弱い**
  - `n not in zout.namelist(): continue` のため、出力から**消えた** part を見逃す。追加された part も見ない。
  - node を表示しない cache shape が不変かどうかを見ない（drawing part は丸ごと比較から外れている）。
  - text しか比べないので、不変条件 (3)（run の対応と属性）や `phldr` を見ない。
  - 試すのは `set_text` だけで、`drop_drawing_cache` や run/段落の編集は扱わない。
  - 「独立」と言うには、少なくとも消えた part と他 shape の不変は判定すべき。
- **m5 — ADR §7 の項目が未実施なのに "Not in this PR" に書かれていない**
  - SmartArt oracle の LibreOffice（O3 workflow）による描画と PDF text の検査。
  - 手動確認 Q2（長い text を入れて兄弟の font size を見る）。
- **m6 — API が ADR §4 と違う**
  - ADR は `ShapeRef::smart_art()`（`doc` point、`parOf` の `srcOrd` 順の木）と `SmartArtMut::node_mut(model_id)` を定めている。
  - 実装は `SmartArtMut::nodes()`（文書順）と node 単位の setter 3 つ。
  - post-acceptance note に API は書かれているが、「no change to the decisions」とある。差分（read model の不在、順序）を明記すべき。

### 確認して問題なしとしたもの

- **対応規則**: 空 run を両側で無視し、run index は text を持つ run だけを数える。`text_runs` の index は L80 で、フィルタの前に `enumerate` している。data の `rPr` 属性 ⊆ cache の属性（proofing の 5 属性は除外）。空の data run を埋めると、直前の cache run の後ろにその書式で入る（既存テストと X7 以外）。
- **トランザクション**: data と drawing を clone で編集し、`stage_part` を 2 回呼んでから `apply` を 1 回。`xml_part` での parse は `RegistryPlan::new` より前なので、stale にならない。sweep の全拒否で byte-identical を再確認した。
- **`drop_drawing_cache`**: 読めない `.rels` で拒否する。同じ slide からの 2 本目の関係でも `SmartArtCacheShared`。drawing 専用の画像は消え、data と共有の画像は残る。`dgm:extLst` に他の ext があれば `extLst` を残す（プローブ P5–P7、P6）。
- **`cache_report`**: 実行後の `to_bytes` は byte-identical だった。
- **`set_text_range`**: whole-body の場合は `set_text` と同じ経路。workspace の全テストが緑。
- **codegen**: main とマージした木で `codegen --check` が通る。merge-base と比べて dml/pml/sml/wml の公開名の消失はない。

## 再現

- probe: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr63/probe/zz_probe.rs`（examples に置いて実行。P0–P10）。
- sweep: 631 decks（633 との差は入力ディレクトリの組み合わせ）。103 SmartArts、600 nodes、408 shown、396 consistent、431 paired runs。587 ok、48+10+2+1 refused。
- oracle: sweep の出力 97 件 ok、fixture cases 5 件 ok。

## 判定

**converged: no** — Major 3 件（M1 hyperlink の拒否漏れ、M2 編集後の不整合、M3 ガードが未テスト）。M1・M2 の修正と、X1–X6 相当のテスト追加のあとに round 2。
