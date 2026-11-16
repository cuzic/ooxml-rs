# PR #49 レビュー（Opus, round 2）

対象: PR #49 `core/detach-same-part` @ `397bd86`。

round 1 は `276879f` をレビューした。これは #18 の detach の変種（prefix を選び直す方式）だった。その後、設計は**span を保つ clone**（#44 の move と同じ方式で照合する）に変わったので、今回は差分だけでなく、新しい設計を全体としてレビューした。

照合先:

- ADR-0016 rev 3/4 §3（`plan-pptx-phase3` @ `cd53c83`）。
- ADR-0016 round 2 レビューの N1〜N3。
- ADR-0002 note「same-part copy」。
- #44（`core-move-subtree` @ `5b8c0d7`）の `plan_move`。

方針: 読み取り専用の敵対的レビュー。作業は `git clone` した copy で行った。

- probe: scratchpad の `review-pr49-r2/pr49/crates/ooxml-core/tests/zz_r2.rs`（P1〜P9）。
- corpus 試験: `.../ooxml-schema/tests/zz_corpus_r2.rs`。
- MC の走査: `scripts/ac.py`（`python3 -I` で実行）。
- ミュータント35件: `scripts/mut.py`、結果は `mut.log`。

## 結論（先に）

**Major が 1 件ある。** `mc:AlternateContent` の分岐をまたぐコピーを拒否していない（P49R2-M1）。

- ADR-0016 §3 は、このコピーを「#44 の `check_move_in_by` と**まったく同じ**に照合する」と定めている。#44 は、移動元か移動先が `mc:AlternateContent` の中にあれば `AlternateContent` で拒否する。#49 にはこの規則が無い。
- その結果、`mc:Choice Requires="y"` の中の `a:rPr`（`y:` の要素を含む）を、分岐の外や `mc:Fallback` に `Ok` でコピーできる（probe P1）。y を理解しない consumer にとっては、無視できない未知の名前空間が現れることになり、MC の規則ではエラーになる。
- 実害の大きさ: corpus（重複を除いて 619 ファイル）で、AC の中にある `a:rPr` は 1 件だけで、a/r 以外の名前空間を使うものは 0 件だった。run の分割（同じ `a:p` の兄弟）では起こらない。ただし公開 API は一般的な用途に開いており、「意味を保つ」という性質が破れる。修正は #44 と同じ数行で済む。

**それ以外は健全である。**

- **意味の保存**: 次のケースは、すべて期待どおりに拒否されるか、通る。
  - 自由な prefix（要素名と属性名）。
  - 既定名前空間（未束縛、`xmlns=""`、束縛の変更）。
  - 宣言が影になる場合（root では自由に使い、子孫で再宣言する）。
  - コピー自身の宣言（挿入先で同じ prefix が別の URI でも通る）。
  - `mc:Ignorable`（prefix を経路の途中で再宣言する場合、コピー自身が宣言する URI の場合）。
  - 継承される `xml:space`（コピー自身の値がある場合、`default`、不正な値）。
- **check と apply の一致**: 5×5 の行列（P8）で `check_insert_under` と挿入の結果が一致した（Ok 6、Err 19）。Err のときは何も変わらない。Ok のときは、再 parse した結果が展開名のレベルで元と等しい。
- **T2 の局所性（corpus）**: 619 ファイルの slide、layout、master、notes にある `a:r/a:rPr` **全 47,464 件**（関係 id を持つものに限らない）をコピーできた。拒否は 0 件だった。
  - 出力は「元のバイト列に、連続した 1 箇所が挿入されたもの」で、その挿入の中に元の `a:rPr` のバイトがそのまま含まれていた（47,464/47,464）。
  - 別の段落への挿入 69 件も、check と insert の結果が一致し、元と構造的に等しかった。
- **文書の同一性**: round 1 の probe と P49-m2 の doc は今も成り立つ。`as_lens` から取ったコピーの試験（n1）も追加されている。
- **一意属性、MC、`extLst` の許可リスト**: ミュータントで確認した（下記）。

**判定: converged = no**（Major 1件）。M1 を直せば、残りは Minor と Nit だけになる。

---

## 要件との照合

| 要件 | PR @ 397bd86 | 判定 |
|---|---|---|
| N1: span を保つ clone。部分木のどこにある宣言も保つ | `RawNode` を span ごと clone する。`usage` は、コピーの中で宣言された prefix を自由な prefix から外す | 満たす（P3b、P3d、corpus で T2 が 47,464 件） |
| N1: #44 と同じ照合（自由な prefix、`mc:Ignorable`、`xml:space`、depth） | `Scope::at` と `check_destination` | **一部を欠く**。AC の規則が無い → **M1**。祖先の ProcessContent/MustUnderstand は #44 でも見ていない → n2 |
| MC 属性値の prefix | MC はコピーの中で拒否するので、値の prefix は起こらない | 満たす |
| 既知の限界: 属性値の中の QName | doc と ADR-0002 note に書かれている | 許容できる（Major ではない） → n3 |
| N2: `ext` の URI 許可リスト | すべての子が、同じ名前空間の `ext` で、許可された `uri` を持つこと。入れ子の `extLst` も再帰的に検査する | 満たす |
| N3: 残す拒否（`smtId`、`effectDag`/`cont@name`、MC） | root と子孫の両方で拒否する。拡張の中でも拒否する | 満たす |
| 関係 id は同じ part の中でしか有効でない | `DocToken`。clone と再 parse は新しい token になる。staged clone の扱い（P49-m2）は doc に書かれた | 満たす |
| depth の上限 | check と insert で同じ式を使う | 満たす。ただし insert 側の境界値の試験が無い → n4 |

---

## Major

### P49R2-M1 `mc:AlternateContent` の分岐をまたぐコピーを拒否しない（#44 との照合の差）

**根拠（probe P1）:**

```xml
<x:p xmlns:x="urn:t" xmlns:y="urn:y" xmlns:mc="…">
  <mc:AlternateContent>
    <mc:Choice Requires="y"><x:r><x:rPr><y:k/></x:rPr>…</x:r></mc:Choice>
    <mc:Fallback><x:r>…</x:r></mc:Fallback>
  </mc:AlternateContent>
  <x:r>…</x:r>
</x:p>
```

- Choice の中の `rPr` を `lens_at` で取り、外の run にコピーすると、check も insert も `Ok` になる。Fallback の中の run へのコピーも `Ok` になる。
- 出力の `<y:k/>` は、`Requires` による保護を失う。y は ignorable ではないので、y を理解しない consumer は MC の規則に従ってエラーにしなければならない。
- 照合は、prefix の束縛（同じ）と ignorable かどうか（どちらも ignorable でない）しか比べないので、すり抜ける。
- #44 の `plan_move`（`move_subtree.rs` L274-285）は、移動元の親と移動先のどちらかが MC の要素の中にあれば `MoveRefusal::AlternateContent` で拒否する。ADR-0016 §3 は「checked exactly like PR #44's same-part move」と書いており、PR の module doc も「validated the way a same-part move is (PR #44)」と書いている。

**影響:**

- corpus で AC の中にある `a:r/a:rPr` は 1 件で、a/r 以外の名前空間を使うものは 0 件だった（`scripts/ac.py`）。
- ADR-0016 の run の分割では起こらない。
- しかし、公開されている一般的な API で、壊れた文書を `Ok` で書けてしまう。

**推奨:**

- `detach_same_part_as` で「移動元の親までの経路に MC の要素がある」を記録する。`check_destination` で、挿入先の経路も同じように調べ、どちらかが真なら `NotDetachable { AlternateContent }` にする。#44 と同じ規則である。
- 試験に P1 の2ケース（Choice → 外、Choice → Fallback）を加える。どちらも check と insert の両方で拒否されることを確かめる。
- 同じ分岐の中のコピーまで拒否するかどうかは、#44 に合わせて「拒否」でよい（単純で安全）。

## Minor

### P49R2-m1 別の名前空間への改名で、挿入先の prefix を選ぶ経路の論理が誤っている（今は到達しない）

いま生成されている `SameType` の組は `rPr`/`endParaRPr` だけで、どちらも `a:` にある。そのため `root_prefix_at_destination` は今は通らない。しかし、手書きの spec で通すと、次の2つが誤る（probe P6）。

- **P6**: `u.free.remove(&root.name.prefix())` は、改名した root の prefix（`None`）を自由な prefix から外す。このとき、子孫の**既定名前空間**（接頭辞の無い要素）の照合まで一緒に消える。`<k/>` が urn:d から urn:other に変わったのに、check も insert も `Ok` になった。
- **P6b**: `choose_element_prefix` は挿入先の宣言だけを見る。コピーの root 自身の `xmlns:u="urn:zzz"` が、選んだ `u` を影にする。その結果、`<u:rPr2 xmlns:u="urn:zzz">` になり、root の名前空間が urn:zzz に変わった。

**推奨（どちらか）:**

1. 別の名前空間への改名を拒否し、その分岐を消す（推奨）。`C::QNAME.ns != E::QNAME.ns` なら `NotDetachable` にするか、コンパイル時の制約にする。
2. 直す場合は、次の2点を守る。
   - root 要素の名前は、`free` とは別に扱う。
   - prefix を選ぶときは、root 自身の宣言を、挿入先の宣言の内側として考慮する。

### P49R2-m2 祖先の走査が、main にすでにある helper を再実装している

`Scope::at` と `Scope::binding` は、main に #18 からある `ancestor_context(root, path, true)`（ignorable と `xml:space`）と、`declarations` + `effective` を書き直したものである。

- module doc の「PR #44 のものと、両方の merge 後に一本化する」は不正確である。#44 は main の helper をそのまま使っている。
- `height` も、`Detached::height` と #44 の `subtree_height` と合わせて、3重になっている。

**推奨:** この PR で main の helper を使う（差分は小さい）。`Usage`（#49 と #44 で別々にある）の一本化は、follow-up として issue にする。その際、次の差分をそろえる:

- AC の規則（M1）。
- 空白だけのテキストの扱い（n1）。
- MC 属性値の prefix（#49 では MC を拒否するので空になる）。

## Nit

- **n1** 空白だけのテキストの扱いが #44 と違う。
  - `xml:space="preserve"` の下で、子要素の間のインデント（空白だけのテキスト）があると、`InheritedXmlSpace` で拒否される（P5）。
  - #44 は、round 1 の m3b で、子要素を持つ要素の中の空白だけのテキストを「内容ではない」として除外した。
  - #49 の扱いは安全側だが、helper を一本化するときにどちらかにそろえる（ミュータント M33 が生き残るのはこのため）。
- **n2** 祖先にある `mc:ProcessContent` と `mc:MustUnderstand` は比べていない（P4d、P4e は `Ok`）。
  - ignorable な要素の中身を処理するかどうかや、理解が必須かどうかが、挿入先で変わり得る。
  - #44 も同じなので、#44 と合わせて既知の限界として1文書くか、共通の helper で比べる。
- **n3** 属性値の中の QName を既知の限界とすることは**許容できる**（Major ではない）。
  - `Detachable` の型は、`[detach]` の許可リストで schema を確認して選ばれる。対象の `CT_TextCharacterProperties` と `CT_TextParagraphProperties` には、QName 型の属性が無い。
  - 拡張の中身は URI の許可リストで個別に確認され、`hlinkClr@val` は列挙型である。
  - 将来 `[detach]` に型を足すときの確認項目として、「部分木に `xsd:QName` 型の属性が無いこと」を、許可リストの規則（ADR-0002 note）に1文足すとよい。
- **n4** 試験の穴（ミュータントで判明。どれも安全性は probe か目視で確認済み）:
  - M26: insert 側の depth の off-by-one が生き残る。check twin の境界しか試験していない。上限 3 の文書に、高さ 2 のコピーを run の下へ insert して `TooDeep` になることを試す。
  - M9: 要素名の自由な prefix を記録しない変異が、既存の試験では落ちない（probe P8 では落ちる）。要素の prefix だけが挿入先で別の名前空間になるケースを1つ足す。
  - M13: `mc:Ignorable` の prefix を、最も外側の宣言で解決する変異が生き残る。`<r xmlns:y="urn:y2" mc:Ignorable="y">` のように、Ignorable の要素自身で y を再宣言するケースを足す。
- **n5** `parent.own_dirty = true; // self-closing parent…` は、外しても出力が変わらない（M32、P9: `<x:r/>` への挿入は、正しく `<x:r><x:rPr b="1"/></x:r>` になる）。描画側がすでに扱っているので、行を消すか、必要な理由をコメントに書く。
- **n6** ADR-0002 note の corpus の記述（「83 `a:rPr` on slides」）に、次の1文を足すとよい。「関係 id を持つものに限らず、slide、layout、master、notes の全 47,464 件も、T2 を保ったままコピーできる（round 2 の review）」。

## ミュータント（`cargo test -p ooxml-core -p ooxml-schema --all-features --tests`）

35件のうち、26件を既存の試験が落とした。

| 生き残ったもの | 評価 |
|---|---|
| M9（要素名の prefix を自由な prefix にしない） | 試験の穴。probe P8 で落ちる → n4 |
| M10（未束縛の既定名前空間を `None` にする） | ほぼ等価。差は `xmlns=""` との比較だけで、安全側の偽の拒否になる |
| M13（Ignorable の prefix を外側の宣言で解決する） | 試験の穴 → n4 |
| M14（束縛を外側の宣言で解決する） | 試験の穴。probe P8 で落ちる |
| M15（移動元のスコープに root 自身を含める） | 等価。root の宣言、`xml:space`、MC は、コピーの中の扱いと同じ結果になる |
| M18（一意属性を E の分だけ見る） | 実質的に等価。生成された `rPr`/`endParaRPr` のリストは同一である |
| M22（root の `extLst` を見ない） | 等価。`Detachable` の型は `extLst` ではない |
| M26（insert の depth の off-by-one） | 試験の穴 → n4 |
| M32（自己終了の親の `own_dirty` を立てない） | 等価（P9） → n5 |
| M33（空白だけのテキストを継承扱いにしない） | 方針の差 → n1 |

落ちたもの（26件）: 文書の照合（insert と check）、clone の token、自由な prefix、ignorable、`xml:space` の照合、コピー自身の宣言、属性の prefix、属性の URI、コピー自身の `xml:space`、一意属性（root、子孫、既定値）、MC（root の属性、子孫の属性、要素）、`ext` の名前空間と URI、height、check の depth、挿入先の照合（insert と check、および照合を挿入の後に回す変異）、skip と改名の dirty 印。

## 判定

- **converged: no.** Major が 1 件ある（P49R2-M1: AC をまたぐコピー。#44 と同じ規則を足し、試験を2つ加える）。
- Minor の m1（別の名前空間への改名）は、分岐を消すのが最も簡単である。m2（helper）は、main の helper を使うだけで済む。
- Nit は任意である。ただし n4 の試験を3つ足すのは安価である。
- M1 を直せば、round 3 は差分の確認だけで足りる見込みである。

## 再現物

scratchpad の `review-pr49-r2/`:

- `pr49/`（`397bd86` の clone）
  - `crates/ooxml-core/tests/zz_r2.rs`（P1〜P9）
  - `crates/ooxml-schema/tests/zz_corpus_r2.rs`（`OOXML_SAME_PART_CORPUS=…/review-20r5-49/pptx.list`、`--release` で 74 秒）
- `scripts/mut.py`、`mut.log`
- `scripts/ac.py`（`python3 -I`）
