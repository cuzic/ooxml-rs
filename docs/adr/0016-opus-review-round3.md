# ADR-0016 レビュー（Opus, round 3）

対象: PR #37 `docs/adr/0016-pptx-phase3-formatting-and-links.md`（Proposed, revision 4, HEAD `cd53c83`）。

照合先:

- round 2 の指摘（`0016-opus-review-round2.md`）。N1〜N7 と Nit。
- PR #49 `core/detach-same-part`（HEAD `397bd86`）。`crates/ooxml-core/src/lens/same_part.rs`、
  `docs/reviews/pr49-round1.md`（P49-m1、P49-m2）。
- PR #38 `core/add-part`（HEAD `68a02c1`）。`package/add.rs` と `docs/reviews/pr38-round2.md`（N1、N2）。
  worktree には、N1 と N2 を直す未コミットの変更があった。判定には使っていない。
- main（`ab1b0b6`）の ADR-0008（Accepted rev 3）、ADR-0012（Accepted rev 3）、ADR-0013（Accepted rev 4）、
  ADR-0002。

独立プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr37-r3/`

- `c49/` は、`core-detach-same-part` を `git clone` して `397bd86` を checkout したもの。
- `c49/crates/ooxml-schema/tests/zz_r3_probe.rs` は P1〜P4 を試す。
  `cargo test -p ooxml-schema --features dml --test zz_r3_probe` で 4/4 が通った。

## 結論（先に）

**round 2 の N1〜N7 は、すべて解消した。P49-m2 の規則も入っている。** Major 以上の指摘は無い。

- §3 が名前を挙げる #49 の API は、`397bd86` にすべてあり、意味も文面どおりだった。
  - `Lens::detach_same_part_as`、`SamePartOptions::keep_extension`、`lens::ext_uri::HYPERLINK_COLOR`、
    `LensMut::insert_same_part_by`、`SamePartDetached::check_insert_under`、`Error::WrongDocument`。
  - 「span を保つ clone」、許可リスト、残す拒否が、実装と一致した。
- rev 4 の経緯の記述は正しい。#49 round 1 のレビューは `276879f` に対して行われ、`cff3623` はその子孫にあたる。
  `cff3623` で clone の設計に書き直された。P49-m1 は、#49 の側が ADR に合わせて解消した。

新しい指摘は Minor 4 件と Nit 数件で、どれも文面の修正で閉じる。

- 3 件は、main で Accepted になった ADR-0008 と ADR-0013 との食い違いである。
  - §10 の `Source::Theme` と `themeOverride`。
  - §9 と Q4 の built-in table style。
  - ヘッダの状態表示。
- 1 件は、rev 3 で 4:3 variant を外したときの直し漏れである（§12）。

## 判定

**converged: yes / Accepted after Minor only: yes.**

- Blocker: なし。
- Major: なし。
- Minor: R3-m1〜R3-m4。

R3-m1〜R3-m4 を反映すれば、round 4 を待たずに Accepted にしてよい。#49 と #38 の merge は、実装の依存として
残る（§3 の「Until the core PR lands」の段落が、その間の挙動を定めている）。

---

## 解消状況（round 2 の指摘）

| 指摘 | rev 3/4 | 確認 | 判定 |
|---|---|---|---|
| N1 same-part copy は span を保つ clone にする | §3、§11、Q3。#49 の API 名 | `same_part.rs` は `RawNode` を span ごと clone する。照合は `check_destination` で行い、対象は free prefix、`mc:Ignorable`、継承された `xml:space`。深さは `TooDeep` で検査する。P2 では、`xmlns:ahyp` を自分に持つ `a:rPr` を 2 回コピーし、byte のまま 3 つになった | 解消 |
| N2 `a:ext` の URI 許可リスト | §3 | `extensions_allowed` は、`extLst` の子がすべて許可された `ext` であることを求める。P4: 許可した ext と未知の ext が混ざると `ExtensionList` になる | 解消 |
| N3 外す拒否と残す拒否 | §3、§11 | 外すもの: `r:`、`o:relid`、宣言、許可した ext。残すもの: `smtId≠0`、`CT_EffectContainer@name`（`dml.toml` の `unique_attrs`）、MC、`AlternateContent`。`check_copyable` と一致する | 解消 |
| N4 最初の分割の前の検査 | §3、§13 T4 | 両端の run の clone の可否、`RangeSplitsField`、値、set と clear の両方を指定した場合の拒否。比較は run 自身の `a:rPr` で行う。T4 に「2 つ目の境界の run」の場合が入った。P3: 2 つ目の run の拒否は detach の時点で出る（どの変更よりも前） | 解消 |
| N5 `notesSz` | §8-2 | プレースホルダを x と y で別々に拡大・縮小する。`sldImg` は `sldSz` の比率で収める。4:3 variant は外した | 解消。ただし §12 に直し漏れ（R3-m4） |
| N6 provenance | §6 | 外側の zip の hash、3 つの hash を省略せずに書く、版の日付、`THIRD-PARTY-NOTICES`（ADR-0002 §2 と main の `crates/ooxml-schema/THIRD-PARTY-NOTICES` に合う）、`presetShapeDefinitons` | 解消 |
| N7 `tableStyles` は関係で探す。リンク置換の順序 | §7、§9 | `Exact` と `NameInUse` は #38 の `add_part`（L854-884）と一致する。置換の順序（add → 編集 → stage → remove）は、`stage_part` の `ConflictsWithPlan`（L1906-1912）と `remove_relationship` の staged の走査（L1137）に合う | 解消 |
| Nit（ADR-0012 へのリンク、`ahyp` の宣言の位置、12/14） | ヘッダ、§7 | — | 解消 |
| P49-m2 copy は staged clone から取る | §3 | P1: parse 済みの木から取った copy は、`check_insert_under(clone, …)` が `WrongDocument` を返す。clone から取ればそのまま通る。`XmlDocument::clone` は新しい `DocToken` を作る（`xml/mod.rs:389`）。#49 の doc にも同じ 1 文がある | 解消 |

## #38 との整合（pr38-round2 の N1、N2 を含む）

- **N1**（編集していない clone の判定をすり抜けられ、元のエントリが保存される）: ADR-0016 は、lens で編集した
  clone（dirty）か、新しく作った木しか stage しない。原則 4 で何も変わらない書き込みは、plan を作らない。
  そのため、この経路を通らない。同じセッションの `xml_part_mut` で dirty になった木を clone した場合も、
  clone は dirty を引き継ぐので detach される。N1 が直れば、この判定は元のエントリとの比較になるが、
  ADR の側は何も変わらない。
- **N2**（`removal()` が `set_relationship_target` を見ない）: ADR-0016 は `set_relationship_target` を使わない。
  リンクの置換は、新しい関係を足して古い関係を外す（§7、代替案 4）。影響はない。
- #38 の `remove_relationship` は、round 1 の M5 で `allowed_rel_types` を引数に取るようになった。
  ADR はまだ、どの型を渡すかを書いていない（Nit 参照）。

---

## Minor

### R3-m1. §10 の provenance の記述が、Accepted の ADR-0008 と合わない

- §10 は「ADR-0008 reads show it with `Source::Theme`」と書いている。
  - ADR-0008 rev 3 の `Source`（§5、L861-866）に `Theme` という variant は無い。
  - テーマの値は、参照している要素の `Source`（たとえば `Direct`）に付く `Font.theme`（`ThemeFontRef`、
    テーマのパート付き）と、`Color.mapped`/`base_rgb` で返る。
- §10 は「ADR-0008 reports those with the override as source」とも書いている。
  - ADR-0008 §1.4（L411-416）では、`themeOverride` は適用されない。そのようなパートの上では、テーマに
    依存する lookup が `Lookup::Unresolved(ThemeOverride)` になる。
- §13 の provenance oracle がテーマの編集に何を期待するのかが、この 2 文から決まらない。
- **推奨:** 次のように書き直す。
  - テーマの編集の後、ADR-0008 の読み取りでは、参照元の `Source` は変わらない。新しい値は
    `Font.theme`/`Color` の解決値に現れる。
  - `themeOverride` を持つパートでは、引き続き `Unresolved(ThemeOverride)` になる（Phase 3 でも override は
    適用しない）。

### R3-m2. §9 と Q4 の built-in table style が、Accepted の ADR-0013 と合わない

- ADR-0013 rev 4 §3（L314-322）は、すでに次のように決めている。
  - `set_style(Some(id))` は、built-in の 74 個の GUID を受け付ける。
  - GUID は大文字小文字を区別せずに比べる。
- §9 は「ADR-0013's `set_style` accepts, with an explicit `TableStyle::BuiltIn(guid)`」と書いている。
  `TableStyle::BuiltIn` は ADR-0013 の API に無い。
- Q4 は、この件を未決の質問として残している。
- **推奨:**
  - §9 の該当の箇条は、「ADR-0013 §3 が built-in の id を受け付ける。Phase 3 は、生成する id がそれらと
    衝突しないことだけを保証する」に縮める。
  - Q4 は「ADR-0013 で決定済み」として閉じる。

### R3-m3. ヘッダの依存の状態が古く、#49 が依存に載っていない

- ヘッダに「ADR-0008 (PR #28, Proposed)」と「ADR-0013 (PR #33, Proposed)」とある。どちらも main で
  Accepted になっている（rev 3、rev 4）。PR #37 のブランチは main より遅れている。
- §3 と Consequences は、core PR #49 を必須としている。それなのに、ヘッダの Depends-on と Sources に #49 が
  無い。#49 が ADR-0002 に足す note「same-part copy」も載っていない。
- **推奨:**
  - origin/main を merge する（rebase はしない）。
  - ADR-0008 と ADR-0013 を、Accepted としてファイルへのリンクにする。
  - Depends-on と Sources に、PR #49（`core/detach-same-part`、ADR-0002 note「same-part copy」）を足す。
  - rev 4 の「head `f8b2998`」は `397bd86` に直す。API 名に変わりはない。`397bd86` で P49-m2 の doc が入った。

### R3-m4. §12 の notes master の行が、外した 4:3 variant を残している（N5 の直し漏れ）

- §12 の表の notes master の行に「template part (16:9 or 4:3)」とある。
- §8-2 と revision 3 の表は、「4:3 variant は外し、`notesSz` と `sldSz` から計算する」と決めている。
- **推奨:** 「template part scaled to `p:notesSz`, `sldImg` fitted to `p:sldSz`」に直す。

---

## Nit

- **§3「staged clone」の範囲**:
  - 原則 6 の degenerate case（1 つのパートの木だけを変え、registry を変えない書き込み）は、その場で編集する。
    `format_range` で、リンクの追加や削除を伴わないものは、通常この場合にあたる。
  - 規則としては、「copy と check は、**編集している文書**から取る（transaction では stage する clone、
    degenerate case ではパースした木）」と書くのが正確である。
- **§3 の照合の列挙**: 「(element, attribute, MC attribute values)」とある。#49 は MC を拒否するので、
  MC の属性値は起こらない。また #49 は、属性値の中の QName を照合しない（doc に書かれた既知の限界）。
  「MC attribute values」を外すか、「MC is refused」と添える。
- **§11 の対応**:
  - #49 の `NotDetachable` は、出所の拒否（`UniqueAttribute`、`ExtensionList`、`MarkupCompatibility`、
    `AlternateContent`）と、挿入先の拒否（`NamespaceBinding`、`IgnorableNamespace`、`InheritedXmlSpace`）が
    同じ variant に入っている。
  - 前者は `FormatNotCopyable` に、後者と `WrongDocument` は `Core(…)`（分割では起こらないので内部のバグ）に
    対応させる、と書く。
  - `Core(…)` の行の `NamespaceNotInScope` は #18 の名前なので、#49 の reason に置き換える。
- **§4、§7 の `remove_relationship`**: #38 は `allowed_rel_types` を求める。渡す型を書く。
  - リンク: `…/hyperlink` と `…/slide`。
  - `buBlip`: `…/image`。
- **件数の母集団**:
  - §8 の「460 of 616」と §8-2 の「449」、§9 の「50 of 606」は、母集団が違う（619 と、SHA-256 で重複除去した
    606）。
  - 後者を使う箇所に、「round 2 の重複除去済み 606 ファイル」と書く。
- **§3 の set と clear の拒否**: set と clear の両方を指定した場合の拒否を `ValueOutOfRange { attr }` で
  表すのは、名前と意味が合わない。`RunFormat` を項目ごとに `Set(v) | Clear` の enum にすれば、
  この状態は表現できなくなる（エラーも不要になる）。
