# PR #45 レビュー（Opus, round 2）

対象: `core/insert-with-ns` @ `3726d5f`（round 1 の修正: M1, m1-m3, n1-n4。`insert_at_with_ns` と check twin を追加）。検証は scratchpad `review-44r2-45r2/pr45/` の clone で行った（`wt` = HEAD、`main` = `origin/main` @ `ab1b0b6`、`merged` = HEAD に `ab1b0b6` を試験マージしたもの）。Rust 1.99.1、Python 3.15.2 / expat 2.10.0。

## 結論（先に）

round 1 の指摘はすべて解消している。Blocker と Major はない。

- **M1:** NCName 判定は XML 1.0 第 5 版の [4]/[4a] から `:` を除いたものと、全コードポイント（1,112,064 個、先頭文字と非先頭文字の両方）で完全に一致した。
- **m1:** check twin が返す宣言は、実際に書かれる `xmlns` 属性と 23,670 ケースで一致した。
- **新しい `insert_at_with_ns`:** 宣言なしの場合の挙動は、`origin/main` の `insert_at` と 11,300 ケースでバイト単位まで同一である。

新しい指摘は Minor 1 件と Nit 4 件で、テストの穴と文言の古さだけである。

## 解消状況

| round 1 | 状態 | 根拠 |
|---|---|---|
| M1 NCName | 解消 | `is_name_start_char`/`is_name_char`（lens.rs L493-516）を独立に実装した第 5 版の表と比較し、不一致は 0 だった（`ncname_cmp.out`）。`:` は両位置で拒否される。round 1 の `ª µ ² Ⓐ ¼` はすべて `InvalidNamespaceDeclaration` になる（`probe_pr45_r1.rs` の再実行）。判定は `validate_decls` 1 か所にまとまり、すべての `*_with_ns`、check twin、`get_or_insert(_with_ns)`、`insert_at_with_ns` がこれを通る |
| m1 check twin の戻り値 | 解消（テストは m4 を参照） | `_with_ns` の twin はすべて `Result<Vec<(prefix, uri)>>` を返す。brute force の 23,670 ケースのうち Ok は 7,756 件で、どれも書かれた `xmlns` 属性と一致した。Err は check と apply で同じエラーになり、不変で dirty にもならない。Ok の出力 843 種はすべて expat の名前空間モードで再パースできた。ADR-0011 rev 4（main）も「宣言を返す」に改められ、文面は整合した |
| m1 入れ子の dry run | 解消 | README、ADR-0002 note、`check_insert_by_with_ns` の doc に「`XmlDocument` の clone 上で全手順を実行する」と書かれた |
| m2 main とのコンフリクト | 解消（要再マージ） | `854e5d9` で一度マージ済み。その後 main は `ab1b0b6`（#20、#29 など）まで進んだが、`git merge-tree` ではクリーンにマージできる。試験マージ上で `cargo test --workspace --all-features` は 364 件通過、失敗 0。`cargo clippy --workspace --all-features --all-targets -D warnings` も問題なし。PR をマージする前に、もう一度 `origin/main` をマージすること |
| m3 生存 mutant 2 体 | 解消 | `effective(..)==Some(uri)` を外した版は `a_shadowed_binding_is_not_reused` が殺した。束縛済みなら検証を省く版は `requests_are_validated_even_when_nothing_is_declared` が殺した（`mutants.out`） |
| n1 get_or_insert が既存の子で不正な decl を通す | 解消 | 既存の子があっても `validate_decls` を先に実行する（check も apply も同じ）。doc も更新済み |
| n2 要求順への依存 | 解消 | `NsDecl` の doc、README、ADR-0002 に追記された。doc の例 `[p1, p]`（`p` 束縛済み）で `p2` になることを確認した |
| n3 `mc:Ignorable` | 解消 | ADR-0002 note と README に追記された |
| n4 `detach_as` との相互作用 | 解消 | ADR-0002 の Extension points と README に追記された（#44 は #44 側の課題のまま） |

## Findings

### m4（Minor）brute-force parity が check twin の戻り値を捨てている。戻り値の誤りを殺せない mutant が 2 体ある

- **該当箇所:** `crates/ooxml-core/tests/insert_ns.rs` L184-193（`check_and_apply_agree`）。どの op も `.map(|_| ())` で戻り値を潰してから比べている。戻り値を見ているのは `checks_return_the_planned_declarations`（L329-）だけで、4 文書に対して `check_insert_with_ns` と `check_insert_at_with_ns` しか確かめていない。
- **生存した mutant:**
  - `goi_existing_returns_decls`: `check_get_or_insert_with_ns` が、子が既にあるときに要求をそのまま返す。apply は何も書かない。
  - `in_slot_check_empty`: `check_insert_in_slot_with_ns` が常に `vec![]` を返す。
- **実装自体は正しい。** `probe_r2_parity.rs` は全 5 種の twin と全 index を対象にし、twin の戻り値と書かれた宣言は 7,756 件すべてで一致した。
- **推奨:** `check_and_apply_agree` の Ok 側を、新しい要素の `attrs()` のうち `is_namespace_declaration()` に当たるものとの比較に変える。`get_or_insert` で子が既にある場合は `[]` と比べる。`probe_r2_parity.rs` の `written()` をそのまま使える。

### n5（Nit）第 5 版の名前規則は expat（第 4 版の表）より広い。仕様上は正しいが、README に一言あるとよい

- **根拠（`ncname_cmp.out`）:** Rust が受理して expat 2.10 が拒否するコードポイントは 937,032 個ある。内訳は BMP の 19,528 個と、補助面 U+10000-EFFFF の全部（917,504 個）である。逆に、expat が受理して Rust が拒否するものは 0 個だった。
  - BMP の例: U+0221 は `<ȡ:x xmlns:ȡ="u"/>` が expat で `not well-formed (invalid token)` になる。
  - 第 5 版の名前規則は第 4 版の上位集合として設計されている。そのため、この差は「第 5 版では整形式だが、古い表を使う厳格なパーサーは拒否する」という差である。
- **実害:** ほぼない。呼び出し側（`a`/`pic`/`r`/`wp`/`a16`）はすべて ASCII の定数である。
- **推奨:** README の該当箇所（L149-151）に「名前規則は第 5 版。古い（第 4 版の表を使う）パーサーとの相互運用を最大にするなら ASCII のプレフィックスを使う」と 1 文足す。ASCII だけに制限する必要はない。

### n6（Nit）README の「Requests are validated before anything else」は厳密ではない。拒否の優先順位が API によって違う

- **該当箇所:** `crates/ooxml-core/README.md` L151、ADR-0002 L222。
- **`insert_by_with_ns`/`insert_in_slot_with_ns`:** 先に `unique_slot`/`given_slot` を見る。そのため、不正な slot と不正な decl を同時に渡すと `NotAllowed` になる。
- **`insert_at_with_ns`:** 先に `validate_decls` を実行する。そのため、範囲外の index と不正な decl を同時に渡すと `InvalidNamespaceDeclaration` になる（`probe_r2_precedence.rs`）。
- **関連する mutant:** `at_validate_late`（`plan_insert_at` の先頭の `validate_decls` を外した版）は生き残る。ただし優先順位が変わるだけである。
- 「文書の状態によらず不正な要求は拒否される」という実質的な保証は成り立っている。
- **推奨:** 文言を「before anything that depends on the document」にする。

### n7（Nit）ADR の相互参照が古い

- **ADR-0002 L228-230:** 「ADR-0011 rev 3 asks for the prefixes actually used」と書いている。しかし main の ADR-0011 rev 4 は、この要件を撤回して「宣言を返す」に改めている（ADR-0011 L84-85、L248-253）。rev 4 を参照するように直す。
- **ADR-0011 側（本 PR の範囲外。後続で直せばよい）:**
  - L85 と L249 の「PR #45 is being changed」は、本 PR で完了した。
  - L381 の「Every insert of the operation is dry-run with the check twin」は、同じ ADR の rev 4 §1「外側の 1 段と深さの上限だけ」と食い違う。
  - L241 は「`insert_at` の `_with_ns` が必要なら」と書いているが、それは本 PR で入った。

### n8（Nit）NCName テストが補助面を 0x101 刻みでしか見ていない。上端の境界 mutant が生き残る

- **該当箇所:** `insert_ns.rs` L278 の `(0x1_0000..=0x10_FFFF).step_by(0x101)`。
- **生存した mutant:** `start_EFFFE`（`'\u{10000}'..='\u{EFFFF}'` を `..='\u{EFFFE}'` にした版）。
- **推奨:** U+EFFFF（受理）と U+F0000（拒否）を明示的に good/bad のリストへ足す。全数を回しても release ビルドで約 3 秒である（`probe_r2_ncname.rs`）。

## 検証

- **HEAD `3726d5f`:** `cargo test --workspace --all-features` は 291 件通過、失敗 0。
- **試験マージ（HEAD + `ab1b0b6`）:**
  - コンフリクトはない。
  - テストは 364 件通過、失敗 0（`test_merged.txt`）。
  - clippy は `-D warnings` で問題なし（`clippy_merged.txt`）。
- **NCName（`probe_r2_ncname.rs` → `rust_verdicts.txt`、`ncname_ref.py` → `ncname_cmp.out`）:**
  - サロゲートを除く全コードポイントについて、`c` 単独と `"a"+c` の 2 通りで、公開 API の `insert_with_ns` の判定を記録した。
  - 独立に書いた第 5 版の表との不一致は 0。
  - expat との差は n5 のとおり。
- **check と apply の一致（`probe_r2_parity.rs`）:**
  - 対象: 12 文書 × 255 の宣言集合 × 全 op。op は `insert`/`insert_by`/`insert_in_slot`/`get_or_insert` と、`insert_at` の全 index。
  - 文書には、覆い隠された束縛、default namespace だけの束縛、fallback の連鎖、`mc:AlternateContent` を含む。
  - 結果: 23,670 件、うち Ok 7,756 件。戻り値と書かれた宣言はすべて一致し、Err は不変で dirty にもならなかった。
  - Ok の出力 843 種（`parity_outputs.txt`）は、すべて expat の名前空間モードで再パースできた。
- **`insert_at` の main 互換（`probe_r2_insert_at_dump.rs`）:**
  - 生成した 1,170 文書 × index 0..=len+1 × 4 種の子について、check、apply、dirty、出力を記録した。
  - `origin/main` と試験マージの結果は完全に一致した（`insert_at_main.tsv` = `insert_at_head.tsv`、11,300 行）。
- **`insert_at_with_ns` と `insert_at` の比較（`probe_r2_insert_at_ns.rs`）:**
  - 14,125 件を比べた。
  - 束縛済みの URI、または空の宣言では、出力と dirty が `insert_at` と同一だった。
  - 新しい束縛を加える場合は、Ok/Err が `insert_at` と同じで、差は `xmlns` 属性 1 個分だけだった（1,984 件）。
  - 不正なプレフィックスは拒否され、Err なら不変で dirty にもならなかった。
- **mutant（`mutants.py`、`mutants.out`）:**
  - 23 体を試し、19 体が殺された。
  - 生き残ったのは `start_EFFFE`（n8）、`at_validate_late`（n6、優先順位のみ）、`goi_existing_returns_decls` と `in_slot_check_empty`（m4）の 4 体である。
  - round 1 の生存 2 体は殺された。
- **round 1 のプローブ（`probe_pr45_r1.rs`）を再実行:**
  - M1 の 5 文字はすべて拒否された。
  - n1 は check と apply の両方で `InvalidNamespaceDeclaration` になった。
  - 覆い隠された束縛、URI のエスケープ、順序依存は、round 1 と同じ正しい結果だった。

プローブと出力はすべて `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-44r2-45r2/pr45/` にある。

## 判定

converged: **yes**。Blocker と Major は残っていない。m4（parity テストで戻り値を比べる）と Nit 4 件は任意だが、どれも数行で直せる。マージ前に `origin/main` をもう一度マージすること（クリーンにマージでき、テストも通ることは確認済み）。
