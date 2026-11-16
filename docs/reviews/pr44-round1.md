# PR #44 レビュー（Opus, round 1）

対象: PR #44 `core/move-subtree`（HEAD `a743d9f`）。`crates/ooxml-core/src/lens/move_subtree.rs`、`lens.rs` の
`place_skipping`、`error.rs` の `NotMovable`/`MoveRefusal`、`tests/move_subtree.rs`、ADR-0002 の note「same-part move」。

照合先:

- ADR-0001「Serialization rule」（`own_dirty` の規則）。
- PR #18 `core/detach-insert`（HEAD `cca838e`）の `ancestor_context`、`check_destination`、`detach_as`。
- PR #26 `core/insert-at`（HEAD `631fc90`）の `insert_at`/`check_insert_at`。
- ADR-0013 rev 3 §2（`plan-pptx-phase1b`、`c09e8b8`）。このノートの利用者で、`merge` が `a:p` を移動する。

独立プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-44-37r2/`
（worktree のコピー `pr44/`、`probe_move.rs`、変異体の元ファイル `orig.rs`）。Rust 1.99.1 で
`cargo test -p ooxml-core` はすべて通った。

## 結論（先に）

設計と実装は、述べている範囲ではおおむね正しい。

- **T2 型の局所性**: 成り立つ。移動した要素は span とフラグを保つので、出力は元の入力から
  「その要素のバイトが抜けて、移動先に現れた」ものになる。隣の空白テキストは元の位置に残る（P7）。
  これは note のとおりで、局所性とも矛盾しない。
- **dirty の付け方**: ADR-0001 と一致している。移動元の親は閉じタグを保ち、`own_dirty` を立てるのは、
  self-closing の移動先が最初の子を得るときだけ。
- **パスの補正**: 移動先のパスは、削除の影響を受ける場合に正しく補正される。同じ親の中での並べ替えでは、
  移動する要素自身をスロットの上限に数えない。
- **深さの判定**: パーサ、#18 と同じ式を使っている。上限ちょうどの移動は受理され、出力は再パースできる（P8）。
- **check と move の一致**: `plan_move` を共有しているので、構造上一致する。

ただし、「意味が変わる移動は拒否する」という中心の保証に穴が 1 つある（M1）。**属性値の中の
プレフィックス**（`mc:Ignorable`、`mc:Choice@Requires` など）を数えていないため、移動先で未束縛になる移動を
受理してしまう。

ほかに Minor が 6 件ある。

- 拒否の向きと境界をテストしていないため、変異体が 5 つ生き残る。
- 移動元の complex type を確認していない。
- 安全側に倒れた過剰な拒否がある。
- 古くなったパスで、黙って別の要素を移動してしまう危険がある。ADR-0013 の複数移動にかかわる。
- `mc:Choice`/`mc:Fallback` からの移動を許している。
- #18 と同じ処理を二重に持っている。

## 判定

**converged: no / Accepted after Minor only: no.** M1 を直し、m1 のテストを足した round 2 で収束すると
見込む。

---

## Major

### M1. 属性値の中のプレフィックス（Markup Compatibility）を照合していない

**根拠:**

- `usage()` が集めるのは、要素名と属性名のプレフィックスだけである。
- 一方、Markup Compatibility には、プレフィックスを**属性値**に持つ属性がある。`mc:Ignorable`、
  `mc:ProcessContent`、`mc:MustUnderstand`、`mc:PreserveElements`、`mc:PreserveAttributes`、それに
  `mc:Choice@Requires`（名前空間なしの属性）である。ISO/IEC 29500-3 は、これらの値のプレフィックスが
  宣言済みであることを要求している。
- このため、バイトは変わらなくても、移動先でプレフィックスが未束縛になれば意味が変わる。

**プローブ:** どちらも移動元の `x:box` だけで `xmlns:y` を宣言し、移動先の box では宣言していない。

- **P1**: `<x:item><mc:AlternateContent><mc:Choice Requires="y">…` を移動先へ動かすと `Ok` になる。出力では
  `Requires="y"` が未束縛になる。
- **P2**: `<x:item mc:Ignorable="y"/>` も同じく `Ok` になり、出力の `mc:Ignorable="y"` が未束縛になる。

**なぜ Major か:**

- note は、この照合を「意味を変える移動を拒否する」唯一の保証として書いている（「refused … when it would
  change their meaning」）。
- 利用者の ADR-0013 は、`a:p` を移動する。`a:p` には数式（`a14:m`）などの `mc:AlternateContent` が入り得る。
  `xmlns:a14` がルートや `mc:Choice` 自身にあれば実害はないが、途中の要素で宣言されていれば、壊れた
  ファイルになる。
- 直すのは安い。

**推奨:**

- `usage()` で、`MC_NS` の上の 5 つの属性と、`mc:Choice` の `Requires` の値を空白で区切る。各トークンの
  プレフィックスを集合に加える。`ProcessContent`/`PreserveElements`/`PreserveAttributes` のトークンは
  `p:local` または `p:*` の形なので、`:` の前を取る。
- 移動元と移動先で束縛を比べる（既存の `NamespaceBinding`）。
- P1/P2 をテストに加える。
- note の「a prefix used in the subtree (element or attribute…)」に「or named in a Markup Compatibility
  attribute value」を加える。

---

## Minor

### m1. 拒否の向きと境界をテストしていない（変異体 5 つが生き残る）

既存の `move_subtree.rs` に対して、手で変異体を作って流した。

| 変異 | 結果 |
|---|---|
| A: 深さ判定の `>` → `>=` | **生存**（上限ちょうどで成功するテストが無い。コメントには「fine at 4」とあるが、assert していない） |
| B: Ignorable の判定を「移動元だけ ignorable」に限る | **生存**（移動先だけ ignorable のケースが無い。プローブ P4 では正しく拒否された） |
| C: xml:space の判定を preserve → default の向きに限る | **生存**（default → preserve のケースが無い。P5 では正しく拒否された） |
| D: 部分木自身の `xml:space` は `preserve` だけを数える | **生存**（自身が `xml:space="default"` を持つ場合が無い。P5b では受理され、正しい） |
| F: `Scope::at` で Ignorable のプレフィックスを最も外側の宣言で解決する | **生存**（途中で再宣言されたプレフィックスが無い） |
| G: `binding()` から「既定名前空間が未宣言なら `""`」の腕を削る | 生存（過剰な拒否側にしか倒れない。優先度は低い） |
| H: 移動元の親に印を付けない | killed |
| I: self-closing の移動先に `own_dirty` を立てない | killed |
| K: 既定名前空間を照合しない | killed |

**推奨:**

- A〜F を殺すテストを加える。P8（上限ちょうど）、P4、P5、P5b、それに Ignorable のプレフィックスの再宣言。
- brute force のテストは、同じ名前空間・同じ空白の木しか回していない。宣言を変えた box を 1 つ加えて
  check と move の一致を回せば、照合の分岐も網羅できる。
- note の「16 mutants killed」は、追加した分に合わせて更新する。

### m2. 移動元の complex type を確認していない

**根拠:**

- `plan_move` が照合するのは `C::QNAME` だけである。
- 同じ QName でも、文脈によって complex type が違う要素がある。例えば `w:pPr/w:rPr`（CT_ParaRPr）と
  `w:r/w:rPr`（CT_RPr）がそうである。このような要素を動かすと、移動先でスキーマ違反の内容になる。
- #18 は、`SameType<E>` でこの問題を型で防いでいる。

**推奨:** 次のどちらかにする。

- rustdoc と note に「`from` の要素が `C` の complex type であることは呼び出し側の責任」と書く
  （#18 の `Lens::cast` の doc と同じ書き方）。
- 型引数 `S: SameType<C>` を取る形にする。

### m3. 安全側に倒れた過剰な拒否（2 種）

- **(a) 部分木が自分で宣言したプレフィックス（P3）**: `<x:item xmlns:y="urn:own" y:k="1"/>` を、外側の `y` の
  束縛が違う box へ動かすと `NamespaceBinding` で拒否される。note は「declarations inside the subtree travel
  with it」と書いているが、照合は部分木の外の束縛で行っている。`usage()` で部分木の中の宣言を追跡し、
  **自由な**プレフィックスだけを照合すれば直る。
- **(b) 要素だけを含む内容の、インデントの空白（P6）**: 継承された `xml:space` に依存するテキストとして数えられ、
  拒否される。OOXML ではまれなので、doc に書くだけでもよい。

### m4. 古くなったパスと、複数の移動の事前検証（ADR-0013 `merge`）

**根拠:**

- ADR-0013 §2 は、複数の `a:p` を移動する。その「Atomicity」の節は、すべての移動の検証を最初の変更より前に
  行うことを求めている。
- 一方、`check_move_in_by` は「現在の木」の上でしか検証しない。
- 1 回目の移動の後は、移動元の後ろの兄弟（とその子孫）のパスがずれる。記録しておいた `from` を使い回すと、
  `WrongElement` にはならず、**同じ QName の別の `a:p` を黙って移動する**ことがある。
- ADR-0013 は「paths … are stale afterwards」と書いているが、core 側の rustdoc には何も書かれていない。

**推奨:**

- `move_in_by` の rustdoc に、どのパスがずれるかを書く。ずれるのは、移動元の親の中で後ろにある兄弟、
  移動先の親の中で挿入位置以降にある子、およびそれらの子孫である。
- 同じ親から複数を動かすときの安全な順序も書く。後ろから動かすか、毎回パスを取り直す。
- 「先行する移動で変わらない検証」と「変わる検証」を note に書く。そうすれば、呼び出し側がまとめて
  事前検証できる。
  - 変わらない: `NamespaceBinding`、`IgnorableNamespace`、`XmlSpace`、`TooDeep`。どれも祖先の属性だけで
    決まり、移動は属性を変えないから。
  - 変わる: スロットの上限、順序、`IntoItself`。

### m5. `mc:Choice`/`mc:Fallback` の中からの移動を許している

**根拠:**

- `from` の祖先に `mc:AlternateContent` があっても、拒否しない。
- 代替分岐の片方から要素を出すと、どちらの分岐を選ぶ consumer にとっても内容が変わる。その要素が片方では
  消え、もう片方では重複して見える。
- 移動先にある `mc:AlternateContent` の兄弟は `UnresolvedSibling` になる（P11）。一方、移動元の側には
  何の照合も無い。
- #18 の `detach_as` は MC を全面的に拒否しており、Phase 0 にも `InsideAlternateContent` がある。

**推奨:** 次のどちらかにする。

- `from`（と移動先）の祖先に `MC_NS` の要素があれば、`MoveRefusal::AlternateContent` で拒否する。
- 呼び出し側の責任として doc に書く。

### m6. #18・#26 との整合

**#18 との重複:**

- `element_at`（同じコード）、`MC_IGNORABLE`、`Scope::at`（#18 の `ancestor_context` と同じ走査）、
  `height`（#18 の `Detached::height`）が二重にある。
- `lens.rs` の import と `error.rs` の追加位置も競合する。
- PR 本文には「後から入る方で共通化する」と書いてある。ADR-0002 の note にも 1 文入れておく。

**xml:space の照合の違い:** #44 は両方向で拒否し、#18 の `check_destination` は preserve の側だけを
拒否する。

- #18 は、移動元で preserve を継承していれば `detach_as` の時点で拒否する。だから、移動先の照合は
  preserve の側だけで足りる。
- #44 は、バイトをそのまま保つので両方向が必要である。
- 違いは正当である。

**#26 との関係:**

- #44 には位置で指定する `move_in_at` が無い。後から足すなら、`check_insert_at` の `place(…)` 呼び出しに
  `skip` を渡す必要がある（同じ親の中の移動）。
- それ以外の競合は無い。#26 は `place` を呼ぶだけで、シグネチャは変わっていない。

---

## Nit

- `the_destination_path_follows_the_removal` に、使っていない `let a` と `let _ = a;` が残っている。
- 深さのテストのコメント「moving item … under box1 is fine at 4」は、assert されていない（m1-A）。
- `IntoItself` は、`from == self.path`（自分自身の中へ）も含む。rustdoc の「this element or one of its
  ancestors」は正しいが、エラー文言の「into itself」と並べて読むと分かりにくい。
- 移動先がスロットの末尾になる場合、移動した要素は末尾の空白テキストの**後ろ**に入る（P7:
  `\n <x:item k="1"/></x:box>`）。局所性の上は正しい。整形の見た目について 1 文あってもよい。

## 確認して問題なしとしたもの

| 観点 | 結果 |
|---|---|
| 同じ親の中の並べ替え | `place_skipping` が移動する要素を除いて数える。`dst_index` の補正は、`skip < index` のときだけ。`skip == index` は起こらない（スキップしているため） |
| 移動先パスの補正 | `dst_parent[src_parent.len()] > src_index` のときだけ 1 減らす。`==` の場合は `IntoItself` で先に拒否される。このため、`>=` に変える変異は等価になる |
| 孫を祖父母へ移す（移動先が移動元の祖先） | 補正は不要。挿入位置は削除より深い場所に影響しないので正しい |
| 作成済みノードの移動（P10） | 作成時に選んだプレフィックスで照合され、正しく描画される |
| no-op の移動（P13） | 出力はバイト同一。dirty は立つが、出力には影響しない |
| 拒否時 | ツリーは不変で、`subtree_dirty` も立たない（テストと brute force の両方で確認） |
| ADR-0001 の `own_dirty` 規則 | 移動元の親は子を持っていたので、self-closing ではあり得ない。移動先が self-closing の場合だけ `own_dirty` を立てる。writer 側にも、立て忘れたときの保険がある |
