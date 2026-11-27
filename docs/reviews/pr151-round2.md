# PR #151 レビュー（Opus, round 2）

対象: impl/core-perf-stage2 @ 9b54712f169a0375e097794912430ac867069df2

## 結論（先に）
round 1 の M2 と m1–m4 は挙動で解消を確認した。M1 は大部分が解消した。全 fixture part で全要素名を defer する oracle が CI の `cargo test` で走り、未展開のまま属性を編集する経路と、コミット済み seed の再生も入った。
ただし、名前空間 snapshot の無効化 3 か所のうち、built の要素が宣言を持つ場合の無効化（`parse.rs` 518 行）を外しても、全テストが通る。この変異では、Ok のまま prefix が別の URI に解決される。oracle は「全要素名を defer」しか試さないので、構造上この経路を通らない。
**converged: no**（Major 1）。

## 前ラウンドの解消状況
- M1: 一部解消。`xml/deferred_oracle.rs` は fixture と seed で次を確かめる。全名の defer、census、未展開の属性編集（T2 付き）、全 release、`force_rerender`、壊した入力でのエラーの一致（`with_max_depth(4)` を含む）。`restore_scope` の変異は `deferred_siblings_restore_the_inherited_namespace` で落ちることを確認した。残りは下の M1 に書く。
- M2: 解消。`census` は `Option<usize>` を返す。要求していない名前は `None`、要求して無かった名前は `Some(0)`。キャッシュ済み part に後から要求した名前も `None` になる（`depth_limit.rs` のテスト）。
- m1: 解消。ADR の注記に、ピークの主因が `read_to_end` の倍々の伸長であること、直接解凍の方針、F-6 との関係が書かれている。PR 本文にも書かれている。
- m2: 解消（doc）。`content_span` の doc に、編集後は古い範囲になりうると書かれている。
- m3: 解消。`ensure_parsed` は深さに `min(package, caller)` を使い、そのあと metadata の下限を適用する。この `min` を外すと `per_part_depth_cannot_relax_the_package_limit` が落ちる。
- m4: 解消。子孫が `preserve_attribute_spelling` を持つと release を拒否する。`has_spelling_preference` を常に false にすると `release_keeps_descendant_attribute_spelling_preference` が落ちる。
- nit 2 件: 解消（PR 本文に記述あり。seed が無いときは理由を表示して失敗する）。

## 確認できたこと（プローブ。コミットしない）
- `cargo test -p ooxml-core --release`: 全件 pass した。fixture の oracle は lib の unittest として 8 秒ほどで走る。CI は 5 ジョブすべて pass。
- 手書きの変異（`parse.rs` の `self.snapshot = None` を 1 か所ずつ外す）:
  - 332 行（`restore_scope`）: 新しい回帰テストが落ちる。
  - 504 行（skip 中の宣言）: 全テストが通る。ただし skip 中は新しい deferred ノードを作らず、閉じタグで `restore_scope` が無効化する。等価な変異と判断し、指摘しない。
  - 518 行（built の要素の宣言）: 全テストが通り、出力が誤る（下の M1）。
- 他の open PR（#144、#155）は core の xml、lens、package に触れていない。

## Findings

### M1（Major）: built の要素が prefix を再宣言したときの snapshot 無効化がテストされていない。外すと Ok のまま別の URI に解決される
- 根拠:
  - `crates/ooxml-core/src/xml/parse.rs` 518 行の `self.snapshot = None;` を外し、`cargo test -p ooxml-core --release --no-fail-fast` を実行すると、全テストが通る。
  - この変異で、次の入力を `with_defer(&[row])` で parse して 2 つ目の row を展開すると、`p:a` が `u1` に解決される（full parse では `u2`）。
    ```text
    <r xmlns:p='u1'><row/><x xmlns:p='u2'><row><p:a/></row></x></r>
    ```
    1 つ目の row で作った snapshot（p=u1）が、built の `x` が宣言した後も使い回される。
  - oracle（`xml/deferred_oracle.rs`）は、part に出る**全要素名**を defer する。そのため `x` も deferred になり、内側の row は `x` の展開時に別の snapshot で作られる。この経路は通らない。上の入力を `tests/support/xml_seeds/` に加えても、変異下で seed の再生は pass した。
  - これは round 1 の M1 と同じ種類の誤り（名前空間の snapshot の取り違えで Ok の黙った誤り）である。round 1 で塞いだのは `restore_scope` の 1 か所だけだった。
- 修正案:
  - oracle に「名前を 1 つずつ defer する」比較を加える。少なくとも seed と、fixture のうち名前の少ない part で行う。defer しない祖先が宣言を持つ構成を通すことが目的である。
  - 上の入力の回帰テストを `tests/deferred.rs` に加える。518 行の変異で落ちることを確かめる。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 0 / nit 0）
