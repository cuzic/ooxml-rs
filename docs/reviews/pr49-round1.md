# PR #49 レビュー（Opus, round 1）

対象: PR #49 `core/detach-same-part` @ `276879f`（push 済みの head）。main は `48aa8f0`。

照合先:

- ADR-0016 revision 3（PR #37 @ `7a138eb`、§3、§11）と、その round 2 レビューの N1〜N3（`plan-pptx-phase3/docs/adr/0016-opus-review-round2.md`）。
- PR の ADR-0002 post-acceptance note「same-part copy」。

注意: worktree `core-detach-same-part` は、別のセッションが作業中だった（HEAD `7b71ae6`、未 push、`lens.rs` などが変更中）。そのため、このレビューは worktree ではなく、`git clone` した push 済みの head（scratchpad の `review-20r5-49/pr49`）を対象にした。

方針: 読み取り専用の敵対的レビュー。次のものを使った。

- probe `zz_probe49.rs`（ooxml-schema の実 DML 型で、24 ケース）。
- corpus 試験の拡張版 `zz_corpus49.rs`（slide に加えて layout、master、notes も対象）。
- ミュータント20件（`mutants49.py`）。
- 履歴の tree 比較。

## 結論（先に）

**Blocker と Major は無い。** 安全性に関わる性質は、どれも成り立っている。

- **文書の同一性（identity）は健全である。** `WrongDocument` になる場合を確かめた。
  - 別の文書、clone、同じ bytes の再 parse、同じ package の別の part（`xml_part_mut("slide2")`）。
  - `LensMut::root`、`Lens::at`。
  - check twin でも同じ結果になる。
  - 文書を move した場合は、identity も一緒に移る。copy の id は、その文書自身の内容から取ったものなので、これは健全である。
  - `set_part` で part の bytes を置き換えると、再 parse で新しい identity になる。
- **一意属性**（`smtId` の 0 以外の値、`effectDag@name`）は、許可された拡張の**中でも**拒否される。MC（`AlternateContent`、`mc:Ignorable` 属性）と ignorable な名前空間も、拒否されたままである。
- **check/apply の一致**: 24 ケースすべてで、`check_insert_under` と `insert_same_part` の結果が一致した。ただし slot の検査は除く（#18 から、check twin の対象外と doc に書かれている）。
- **T2 の局所性**: 挿入は純粋な挿入で、元の run の bytes は変わらない。
  - 宣言を `ahyp:hlinkClr`、`a:ext`、root のどこに置いても、`a` を拡張の中で別の名前空間に束ね直しても、`r` を束ね直しても、既定名前空間とその解除（`xmlns=""`）、コメントと PI があっても、出力を再 parse した結果は、展開名のレベルで元と等しかった。
- **corpus**: 621 ファイル（重複を含む）の slide、layout、master、notes にある、関係 id を持つ `a:rPr` のコピーは **180/180** 成功し、再 parse した結果は元と構造的に等しかった。そのうち 30 件は拡張を持っていた。
- **履歴**: 次の2点を確かめた。#18 のコードは逆戻りしていない。
  - `176a2ac` の tree は、#18 の merge（`131f7b3`）に、この PR の10ファイルを足したものと一致する。
  - main との正味の差分は、同じ10ファイルの追加だけである。削除行は、シグネチャを変えた行だけだった。
- **ミュータント**: 20件のうち18件が既存の試験で落ちる。生き残った2件は次のとおり。
  - N17: 等価ミュータント。
  - N9: 安全側に倒れる。

ただし、**設計が ADR-0016 rev 3 §3 の文面と食い違っている**（P49-m1）。

- ADR は、「span を保つ clone」で、部分木の中の名前空間宣言はどこにあっても許す、と書いている。
- PR は、#18 の detach の変種である。prefix を挿入先で選び直し、宣言は**許可された拡張の中だけ**に限る。
- 実害は corpus に無い（宣言を拡張の外に持つ `a:rPr` は 58,076 件中 0 件）。拒否は安全側である。
- しかし、2つの文書が逆のことを言っている状態は、ADR-0016 を Accepted にする前に解消する必要がある。

**判定: converged = yes**（Minor と Nit だけ）。P49-m1 は、ADR-0016 rev 4（PR #37）の側で文面を実装に合わせることを推奨する。

---

## 要件との照合

| 要件 | PR | 判定 |
|---|---|---|
| ADR-0016 rev 2 M2: `r:` id と `a:extLst` を保つ same-part copy | `detach_same_part_as`。`RELATIONSHIP_ATTRIBUTES`（Transitional/Strict の `r:`、`o:relid`）を値ごと保つ | 満たす |
| N1: `ahyp:hlinkClr` の、自分の要素の上の `xmlns:ahyp` | 許可された拡張の中の宣言は、そのまま保たれる。`with_prefixes` は、それを最も内側の束縛として扱う | 機能としては満たす（corpus の 14/14）。「span を保つ clone」ではない → P49-m1 |
| N1: 照合（自由な prefix、`mc:Ignorable`、`xml:space`）は #44 と同等 | `check_destination` と `with_prefixes`（#18 のもの）。MC は拒否するので、MC の属性値の prefix は起こらない | 同等。ただし、拡張の中の属性値にある QName は見ない → n3 |
| N2: `a:ext` の URI 許可リスト | `SamePartOptions::keep_extension(uri)`。ASCII で大文字小文字を区別しない。すべての子が、同じ名前空間の `ext` で、許可された `uri` を持つこと | 満たす。入れ子の `extLst` も、同じ規則で再検査される（probe M2） |
| N3: 残す拒否（`UniqueAttribute`、MC） | 残る。拡張の中にある `a:effectDag@name` と `a:rPr@smtId` も拒否される（probe H、H2） | 満たす |
| 関係 id は同じ part の中でしか有効でない | 型を分けた（`SamePartDetached`。`compile_fail` の doc test がある）。実行時は `DocToken` で照合する | 満たす。限界は P49-m2 と、doc に書かれた「取得から挿入までの間の関係の削除」 |

## Identity の probe（`zz_probe49.rs`、`package_identity`）

| ケース | 結果 |
|---|---|
| `doc.clone()` への挿入、または check | `WrongDocument` |
| 同じ bytes を再 parse した文書 | `WrongDocument` |
| `LensMut::root(&mut raw)` | `WrongDocument` |
| `Lens::at(..)` からの `detach_same_part_as` | `WrongDocument` |
| 同じ `Package` の別の part（slide1 → slide2、`xml_part_mut` と `lens_mut_at`） | `WrongDocument` |
| move した文書（`let doc2 = doc`） | `Ok`（健全。上の説明を参照） |
| `lens_mut_at(..).as_lens()` からの detach | `Ok` |
| 同じ part で、すでに `a:rPr` を持つ run への挿入 | check は `Ok`、insert は `AlreadyPresent`（slot は check の対象外。#18 の doc どおり） |
| `rPr` → `endParaRPr`（`SameType`） | check と insert が両方 `Ok`、純粋な挿入 |

## ミュータント（`mutants49.py`、`cargo test -p ooxml-core -p ooxml-schema --all-features --tests`）

| ID | 内容 | 落とした既存の試験 |
|---|---|---|
| N1 | insert で文書を照合しない | `a_same_part_copy_goes_only_into_its_own_document` |
| N2 | check twin で文書を照合しない | 同上 |
| N3 | clone が同じ token を持つ | 同上 |
| N4 | token を定数にする | 同上 |
| N5 | `lens_at` が文書を持たない | `path_lenses_know_their_document`、`hyperlink_run_properties_copy_within_the_part` |
| N6 | `lens_mut_at` が文書を持たない | `path_lenses_know_their_document` |
| N7 | `child_lens` が文書を落とす | 6件 |
| N8 | `descend` が文書を落とす | 5件 |
| **N9** | `as_lens` が文書を落とす | **生存**（→ n1） |
| N10 | uri のリストが空でも `extLst` を許す | `declarations_inside_an_allowed_extension_are_kept` |
| N11 | `ext` の名前空間を見ない | `extension_lists_only_with_an_allowed_uri` |
| N12 | `ext` の局所名を見ない | 同上 |
| N13 | uri の大文字小文字を区別する | 同上 |
| N14 | 宣言をどこでも保つ | 2件 |
| N15 | 拡張の中の宣言を拒否する | 2件 |
| N16 | 保った宣言を、prefix の選択で無視する | 2件 |
| **N17** | 保った宣言の depth を 0 にする | **生存**。等価ミュータントである。`effective` は depth ではなく vec の順で最も内側を選ぶので、depth は「正しい prefix の候補のうち、どれを選ぶか」の順序にしか効かない |
| N18 | 別 part 用のモードでも id を保つ | 4件 |
| N19 | `xmlns` 属性にも prefix を選び直す | 2件 |
| N20 | 文書を知らない lens からでも detach できる | `a_same_part_copy_goes_only_into_its_own_document` |

---

## Minor

### P49-m1 ADR-0016 rev 3 §3 と実装の意味論が食い違っている

**根拠:**

- ADR-0016 rev 3 §3（L213-245）の記述:
  - 「It is **not** a variant of Phase 0.1's `Lens::detach_as` … It is a **span-preserving clone**: … its bytes — prefixes and the declarations inside it included — are written unchanged」
  - **Lifted**: 「namespace declarations inside the subtree」
  - `FormatNotCopyable`（§11 L560）の対象は、`smtId`、`effectDag`/`cont@name`、許可リストにない `a:ext`、MC だけである。
- PR の記述（ADR-0002 note、README、`detach_same_part_as` の doc）:
  - prefix を消して挿入先で選び直す（#18 と同じ）。
  - 宣言は**許可された拡張の中だけ**保つ。
- probe での違い:
  - `<a:rPr xmlns:a="…">`（L）と `<a:hlinkClick xmlns:r="…" r:id=…>`（L2）は、`NotDetachable(NamespaceDeclaration)` になる。ADR rev 3 の文面では、これらはコピーできるはずである。
  - `extLst` の上の宣言（C）も拒否される。
  - 出力の prefix は、挿入先の規則で決まる。分割で作る兄弟の run では、元と同じになる。
- 実害の大きさ: 621 ファイル（slide、layout、master、notes）の `a:rPr` 58,076 件のうち、宣言を拡張の外に持つものは **0 件**、拡張の中だけに持つものが 14 件だった。どちらの方式でも、corpus の結果は同じになる。拒否は `FormatNotCopyable` になるだけで、壊れたファイルは作らない。
- PR の本文は「Requested by ADR-0016 rev 2」と書いており、rev 3 の N1 の書き方に追従していない。

**推奨**（どちらか）:

1. **（推奨）ADR-0016 rev 4 で、§3 を実装に合わせる。**
   - 「prefix を選び直すコピー（#18 の変種）。部分木の宣言は、許可された `a:ext` の中だけ保つ。ほかの場所の宣言は `FormatNotCopyable`（corpus で 0 件）」と書く。
   - §11 の `FormatNotCopyable` の行に、「拡張の外の名前空間宣言」を足す。
   - PR 本文の「rev 2」を「rev 3/4」に直す。
2. 実装で、拡張の外の宣言も許す。
   - `in_ext` を常に真にして、`with_prefixes` の own decls の仕組みを全体に使う。
   - 試験 `document_unique_attributes_and_the_other_refusals_still_apply` の宣言の拒否を外す。

1 で足りるのは、span を保つ clone の利点（バイトの保存）が、分割で作る run には無関係だからである。新しい run は T2 では作られた内容として扱われ、元の run のバイトは変わらない。

### P49-m2 「clone は別の文書」と、ADR-0012/0016 の transaction（clone を編集する）の関係を書く

- ADR-0016 §1 原則 6 では、すべての書き込みは「parse 済みの木を clone し、clone を lens で編集し、`stage_part` する」という形をとる。
- clone は新しい identity を持つ。そのため、**元の文書**（`pkg.xml_part(..)`）から取った copy は、同じ part になる clone に挿入すると `WrongDocument` になる。§3 の「最初の分割の前の検査」で、check twin に元の文書を渡しても同じである。
- これは安全側（偽の拒否）なので、正しさの問題ではない。ただし、format crate の実装者が最初に踏む罠である。
- 推奨:
  - `SamePartDetached` の doc と ADR-0002 note に、「transaction では、copy は staged の clone から取り、check twin にもその clone を渡す」と1文足す。
  - ADR-0016 §3 の「Checks before the first split」にも同じことを書く。

---

## Nit

- **n1** ミュータント N9（`LensMut::as_lens` が文書を落とす）が生き残る。結果は偽の `WrongDocument` なので安全側だが、doc は「lenses derived from it carry the token」と約束している。`lens_mut_at(..).as_lens().child::<RPr>()?.detach_same_part_as(..)` が `Ok` になることを assert する1行を、`path_lenses_know_their_document` に足すとよい。
- **n2** `ext_uri::HYPERLINK_COLOR` の doc にある「12 in the 619-file pptx corpus」は、slide だけを数えた値である。ADR-0016 round 2 の数え方では、layout と master を含めて 14 になる（今回の走査でも 14）。どちらを数えたかを書く。ADR-0002 note の「83」も、slide だけを数えた値と明記する（今回の拡張版では、重複を含めて 180）。
- **n3** 拡張の内容は自由なので、属性値の中の QName（例: `type="foo:T"`）は prefix を選び直しても書き換わらず、照合もされない。分割で作る兄弟の run ではスコープが同じなので起きない。ほかの使い方では、挿入先が `foo` を別の名前空間に束ねていると、意味が変わる。ADR-0002 note に、既知の限界として1文足す（MC の値は MC の拒否で防がれている）。
- **n4** 履歴: `7cbda0c`〜`bb2c063` の3コミットの tree には、#14/#26 の merge の内容（`generated/wml.rs` の 4,879 行、`package.rs` など、5,835 行の削除）が無い。正味の差分は正しいが、通常の merge commit で取り込むと、bisect がこの区間で main より古い状態を見る。**squash merge** を推奨する。force-push は不要。
- **n5** `SamePartDetached::check_insert_under` の doc に、「slot（`AlreadyPresent` など）は検査しない」と、`Detached::check_insert_under` と同じ一文を繰り返しておく。probe では、check は `Ok`、insert は `AlreadyPresent` になった。

## 判定

- **converged: yes.** Blocker/Major は無い。identity、一意属性、MC、ignorable、check/apply の一致、T2、#18 の非退行は、どれも probe と corpus で確認した。
- P49-m1 は、ADR-0016 の側（rev 4）で解消する。PR 側では、本文の「rev 2」を直し、P49-m2 の doc の1文を足すだけで足りる。

## 再現物

scratchpad の `review-20r5-49/`:

- `pr49/`（push 済みの head の clone）
  - `crates/ooxml-schema/tests/zz_probe49.rs`（`ZZ_PPTX=…/pptx-p1/corpus-lo/hyperlinkOnImage.pptx` で package の probe も走る）
  - `crates/ooxml-schema/tests/zz_corpus49.rs`（`OOXML_SAME_PART_CORPUS=…/review-20r5-49/pptx.list`）
- `mutants49.py`、`mut49.log`、`mut49b.log`
- `scripts/rpr.py`（`a:rPr` の宣言と拡張の走査。`python3 -I`）
