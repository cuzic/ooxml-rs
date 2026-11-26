# PR #157 レビュー（Opus, round 1）

対象: `impl/xlsx-ej-c7` @ `0b65826e84a410a5fb8c1fb72c66a0e41d354aed`（ADR-0024 §7.4 C7、§7.1 Canonical cell form、§7.2 step 5、oracle 11）

検証:

- `cargo test -p ooxml-xlsx --lib`: PR のテストはすべて ok（プローブを除く 117 passed）。CI の test/clippy はレビュー時点で pending。
- プローブ（コミットしない）: 整形済み（改行・字下げ入り）の sheet、`x:` 接頭辞の sheet、`ph` と未知属性を持つ cell、C2 の `restore_cell`（content restore）経由の cell、奇妙な引用符・空白・文字参照（`s = '&#48;'`、タブ区切り）の Base 開始タグ、epoch 境界での再パース。
- 手書きの変異 2 個（下記）。

## 結論（先に）

converged: **no**（Major 1）。コードの挙動は、ADR の前提（Base の parse source はセッションの Base。structural op では再パースしない。C6 / (A′) で保存後だけ取り替える）の下では §7.1 の C7 規則どおりである。C2 の `restore_cell` 経由の cell、奇妙な引用符・空白・文字参照の開始タグ、`x:` 接頭辞、行 index と dirty カウントの整合も、プローブで正しく動いた。Major は、モジュール文書と PR 本文が「epoch 境界ごとに再パースする」ことを正しさの前提としている点である。これは逆で、その前提に従うと `Ok` のまま C7 規則を破る（M1）。ほかは、整形済み source の空白だけの子（m1）、テストの穴（m2, m3）、未知属性を持つ cell が自己終了にならない件（m4）。

## 前ラウンドの解消状況

round 1 のため、なし。

## Findings

### M1（Major）: 「epoch 境界で再パースする」という前提が逆で、それに従うと C7 規則を `Ok` のまま破る

- 根拠:
  - `crates/ooxml-xlsx/src/canonical.rs` L20-23 には「A node restored to a later epoch's snapshot keeps the span of the source it was parsed from, which is that epoch's own Base; the Journal re-parses the part at each epoch boundary, B.2」とある。PR 本文の Open questions「Later-epoch restores」も、epoch 境界ごとの re-parse（または rebase）を正しさの条件にしている。
  - ADR の規則は「cell の **Base node** に `s` があったか」である（§7.1 Canonical cell form、§7.4 C7）。epoch 境界での再パース（B.2 の `materialize`）は spike の代用品であり（§7.4 C2 の「text layer」列、B.2 表）、本設計にはない。§4.4 では、structural op は snapshot を閉じるだけで、node は dirty のまま元の span を保つ。parse source を取り替えるのは保存後の C6 / (A′) だけである。
  - epoch 境界で再パースすると、`base_has_s`（L96-100）は materialize された中間状態の開始タグを読む。Base node の開始タグは読まない。プローブ `probe_epoch_reparse` で確かめた:
    1. Base が `<c r="A1"><v>1</v></c>`（`s` なし）。epoch 0 で `Number(9)` を書き、`s="7"` にする。
    2. シリアライズして開き直す（materialize）。
    3. epoch 1 で `Number(8)` を書き、`s="0"` にする。
    4. `canonicalize_cells` の出力は `<c r="A1" s="0"><v>8</v></c>`。ADR の規則では、Base node に `s` がないので `<c r="A1"><v>8</v></c>` になるはずである。
  - 同じ前提の下では、ADR round 9 m3 も満たせない。m3 は「carried mark を持ち、後の epoch の snapshot に R2 で戻された cell も対象」とする。再パースで clean になった node に戻すと、`subtree_dirty` の判定（L173）で飛ばされてしまう。一方、再パースしない本設計なら、carried mark を持つ cell の後 epoch snapshot は epoch 0 で dirty になった node の clone である。`restore_cell` もその dirty 状態ごと戻すので、L173 で正しく対象になる。
  - つまり、このコードが正しく動くのは ADR の設計の下であって、文書が述べる前提の下ではない。J0/J2 の実装者が文書に従って materialize を入れると、何も検出されないまま出力を誤る。
- 修正案:
  - L13-29 と PR 本文を次の内容に直す。「`XmlDocument::source()` は、セッションの Base（epoch 0、または C6 / (A′) で取り替えた直前の保存の出力）でなければならない。structural op で再パースしてはならない。再パースしてよいのは保存後だけである。」
  - 上の手順を回帰テストとして残す（再パースしない場合に `<c r="A1"><v>8</v></c>` になること）。structural op をまたいで後 epoch の snapshot に戻した cell のケースは、oracle 11 として J2 に回す旨も書く。

### m1（Minor）: 整形済み source の cell をクリアすると、空白だけの子が残り、自己終了にならない

- 根拠:
  - L186 は `cell.children().is_empty()` で「子がない」を判定する。空白だけのテキスト項目も子に数えられる。
  - Base が `<row r="1">\n  <c r="A1" s="3">\n    <v>1</v>\n  </c>\n</row>` のとき、`Clear` の後の出力は `<c r="A1" s="3">\n    \n  </c>` で、`<c r="A1" s="3"/>` にならない（プローブ `probe_pretty_clear_vs_write_then_clear`）。
  - 履歴 `Clear` と履歴 `Number(9)` → `Clear` の出力は同じだったので、oracle 11 の破れは見つからなかった。ただし §7.1 の「a childless element is self-closing」は満たしていない。F11 の痕跡 `<c …></c>` に相当するものが残る。
- 修正案: 子が空白だけのテキスト項目からなる `c` も「子なし」として作り直す。作り直しでテキストは落ちる。テストを 1 件足す。

### m2（Minor）: 作り直しで `cm` / `vm` / `ph` を写すことがテストされていない

- 根拠:
  - 変異「L189-196 で `r` と `t` だけを写す」（`cm` / `vm` / `ph` を黙って落とす）は、PR の 11 テストを全部通る。
  - 実装自体は正しい。プローブで `ph="1"` 付きの cell を `Clear` すると `<c r="A1" ph="1" s="3"/>` になり、`ph` は保たれ、その後の `edit_cell` も正しい位置に効いた。
- 修正案: `ph` を持つ cell のクリアを canonicalize し、`ph` が残ることを見るテストを足す。`cm` / `vm` は metadata part の付いた fixture で同じことを見る。

### m3（Minor）: C2 の `restore_cell` 経由の cell と、structural op でずれた Base cell のテストがない

- 根拠:
  - PR のテストは、どれも `edit_cell` と手書きの `set_attr` で状態を作っている。
  - 依頼された観点の中心は「`restore_cell` で戻した node は span を保ち、Base の `s` を正しく読めるか」だが、これを固定するテストがない。プローブ `probe_restore_content` では正しく動いた。手順は次のとおり。
    - A1（Base に `s` なし）と B1（Base が `s="0"`）を snapshot する。
    - それぞれ書き換え、A1 を `s="0"` にし、B1 から `s` を外す。
    - どちらも `RestoreStyle::Current` で戻す。
    - canonicalize すると、`<c r="A1"><v>1</v></c><c r="B1" s="0"><v>2</v></c>` になる。
  - `insert_rows` で `r` が書き換わった（dirty で、span は元のまま）Base cell のケースもない。
- 修正案: 上の手順をそのままテストにする。行挿入でずれた `s="0"` の Base cell に値を書いて、`s="0"` が残るケースも足す。

### m4（Minor）: `r`/`s`/`t`/`cm`/`vm`/`ph` 以外の属性を持つ子なし `c` は作り直されず、`<c …></c>` のまま残る

- 根拠:
  - L182-186 の `copyable` の判定による。プローブでは `<c r="B1" s="3" foo="1"></c>` がそのまま残った。属性自体は失われない。
  - §7.4 C7 は「a changed `c` with no children is replaced by a fresh empty-element node」と例外なしに述べている。PR 本文の Open questions に書いてあるとおり、ADR からの逸脱である。CT_Cell にはこれ以外の属性がないので、実害は schema 外の入力に限られる。
- 修正案: 次のどちらかにする。
  - 名前空間宣言でない属性をすべて写す。`set_attr` が prefix を選ぶ。宣言が `c` 自身にある場合だけ作り直しをやめる。
  - この例外を ADR（§7.4 C7 か J1 の記録）に書き足す。

### n1（nit）: 作り直した `c` で `s` が属性の最後に来る

- 根拠: L189-196 では `s` を除いて写し、L214-216 で `s` を最後に足す。そのためプローブでは `<c r="A1" ph="1" s="3"/>` になった。`t` があれば `r t s` の順になる。Excel や Layer 2 の既定の順（`r s t cm vm ph`）と違い、作り直さなかった cell とも見た目が揃わない。
- 修正案: `attrs` を作る段階で `s` を `r` の直後に入れる。

### n2（nit）: 「changed」と「`subtree_dirty`」を同一視している

- 根拠:
  - L13-16、L117-118、PR 本文の「Which cells」は、dirty かどうかで「changed」を判定するように読める。しかし、structural op で `r` だけ書き換わった Base cell は、dirty だが changed set には入らない。
  - 呼び出し側が changed set 以外（たとえば part の全 cell）を渡すと、その cell の `s="00"` が `s="0"` に書き換わり、子なしなら作り直される。
- 修正案: 「呼び出し側は §4.4 の changed set を渡す。dirty の判定は、Base に戻った cell を除くためのフィルタにすぎない」と書く。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 4 / nit 2）
