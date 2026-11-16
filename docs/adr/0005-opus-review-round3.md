# PR #15 レビュー（Opus, ADR-0005 round 3 / 実装 round 2）

## 対象/方針

- 対象: PR #15 `impl/pptx-phase0-1`（HEAD `64fbb80`）。`0adf206`（R2-1/R2-5/R2-6/R2-7 のコード）、`5ae39ce`（T3 の正規形モデル、python 側の独立 edit、multi sweep）、`57230f1`（README）、`64fbb80`（ADR-0005 rev 3 = Accepted）。
- 範囲外: Phase 0.1 の copy primitive（#18 と pptx 側ブランチ）。
- 方針: 読み取り専用。worktree はこのファイルを置くだけで、ほかには触れていない。検証はすべて scratchpad（`review-pptx-r3/`）で、`git archive HEAD` のコピーに対して行った。fixture は信頼しないデータとして解析しただけで、実行していない。
- 根拠:
  - **既存テストの実行**: `cargo test -p ooxml-pptx` は全件が通った（lib 3、locality 1、oracle 4、refusals 16、roundtrip 3、doc 1）。`cargo clippy -p ooxml-pptx --all-targets -D warnings` の警告は 0。T2 の stderr: `body set: FormatNotCopyable 156`、`para set: FormatNotCopyable 217`。T3（Rust）: `1393 edits applied and checked`。
  - **python-pptx 1.0.2 での照合**: `examples/oracle_cases` の出力に `check.py` を実行し、`48 cases, 1393 edits, 0 failures`。出力と自作プローブの出力に含まれる XML パート 2,080 個は、すべて整形式だった。
  - **ミューテーションテスト**: 8 種類の誤りを入れ、T2/T3/T4 と `check.py` を回した（下表）。2 回目以降の mutant が古いビルドを使わないように、ソースを `touch` してから実行した。
  - **python 側のミューテーション**: `check.py` の `edit` に誤りを 4 種類入れた（下表）。
  - **const assert の発火確認**: 生成コードの `RunProperties` に `ATTRS_INCOMPLETE = true` を足すと、`ooxml-pptx` のビルドが `E0080 … review R2-6` で止まる。
  - **敵対的プローブ**（別クレート）: `a:p` を持たない本体を 3 形 × 6 操作で試した。新しい段落の `endParaRPr` の中身、末尾が空行の `set_text`、未検査の §6 の条件も確かめた。

## 結論（先に）

**R2-1〜R2-7 はすべて解消した。** 中でも次の 2 件は、ミューテーションで裏付けが取れた。

- **R2-2**: round 2 で T2 も T3 も通過した「残す先頭ランの `a:rPr` の子要素を全部消す」誤りが、今は 2 か所で落ちる。T3（Rust）では `t3_expected_model_after_edits` と `t3_detects_dropped_format_children`、python では `check.py` が 16 failures を出す。`a:pPr` の子要素、`a:endParaRPr` の子要素を消す誤りも、同じく両方で検出される。
- **R2-3**: `check.py` は自分で読んだ結果から期待値を作るようになり、Rust 側と独立した。Rust の書き込みと Rust の `apply` に**同じ誤り**を入れた mutant を 3 つ作った。Rust の T3 を通ってしまうものもあるが、`check.py` は 3 つとも検出した。中でも `new_runs_unformatted_both` は、Rust の T2/T3/T4 を全部通過し、python だけが 18 failures で落とした。`a:endParaRPr` の子要素を消す誤りは multi sweep でしか検出されない（`multi-poi-bug68703`）。本体ごとに複数回編集する網羅実行が、実際に判定力を足している。

修正によって新しく壊れる経路は見つからなかった。

- `a:p` の無い本体: どの操作も panic しない。型付きの `UnsupportedContent` か `ParagraphNotFound` で拒否され、出力はバイト単位で変わらない。
- 新しい段落の `endParaRPr`: スキーマの順序どおりに付く。拒否の判定は書き換えより前に行われる。
- const assert: 対象の要素を正しく覆っており、実際に発火する。

新しい指摘は Minor の 3 件だけ（N3-1〜N3-3）。どれも文言か根拠の書き方の問題で、出力の正しさには影響しない。

判定: **converged: yes / ADR Accepted OK: yes**（N3-1〜N3-3 は Accepted の後に直してよい）。

---

## 解消状況

| ID | 判定 | 根拠 |
|---|---|---|
| R2-1 `a:p` 0 個で panic | **解消** | `body_paragraphs`（`write.rs` L180-188）が `UnsupportedContent("text body without a:p")` を返す。T4 `body_without_paragraph` で確認されている。プローブでは `<p:txBody><a:bodyPr/><a:lstStyle/></p:txBody>`、`<p:txBody/>`、`<p:txBody><a:bodyPr/></p:txBody>` の 3 形を試した。`set_text("x")`/`""`/`"a\nb"`、`add_paragraph`、`remove_paragraph` は `UnsupportedContent`、`paragraph_mut(0)` は `ParagraphNotFound(0)` になった。panic は 0 件で、どの場合もバイトは変わらない。`expect("body_paragraphs: a:p+")` は、空でないことを確かめた後でしか呼ばれない |
| R2-2 T3 が書式の子要素を見ない | **解消** | `model.rs` の `fmt`/`canon` と `readback.py` の `fmt`/`canon` が、同じ正規形（Clark 名、namespace 宣言を除く、子要素は文書順、葉の要素だけ text）を作る。**round 2 の mutant（`strip_rpr_children`）は今は落ちる**: Rust T3 が FAILED、python は 16 failures（`japanese-table … items[0][2].children: length 0 != 3`）。`strip_ppr_children` も Rust T3 FAILED と python 14 failures、`strip_end_children` も Rust T3 FAILED と python 7 failures で落ちる。T2（locality）は 3 つとも通過する（想定どおり。T2 は位置の検査）。段落とランの付け替えは `t3_detects_wrong_paragraph_and_run` で固定されている |
| R2-3 python の `apply` が Rust と同じ構造、1 本体 1 編集 | **解消** | `check.py` の `edit` は、python-pptx の木の上で shape id とセルから本体を探し、lxml の要素を直接動かす（deepcopy、`addprevious`、`remove`）。期待値は、編集した木を `readback.dump_prs` で読み直して作る。モデルの JSON から計算はしない。独立していることの証拠は次のとおり（Rust と python に同じ誤りを入れた mutant）。`no_new_endpara_both`: Rust の T3 は通過し、T4 `format_sources` と python（18 failures）が検出。`new_runs_unformatted_both`: **Rust の T2/T3/T4 は全部通過**し、python だけが 18 failures で検出。`kept_rpr_strip_both`: Rust の全体比較は通過し、専用テストと python（16 failures）が検出。python 側に誤りを入れた場合（新段落の endParaRPr 無し / pPr 無し / br を残す / 最後でなく最初の段落を源にする）は、それぞれ 18 / 6 / 11 / 3 failures で検出された。multi sweep は本体ごとに 3 回、直前の状態から選んだ最後の段落とランへ編集を当てる。`cases.json` に入るので python 側でも検査される（48 cases の半分近く）。注: python の `edit` も同じ書き手が §3 から書いたものなので、仕様の**解釈**を誤れば両方が同じように誤る。ただ、その点は ADR §7 が正しく書いている |
| R2-4 §8 の文言、156/394 | **解消** | §8 L511-518。T2 の実測は 156/394 と 217/599 で、引用と一致する。「Not affected」の範囲も実装と合う |
| R2-5 新しい段落に `endParaRPr` が無い | **解消（根拠の書き方は N3-1）** | `fill_new_paragraph` が最後に `EndParagraphRunProperties` を挿入する。プローブでは `a:pPr, a:r…, a:br, a:endParaRPr` のスキーマ順で出力され、python-pptx も読めた。`FormatNotCopyable` の判定は `set_text` の計画段階（書き換えの前）で行う。mutant `no_new_endpara`（Rust だけ）は Rust T3 と python（18）で検出され、`endpara_first_not_last` も Rust T3 と python（2）で検出された。挙動の変化（空の行を足すだけでも書式源を参照する）は、§3 と T4 `format_sources`（`"t\n"` → `FormatNotCopyable`）に書かれている |
| R2-6 `IncompleteSpec` の列挙漏れ | **解消** | `write.rs` L311-322 の const assert は、Phase 0 が `set_attr`/`set_text` する要素（rPr/pPr/endParaRPr/t）とその親（r/br/p/txBody）を覆っている。わざと `ATTRS_INCOMPLETE = true` にするとビルドが止まることを確かめた。ADR §3 L193-199 にも列挙が加わった |
| R2-7 §6 の条件が T4 で全部は発生しない | **概ね解消（文言は N3-2）** | Strict、`a:p` 無し、順序の崩れ、グループの id 欠落、未知の `p:txEl` の子、数値でない `spid`、comment の rel の先の欠落、アンカーの無い `p188:cm` が T4 に加わった。いずれも `refused()` でバイトが変わらないことを確かめている。残りは N3-2 に書いた |
| m9 の残り（トップ README） | **解消** | `README.md` に「Third-party test fixtures」の節が加わった |

---

## 新しい指摘

### N3-1 [Minor] R2-5 の根拠「PowerPoint の Enter が段落末の書式を引き継ぐ」と、実装の書式源が一致しない

- ADR §3 L243-246 と rev 3 の表は、新しい段落の `a:endParaRPr` を「run format source のコピー」と定め、その理由に「PowerPoint's Enter carries the end-of-paragraph format over」を挙げている。
- ところが run format source は、最後の段落に**ランがあれば先頭ランの `a:rPr`** である。その場合、最後の段落にある `a:endParaRPr`（これが end-of-paragraph format）は見られない。プローブの結果は次のとおり。
  - `<a:r><a:rPr lang="ja-JP" … err="1" sz="1800" b="1"/>…</a:r><a:r><a:rPr lang="en-US" i="1"/>…</a:r><a:endParaRPr lang="de-DE" sz="900"/>` に `set_text("a\n\nc")` を当てると、新しい段落はどれも `<a:endParaRPr lang="ja-JP" … err="1" sz="1800" b="1"/>` になる。元の段落の `de-DE sz=900` は引き継がれず、スペルミスの印 `err="1"` が引き継がれる。
  - 最後の段落の `a:endParaRPr` が子要素（`a:latin`）を持っていても、最後の段落にランがあれば拒否されない。新しい段落は `lang` だけの `endParaRPr` になる。
- 出力は壊れず、ADR の規則の文言どおりに動いている（Rust と python の両方がこの規則で一致している）。問題は根拠の書き方だけである。
- 提案（どちらか）:
  - (a) 根拠を「新しいランと同じ書式にする（空の行の高さが新しいランの行と揃う）」に書き換える。
  - (b) 規則を「最後の段落の `a:endParaRPr` があればそれを、無ければ run format source を写す」に変える。
  - Phase 0.1 で subtree をコピーするようになると、この選択で引き継ぐ子要素が変わる。そのため、copy primitive の PR より前に決めておく方がよい。

### N3-2 [Minor] T4「every error **and every sub-condition** of §6」はまだ文字どおりには成り立たない

T4 で発生させていない条件は次の 3 つ（いずれも挙動は正しい）。

| 条件 | プローブの結果 | テストの状況 |
|---|---|---|
| `p188:cmLst` の未知の子 | `CommentAnchoredText { shape_id: 3 }` | `refs.rs` の単体テストにだけある |
| `ac:deMkLst` の下に `txMk` 要素は無く、`cp`/`len` 属性だけがある | `CommentAnchoredText { shape_id: 2 }` | どこにも無い |
| `cNvPr@id` が数値でない（欠落ではなく） | `InvalidShapeId` | どこにも無い |

`p:txEl/p:charRg` の直接の形は `animations` で検査されている。

提案: 上の 3 件をメモリ上で書き換えた入力として T4 に加える。または §7 を「every error, and every sub-condition either in `refusals.rs` or in the `refs.rs` unit tests」に書き換える。

### N3-3 [Minor] Accepted / Phase 0.1 と食い違う文言が残っている

- `docs/roadmap/pptx.md` L21 の「ADR-0005 (Proposed)」が古い。
- ADR-0005 の L265（「the subtree-copy primitive that the coordinator deferred to Phase 2」）、§8 L507（「coordinator default: subtree copy deferred to Phase 2」）、Follow-ups L547（「Copy-subtree primitive deferred to Phase 2」）は、rev 3 の記述（Q2 には答えが出て Phase 0.1 で行う）と矛盾する。
- 提案: 「Phase 0 は attribute-only。subtree copy は Phase 0.1（別の revision）で行う」に揃える。履歴として残すのであれば、「（rev 3 で Phase 0.1 に前倒し）」と注記する。

---

## ADR-0005 rev 3 とコードの照合

一致している。

- §3 Paragraph content（`a:p+`）は `body_paragraphs` と、Edits / Format sources は `fill_new_paragraph` と `set_text` と一致する。ただし根拠の書き方に N3-1 がある。
- §3 Losslessness（`IncompleteSpec`）は const assert と、§6 の表（`UnsupportedContent` に本体の `a:p` 無しを含む）は実装と一致する。
- §7 T3 の数値（48 cases、1,393 edits）、正規形、python の独立性の説明、「wrong shape/paragraph/run と a:sym の欠落を検出することをテストで示す」という主張は、いずれも実行結果と一致した。
- §7 T4 の列挙にある条件は、すべて T4 に存在する（「every sub-condition」という総称だけが N3-2）。
- crate README と `error.rs` の文言も一致する。

## 判定

- **converged: yes** — Blocker 0、Major 0。round 2 の指摘 7 件はすべて解消した。新しい指摘は Minor 3 件で、どれも文言か根拠の書き方の問題。
- **ADR-0005 を Accepted にしてよい: yes** — N3-1〜N3-3 は、Accepted の後に文言を直すだけで済む。N3-1 は、Phase 0.1 の copy primitive を pptx に入れる PR より前に、どちらにするか決めておくことを推奨する。
