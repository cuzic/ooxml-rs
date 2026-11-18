# PR #73 レビュー（Opus, round 1）

対象: `fix/core-registry-followups` @ `d78432d50ef96dd65d745a618c66f410ad053643`

照合したもの: PR 本文、`docs/reviews/pr38-round3.md`（n9〜n14）、ADR-0016 §7、`AGENTS.md`、
差分（`add.rs`、`package.rs`、README、`tests/registry_review3.rs`、`tests/registry_fuzz.rs`）。
プローブと変異はスクラッチ領域の clone で行い、コミットしていない。

## 結論（先に）

**converged: yes（Blocker 0 / Major 0）。** 変更は #38 round 3 の n9、n10（R1/R3/R4/R15 と retarget の fuzz）、
n12、n13、n14 に収まっていて、範囲外の n11 には触れていない。新しい拒否（n12 の 3 形）は、どれも `Err` の後の保存が
入力とバイト単位で同じだった。plan の途中で拒否されたときも、plan は変わらなかった。
ooxml-xlsx、docx、pptx は `set_relationship_target` を呼んでいないので、n12 の拒否で壊れる呼び出し元はない。
Minor が 1 件ある。外部 URI の `#` の規則が、`add_relationship` と `set_relationship_target` で食い違っている。

## 前ラウンドの解消状況（#38 round 3 の follow-up）

- n9（旧 Id の規則の範囲）: **解消**。README の 2 行を「`stage_part`、同じ plan に限る」に直した。`copy_relationships` が source の Id を保つことも書いた。実装の検査は広げていない。これは round 3 が示した 2 案のうちの後者である。
- n10 R3/R4: **解消**。`r3_r4_last_retarget_wins` を足した。`.rev()` を外す変異は、view 側（`add.rs:679`）と removal 側（`add.rs:1798`）のどちらも、このテストで落ちる。
- n10 R15: **解消**。`r15_external_and_fragment_ids_count` を足した。`gone_ids` を part を指す関係だけに絞る変異（`add.rs:1751`）は、このテストで落ちる。
- n10 R1: **解消**。`r1_unreadable_original_detaches` を足した。`map_or(true, …)` を `false` にする変異（`add.rs:1930`）は、このテストで落ちる。
- n10 fuzz の推奨: **解消**。`registry_fuzz.rs` に retarget、plan の前のセッション編集、`read_part` の parse し直しの stage を入れた。既定の 3000 シードは debug で 17 秒で通った。
- n12: **解消**（m1 を参照）。`#` だけの fragment と、2 つ目の `#` を `InvalidTarget` で拒否する。拒否後の bytes が同じであることは `n12_fragments` で確かめている。
- n13: **解消**。`from_bytes` は `find_ci` で package の関係を読む。`_rels/.Rels` でも、追加と retarget の後のキャッシュと再読込が一致した（3 件）。保存しても、別名の `_rels/.rels` はできない。完全一致に戻す変異は `n13_package_rels_any_case` で落ちる。
- n14: **解消**。README の「Not yet」に 1 行足した。`xl/media/あ x.png` が実際に `InvalidTarget` になることも確かめた。
- n11: 範囲外のまま（PR 本文に明記）。

手で入れた変異 8 個（n12 の空ロケーション、`#` の個数、external だけを除外、n13 の完全一致、R1、R3、R4、R15）は、すべて `registry_review3` で落ちた。

## Findings

### m1（Minor）: 外部 URI の `#` が 2 個以上のとき、`add_relationship` は受け付けるが `set_relationship_target` は拒否する

- 根拠:
  - `add.rs:994` の `Target::External(u)` は `uri_like` だけを見る。`add.rs:1186` の retarget は `target.matches('#').count() <= 1` も要求する。
  - プローブで、`add_relationship(Some("xl/s.xml"), …, Target::External("https://e.example/#/r#sec"))` は `Ok("rId1")` になった。同じ Id を同じ値に retarget すると `InvalidTarget` になった（保存は byte-identical）。
  - core 自身が書いた関係を、同じ値に戻せない。入力に元からある `https://e.example/#/r#sec`（hash routing の URL。Office は書かれたまま保存する）も、別の同じ形の URL には retarget できない。
  - 安全側の過剰拒否である。いまの呼び出し元はない（format crate は `set_relationship_target` を使っていない。ADR-0016 §7 のリンクは `add_relationship` を使う）。
- 修正案: 2 つの API の規則をそろえる。`#` の個数の検査を、part を指す internal target（`media/b.png#a#b`）だけに限るのがよい。外部 URI にも当てる場合は、`add_relationship` にも同じ検査を入れて README の rules table に書く。

### n1（nit）: fragment-only の「空でない」の検査は長さだけを見ている

- 根拠: `add.rs:1184` の `target.len() > 1` は、`"# "` を受け付ける（空白は control 文字ではない）。`TargetMode="External"` の `"#"` は fragment の分岐に入らない。`uri_like` も通るので、受け付けられる。
- 修正案: 必要なら `#` の後ろを trim して空なら拒否する。doc の「non-empty location」を、この挙動に合わせる。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 1）
