# PR #49 レビュー（Opus, round 3）

対象: PR #49 `core/detach-same-part` @ `937d4f0`（round 2 は `397bd86` をレビューした）。

方針: 読み取り専用の敵対的レビュー。作業は `git clone` した copy で行った。

- probe: scratchpad の `review-pr49-r3/`
  - `pr49/crates/ooxml-core/tests/zz_r2.rs`: round 2 の P1〜P9。P6 は除いた（m1 の確認に使った。下記）。
  - `pr49/crates/ooxml-core/tests/zz_r3.rs`: 新しい probe。copy と move の差分照合、MC が root の文書、派生 lens。
- corpus 試験: `pr49c/crates/ooxml-schema/tests/zz_corpus_r2.rs`。round 2 の試験に、move との差分照合を足した。
- ミュータント 40 件: `scripts/mut.py`、結果は `mut.log`。

## 結論（先に）

**Blocker と Major は無い。Minor も無い。Nit が 4 件ある（どれも文書か試験の小さな補足）。**

- **P49R2-M1 は解消した。**
  - 移動元はコピーを取る時点で、移動先は check twin と insert の両方で、`mc:AlternateContent` の分岐の中なら `NotDetachable { AlternateContent }` で拒否する。
  - 規則は #44 の `plan_move` と同じである。root を含む経路上に MC の名前空間の要素があれば拒否する。照合は、移動元の親と、挿入先の要素そのものについて行う。
- **#44 との一致を差分照合で確かめた。** 同じ文書の 90 組（AC の Choice と Fallback、既定名前空間、prefix の再束縛、`mc:Ignorable`、`xml:space` を含む）で、copy の結果と、#44 の `check_move_in_by` の結果を比べた。
  - 拒否理由の分類は 90/90 で一致した。
  - check と insert も 90/90 で一致した。
  - Ok の 12 件は、再 parse すると元と構造的に等しい。Err の 78 件は、文書を変えていない。
  - corpus の 69 組でも、move との差は 0 件だった。
- **m1 は解消した。** 別の名前空間への改名は、コンパイルエラーになる（E0080: "a same-part copy cannot be renamed into another namespace"）。round 2 の P6 で確かめた。
- **m2 は解消した。** main の helper と #44 の `Usage` を使っている。
  - #44 への変更は、`Usage` とそのフィールドと `of` を `pub(super)` にしただけである。`origin/main` との差分は可視性の 13 行だけで、挙動は変わらない。
  - #44 の試験（`move_subtree.rs` の 16 件、`insert_ns.rs` の 12 件）は通る。
- **全体の結果:**
  - `cargo test --workspace --all-features`: 397 passed、0 failed、10 ignored（PR の記載と一致）。
  - clippy `-D warnings` は警告なし。
  - `xtask codegen --check` は up to date。
- **corpus（重複を除いて 619 ファイル、`a:r/a:rPr` 全 47,464 件）:**
  - 47,463 件をコピーした。T2 は 47,463/47,463 で成り立った。
  - 拒否は 1 件で、理由は `AlternateContent`。`mc:Fallback` の中にある唯一の `a:rPr` である（`ac.py` を `python3 -I` で実行: `rpr_in_ac=1`、`rpr_in_choice=0`）。M1 の修正どおりの結果である。
  - 別の段落への挿入 69 件は、check と insert が一致し、元と構造的に等しかった。

**判定: converged = yes。**

---

## 解消状況（round 2 の指摘）

| ID | 修正 | 確認 | 状況 |
|---|---|---|---|
| **P49R2-M1** AC をまたぐコピー | `mc_on_path` を追加した。移動元の親（取る時点）と挿入先（`check_destination`、つまり check twin と insert の両方）を調べる | P1 と P1b は取る時点で `AlternateContent`。新しい試験は Choice→外、Fallback への挿入、外→外を扱う。差分照合は 90/90 で一致。派生 lens（`lens_at::<R>` → `child::<RPr>()`）も拒否される。ミュータント N1、N2、N6 は落ちる | **解消** |
| m1 別の名前空間への改名 | `const { assert!(str_eq(C::QNAME.ns, E::QNAME.ns)) }`。`root_prefix_at_destination` の分岐を削除した | P6 のビルドが E0080 で失敗する | **解消**（ただし n3 を参照） |
| m2 helper の重複 | `declarations`/`effective`、`ancestor_context`、`subtree_height`、#44 の `Usage` を使う。`Scope`、`usage`、`height` を削除した | 差分を読んだ。#44 への変更は可視性だけ。#44 の試験は通る。ミュータント N35〜N39（`Usage` 側）は copy の試験と move の試験の両方で落ちる | **解消** |
| n1 インデント | 共通の `Usage` を使うので、#44 の m3b と同じ扱いになった。試験 `indentation_is_not_inherited_text` | P5 は Ok/Ok で、出力は元と等しい | 解消 |
| n2 ProcessContent/MustUnderstand | module doc と ADR-0002 note に、既知の限界として書いた | P4d と P4e は従来どおり Ok（#44 と同じ） | 解消（文書化） |
| n3 QName 型の属性 | ADR-0002 の `[detach]` の確認項目に1文を足した | 文面を確認した | 解消 |
| n4 試験の穴 | depth の境界、要素名の prefix だけの再束縛、Ignorable の prefix の再宣言を試験に足した | 対応するミュータント N15、N39、N9 は落ちる | 解消 |
| n5 `own_dirty` | 行を残し、理由をコメントに書き、試験を足した | N33 は落ちる | 解消 |
| n6 corpus の記述 | ADR-0002 note に 47,464 件の結果を書いた | 件数は正しい。ただし「0 refusals」は round 2 の head での値 → n2 | ほぼ解消 |

round 2 の probe を再実行した（`937d4f0`）:

- P2〜P5 と P7〜P9 は、round 2 と同じ結果だった。P5 は n1 の方針どおり、Ok に変わった。
- P8 の 5×5 の行列は、Ok 6、Err 19 で一致した。Err のときは何も変わらず、Ok のときは構造的に等しい。

## Findings

Blocker、Major、Minor は無い。

### Nit

- **r3-n1** 移動元の AC 拒否が、取る側の文書に書かれていない。
  - `detach_same_part_as` の rustdoc の「Still refused」と、README の `detach_same_part_as` の行は、取る時点の `AlternateContent`（要素が `mc:AlternateContent` の分岐の中にある場合）を挙げていない。
  - README の「out of」は `insert_same_part` の行に書かれているが、実際に拒否するのは取る側である。
  - 推奨: rustdoc の「Still refused」に「an element inside an `mc:AlternateContent` branch（`AlternateContent`, as a move）」を1項目足す。
- **r3-n2** ADR-0002 note の「47,464 … with T2 intact and 0 refusals」は、round 2 の head（`397bd86`）での値である。
  - `937d4f0` では、47,463 件をコピーし、1 件を `AlternateContent` で拒否する（`mc:Fallback` の中の `a:rPr`）。これは M1 の修正による正しい結果である。
  - 推奨: 文を「47,464 – 1 refused (`AlternateContent`, the only `a:rPr` inside a branch)」のように直す。
- **r3-n3** m1 の `const { assert! }` は、単相化の後で評価される。
  - そのため、エラーが出るのは `cargo build` と `cargo test` だけである。`cargo check` は通る（P6 入りで確認した）。また、そのペアを実際に呼ぶまでエラーにならない。
  - 公開型は `SameType` の実装を外部に開いていない（sealed、生成のみ）。そのため実害は無い。
  - 推奨: ADR の「compile-time error」に「(post-monomorphization: at build, not `cargo check`)」と添える。任意。
- **r3-n4** 試験の小さな穴（任意）。
  - ミュータント N7（未束縛の既定名前空間を `""` でなく `None` にする）が生き残る。#44 には `an_undeclared_default_namespace_equals_xmlns_empty` があるが、copy 側には対応する試験が無い。
  - P2 の「nons → `xmlns=""`」は、現状では Ok/Ok である。変異を入れると、安全側の偽の拒否になる。
  - 1 行の assert で落とせる。

## ミュータント（`cargo test -p ooxml-core -p ooxml-schema --all-features --tests`）

round 3 の差分に合わせて 40 件を作り直した。AC の規則、共通の `Usage`、check と insert、従来の拒否を含む。**32 件が落ち、8 件が生き残った。**

| 生き残ったもの | 評価 |
|---|---|
| N3（`mc_on_path` で文書 root 自身の MC 判定を外す。PR が「生き残る」と書いたもの） | **MC に適合する入力では等価。** 観測できるのは、文書の root が `mc:AlternateContent` で、その直下が MC でない要素である、不正な文書だけである。probe `r3_mc_root_document` では、変異があると copy は `Ok` になり、move は `AlternateContent` になる。変異の無いコードは #44 と同じく拒否する。PR 本文の理由（「a typed root is never an MC element」）は正確でない。文書の root は型付きの要素とは限らない。正しい理由は「AC の子は必ず `mc:Choice`/`mc:Fallback` で、それ自体が MC なので経路の途中で捕まる」である。欠陥ではない |
| N4（経路の最後の要素を MC 判定から外す） | 等価。挿入先の最後の要素は、型付きの `LensMut<E>` の要素である。移動元の最後の要素は、MC の要素の直下に `rPr` があるという不正な形でない限り、祖先の AC か Choice が捕まる |
| N5（`AlternateContent` という名前の要素だけを MC とみなす） | 適合する入力では等価。Choice と Fallback は必ず AC の中にある |
| N7（未束縛の既定名前空間を `None` にする） | ほぼ等価。差は `xmlns=""` との比較だけで、安全側の偽の拒否になる → r3-n4 |
| N11、N12（移動元のスコープに root 自身を含める） | 等価。root の宣言はコピーの中で宣言済みとして扱われる。root の MC 属性は拒否される。root の `xml:space` があれば、その下のテキストは継承しない |
| N13（文書の root をコピーするとき、root の文脈を入れる） | 同じ理由で等価 |
| N25（一意属性を E の分だけ見る） | 実質的に等価。生成された組の一意属性のリストは同じである（round 2 の M18 と同じ） |

落ちたもの（32件）:

- AC: 移動元、挿入先、移動元の検査を後に回す変異。
- 照合: 自由な prefix、ignorable、`xml:space`。
- depth: height、insert、check、`created` を無視する変異。
- 挿入先の照合を insert と check から外す変異。
- 文書の照合: insert、check、clone の token。
- 一意属性: root、子孫、既定値。
- MC: root の属性、子孫の属性、要素。
- `ext`: 名前空間と URI。
- dirty 印: skip、改名、自己終了の親。
- 改名先の名前。
- `Usage`: インデント、自身の `xml:space`、自身の宣言、属性の URI、要素名の prefix。

PR の主張（「生き残るのは等価な root-MC の1件だけ」）は PR 自身のミュータントの集合についてのものである。今回の集合では、ほかにも等価か、ほぼ等価なものが7件生き残った。安全性に関わる穴は無い。

## 判定

- **converged: yes.** Blocker、Major、Minor は無い。
- round 2 の Major（M1）と Minor（m1、m2）は、すべて解消した。#44 と同じ規則であることを、差分照合（手作りの 90 組、corpus の 69 組）で確かめた。
- Nit の r3-n1〜n4 は任意である。merge 前に直すなら、r3-n1（rustdoc の1項目）と r3-n2（ADR の件数）が安価である。
- merge は squash を推奨する（round 1 の n4 のとおり）。

## 再現物

scratchpad の `review-pr49-r3/`:

- `pr49/`（`937d4f0` の clone）: `tests/zz_r2.rs`（P6 を除く）、`tests/zz_r3.rs`
- `pr49c/`（corpus 用の clone）: `crates/ooxml-schema/tests/zz_corpus_r2.rs`。move との差分照合を加えた。`OOXML_SAME_PART_CORPUS=…/review-20r5-49/pptx.list`、`--release` で 64 秒。結果は `corpus.log`
- `scripts/mut.py`、`mut.log`
