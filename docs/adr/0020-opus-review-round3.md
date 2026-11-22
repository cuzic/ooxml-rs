# ADR-0020 レビュー（Opus, round 3）

対象: PR #145 `plan/ac-rank` @ `9c421927e29a1714f28d39aba1dbbdade3422781`
（`docs/adr/0020-alternatecontent-rank.md` revision 3 と `spikes/ac-rank/`）。

範囲: round 2 の指摘が直ったかどうかの確認と、revision 3 で新しく入った記述だけ。差分は `2321841..9c42192`。コードは `main`（`d066ff1`）で照合した。

## 結論（先に）

round 2 の M1、m1、m2、m3、n1 は、すべて本文で解消されている。

AC フラグの置き場所として選んだ `CompactSpan::close_start` の上位ビットは、本当に空いている。

- parser は、`MAX_SPAN_OFFSET`（2^31 − 1）を超える入力を拒否する（`parse.rs:105`）。だから `close_start` の値は最上位ビットを使わない。
- ADR-0010 も parser も、このビットを使っていない。

`child_mut` / `children_slice_mut` が要素を丸ごと差し替えていないかの確認は、U1 に残されている。`main` で確かめたところ、差し替えている呼び出しは 1 つも無かった。したがって、U1 の作業として残しておいて問題ない。

新しく見つけたのは nit が 2 件だけである。Blocker と Major は 0 なので、**converged: yes**。

## 前ラウンドの解消状況

- **M1: 解消。**
  - callback や位置指定で挿入する場合、全 branch が空の AC を「どの slot にも入っている」とみなして拒否する（§4.2、§4.3 の手順 1）。
  - docx の `insert_image` とハイパーリンクは、slot をまたいで要素を数える。そこで docx 側の事前チェックとして、`w:p` に AC の子が 1 つでもあれば拒否する（§4.4、§9）。
  - round 2 で挙げた 2 つの例は、U1（core の拒否）と U3（docx の事前チェック）の byte-identical テストに入った（oracle 2 と 3）。
  - §4.2 には、slot 単位の規則が守るのは「slot の中で数える呼び出し側」に限られることが明記された。
  - mutation（f）も足された。
- **m1: 解消。** 記録された grep を `d066ff1` で実行すると、非テストの呼び出しがちょうど 86 件出る。テストモジュール内のものは 0 件である。漏れていた 6 件は表に入った。`worksheet.rs:280` は `check_get_or_insert` に切り替える行として追加された。
- **m2: 解消。** 確認した事実は次のとおり。
  - `FLAG_BIT = 1 << 31`、`MAX_SPAN_OFFSET = FLAG_BIT − 1`（`node.rs:6-7`）。
  - `parse.rs:105` は、`len > MAX_SPAN_OFFSET` の入力を拒否する。`parse.rs:410` でこの上限が assert されている。
  - `close_start` の上位ビットを使っている箇所は、`main` に無い。
  - 既存のフラグは、`start` / `open_end` / `close_end` の上位ビットにある。
  - `expand` は今のところ `close_start` をマスクしていない（`node.rs:29`）。ADR はマスクを U1 の作業としている。
  - サイズの assert は今 `<= 64` で、U1 でこれを `==` に強める。

  フラグを更新する場所は、子を足す 3 つのメソッド（`insert_child` / `splice_children` / `set_children`）の中に決まった。debug_assert、単体テスト（2a）、mutation（g）もある。
- **m3: 解消。** `check_append_among` は削除され、新しい公開 API は無い。`check_insertable` は AC をすべて拒否したままにする。その一致テスト（`worksheet.rs:780`）は、AC を含まない入力に限定される。
- **n1: 解消。** `ac_stats` は、`cur_before` で実際の greedy の `cur` を使うようになった。全 branch が空の AC は `ok_same_slot_ac` に数えられる。
  - spike を再実行した（xlsx `/tmp/poi-xlsx`、docx/pptx は README の手順で取得した POI のファイル、`reference/fixtures`）。
  - 出力はコミット済みの `results/ac-rank.txt` と完全に一致し、results は revision 2 から変わっていない。

## `child_mut` / `children_slice_mut` を U1 に残してよいか

`main` で、子を変更する呼び出しをすべて見た。

- **呼び出し元。**
  - `xml/mod.rs:189/197` と `move_subtree.rs:335`: path をたどって子に降りるだけ。
  - `package/add.rs:1393`: 属性の値を書き換えるだけ。
  - `lens.rs:1415`（`apply_prefixes`）: 子の prefix を書き換えるだけ。
- **要素名を書き換える箇所。** 3 箇所ある。
  - `lens.rs:212` と `same_part.rs:240` は、detach した木の root を `C::QNAME` に変える。`C` は `ChildOf` を満たす型なので、AC になることはない。
  - `lens.rs:1409` は prefix だけを変える。

したがって、`RawItem` を丸ごと差し替えたり、要素を AC に名前変更したりする経路は、今は存在しない。「U1 で確認し、debug_assert で検出する」という扱いで十分である。今この ADR で決める必要はない。

## Findings

### n1（nit）: `RawNode::close` は `close_start` を代入で上書きする。AC フラグの保存を明記したい

`node.rs:64` は `self.span.close_start = start as u32;` で、値をそのまま代入している。parser は `node.close(..)` を先に呼び、そのあとで `set_children` を呼ぶ（`parse.rs:201-205`）。このため、今の順序ではフラグは消えない。

ただし、この 2 つの呼び出しの順序を入れ替えると、フラグが黙って消える（AC があるのにフラグが立っていない状態）。parser を通る 2a のテストで検出できるので、nit とする。U1 では次のどちらかを入れるのが望ましい:

- `close` が、フラグのビットを保ったまま offset だけを書き込むようにする。
- 「`close` のあとに `set_children`」という順序を、コメントで明記する。

### n2（nit）: §4.3 の `child_mut` についての記述は、確認済みとして書ける

上の確認結果（`main` には、要素を丸ごと差し替える経路も、要素を AC に名前変更する経路も無い）を、§4.3 に 1 行で書けば、U1 の作業がひとつ減る。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）
