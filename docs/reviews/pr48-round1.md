# PR #48 レビュー（Opus, round 1）

対象: PR #48 `impl/pptx-phase1-replace`（HEAD `0c9fa99`。origin/main の Phase 0.1 #18/#19 をマージ済み）。
`Presentation::find` / `Presentation::replace_all`。ADR-0008 rev 3 §4（Accepted、main）の実装。
照合したもの: ADR-0008 §4・§7 O5、`spikes/pptx-phase1/findreplace.py`、ADR-0005 §3/§6/§9、
`ooxml-core` の `LensMut::insert_by`・`Package::xml_part_mut`。
作業場所: スクラッチの `git clone` コピー（`review-48-46r2/pr48`）。プローブ用テスト（`zz_probe.rs`、`zz_gen.rs`）と
手動 mutant はスクラッチ内だけにあり、リポジトリには入っていない。

## 結論（先に）

実装は §4 の規則 1–7 に忠実である。check-all-before-change、Skip/Abort、拒否時のバイト不変、T2 局所性も
実測で成り立った。手元で次を独立に再現した。

- **コーパス一巡**（614 ファイル: LibreOffice 420 + POI 94 + ShapeCrawler 100）:
  22,012 呼び出し、30,038 置換。拒否は `FieldInMatch` 572、`LinkSplitByBreak` 30、`MergedCell` 4。
  不変条件 T/F はすべて成立した。PR の数値（633 ファイルで 22,120 / 30,174）と整合する。
- **python-pptx 1.0.2 による読み戻し**: fixture、LibreOffice、ShapeCrawler から作った 942 個の出力を読んだ。
  置換文字列は `Ẍ\vQ`、空文字列、`置換` の 3 種。変化した 1,404 段落はすべて Python の `str.replace` と一致した。
  読み込みエラーは 0 件、段落集合の変化も 0 件だった。
- **形式の喪失を狙ったプローブ**: いずれも期待どおりだった。
  - 開始ランの `lang`/`altLang`/`dirty`/`err` は保持される。`a:pPr` と `a:endParaRPr` は変更されない。
  - サロゲートペア（𠮷）、先頭と末尾の空白、`&amp;`/`&lt;`、`a:t` 内の CRLF。
  - 同じラン内の 2 一致に `\v` を含む置換。
  - リンク付き開始ランから次のランへまたがる一致に `\v` を含む置換（新しいランにはリンクが付かない）。

ただし、ADR-0008 §7 **O5 の独立オラクルの半分**（`pptx-readback` ジョブで、§4 の規則を python-pptx の
lxml 木に独立に適用して比較する）が実装されていない（M1）。このほか Minor が 6 件ある。
未テストの規則分岐が残っている（mutant が 3 件生存）。また、無関係なランに `<a:t/>` を挿入する局所性違反が 1 件ある。

**判定: no**（Major 1 件。ただし M1 は試験基盤の追加だけで閉じ、実装の変更は不要。M1 と Minor を直せば
次の round で収束する見込み）。

---

## Major

### M1. ADR-0008 §7 O5 の独立オラクル（python-pptx 側）が実装されていない

**根拠。** ADR-0008 §7 O5 は、Rust 側の不変条件 T/F に加えて次を求めている。

> Independently in the `pptx-readback` job: `check.py` performs the same edits on python-pptx's lxml tree
> from the rules of §4 and compares the whole-deck model; python-pptx's paragraph text equals Python's
> `str.replace` on the input text.

PR の差分には `tests/oracle/check.py`、`examples/oracle_cases.rs`、`tests/oracle/cases.rs`、
`.github/workflows/ci.yml` への変更がない（`git diff origin/main...HEAD --stat` の 8 ファイルにも含まれない）。
`tests/replace.rs` の期待値（`expected()`）は、実装と同じ作者が同じ規則を読んで書いたモデルである。
これは ADR-0005 以来この repo が「独立でない」として退けてきた形にあたる。

今回は手元の読み戻し（上記の 942 出力）で不一致を見つけなかった。それでも CI で継続して守られていない。

**推奨。**

- `oracle_cases` に find/replace のケース群を追加する。fixture ごとに、ランをまたぐ句、`\v` を含む置換、
  空文字列への置換、リンク付きラン、Skip で一部が拒否されるケースを入れる。各ケースに (pattern, replacement) を付ける。
- `check.py` に次の 2 つを加える。
  - (a) python-pptx の段落テキスト = 入力テキストに対する `str.replace`（拒否された一致を除く）。
  - (b) 入力の lxml 木に §4 の規則 1–5 を Python で独立に適用し、ラン単位の (text, canonical rPr) 列を
    出力と比べる。スパイク `findreplace.py` の参照モデルを流用してよい。
- README の「T3 independent half」の記述をこれに合わせる。

---

## Minor

### m1. 一致に関係しない `a:t` のないランに `<a:t/>` が挿入される（T2 局所性違反）

`replace.rs` L416-424:

```rust
if item_text(n) != it.text || n.elements().all(|c| !c.is(Text::QNAME)) {
    plan.texts.push((i, it.text.clone()));
}
```

この条件は、計画対象の段落にある**残すランすべて**に適用される。そのため、一致に関係しないランでも
`a:t` がなければ `get_or_insert::<Text>()` で `<a:t/>` が足される。

プローブ（`zz_probe.rs` P1）の結果:

- 入力: `<a:r><a:rPr lang="en-US"/><a:t>abc</a:t></a:r><a:r><a:rPr lang="en-US" b="1"/></a:r>`
- 操作: `"b"` を `"X"` に置換する。
- 出力: 2 番目のランが `…<a:rPr lang="en-US" b="1"/><a:t/></a:r>` になる。

`a:t` のないランは空の区間しか持たないので、一致に覆われず、touched にもならない。したがって、この節が
効くのはこのケースだけである。節を削除する mutant（下表 M4）でどのテストも落ちないことがその裏付けになる。

**推奨。** `|| n.elements().all(…)` を削除する。あるいは条件を `it.touched` に限る。上の入力で、2 番目の
ランのバイトが変わらないことを確かめる回帰テストを足す。

### m2. 規則 6（`MixedFormat::Refuse`）の正規形が、見えない差で拒否する

`canon()`（L187-205）の結果は次のとおり。

- `a:rPr` がないランと、`<a:rPr lang="en-US"/>`（校正属性しかない）のランは別の書式とみなされ、
  `MixedFormatInMatch` になる（P2）。ADR の文言は「proofing attributes 以外で異なる」場合に拒否する、である。
  スパイクも同じ分類なので数値への影響はないが、文言と実装が合っていない。
- 整形された `a:rPr`（子要素の間に改行やインデントの空白テキストがある）と、整形されていない同じ `a:rPr` も
  別の書式とみなされ、`MixedFormatInMatch` になる（P2b）。PR は、テスト側の正規形については同じ欠陥を直した
  （「whitespace between child elements is not content」）。しかし実装側の `canon` は空白テキストを含んだままである。

どちらも拒否が増える方向（安全側）だが、利用者には理由が見えない。

**推奨。**

- 空白だけの `RawItem::Text` を無視する。
- 「`a:rPr` がない」を「属性も子要素もない `a:rPr`」と同じに扱い、校正属性を落としてから比べる。
- 選ばなかった方の振る舞いは rustdoc に書く。
- テストを 2 本足す。

### m3. 生存した mutant（規則分岐の試験漏れ）

cargo-mutants はこの環境にないので、手で 7 個の mutant を作り、`tests/replace.rs` を実行した。

| mutant | 内容 | 結果 |
|---|---|---|
| M4 | m1 の `a:t` 欠落の節を削除 | 生存（m1 の修正そのもの） |
| M9 | 規則 6 の比較を、隣り合うランの組（`windows(2)`）から最初と最後のランだけに変える | **生存**: 3 ラン（[通常][太字][通常]）にまたがる一致を `Refuse` で試すテストがない |
| M12 | `\v` のコピーの挿入先検査（`d.check(doc, para_path, 1)`）を `Ok(())` にする | 生存（実際には失敗しない防御コードだが、拒否経路が未テスト） |
| M13 | 規則 3: 最後のランの残りが空でも削除せずに残す | **生存**: 空の `<a:r><a:t/></a:r>` が残る。T/F モデルは空のランを見ないので検出できない |
| M14 | 規則 4: 「ランが残らないなら 1 つ残す」を無効にする | 検出 |
| M15 | 校正属性の除外を入れ子の要素にも適用する | 生存（ほぼ等価） |

M5 は、レビュー側が誤って等価な mutant を作ったので除外した。

**推奨。**

- 構造の不変条件を F に加える。「touched なランで空のものは、段落の唯一のランである場合を除き存在しない」。
  これで M13 を検出できる。
- 3 ランにまたがる `Refuse` のテストを足す（M9）。
- コーパス一巡に `MixedFormat::Refuse`、`ascii_case_insensitive`、`OnRefusal::Abort` の組を少数加える。
  今は 3 つとも既定値でしか回していない。
- `CommentAnchoredText` で拒否されることの置換テストを 1 本足す。今はガードを Phase 0 のテストでしか試していない。

### m4. apply の途中失敗が「Err ⇒ 何も変わらない」を破りうる

`replace_all` は全スライドを計画した後、スライドの順に `apply_plan(...)?` を実行する。

- スライド k で `Err` が返ると、スライド 1..k−1 は変更済みのまま `Err` になる。
- `apply_plan` の失敗は、計画で検査済みなので構成上は起こらない。しかし pptx には xlsx の poisoning に
  相当するものがなく、「起こらない」ことを守る試験もない。

**推奨。** どちらかを選ぶ。

- (a) rustdoc に「apply 中の Err はバグで、パッケージは部分的に変更されている」と明記する。
  あわせて `debug_assert!` を置き、fault injection で 1 本テストを書く。
- (b) 失敗したら変更済みのスライドを元の木に戻す。slide part の木を apply 前に clone しておく。

### m5. 公開型の将来互換と、ADR の API スケッチとの差

ADR §4 のスケッチとの差:

- `Match.items`（`Range<usize>`）がない。
- `Scope::SlidesAndNotes` / `Notes(i)` がない。`Scope` は `#[non_exhaustive]` なので後から足せる。
- `ReplaceReport.aborted` が追加されている。

一方、`Match`、`TextLocation`、`ReplaceReport`、`FindOptions`、`ReplaceOptions` は pub フィールドの構造体で、
`#[non_exhaustive]` がない。そのため、Phase 1 の残り（notes、`items`）でフィールドを足すと破壊的変更になる。

**推奨。**

- これらに `#[non_exhaustive]` を付ける。オプション型は `Default` と `..Default::default()` で組み立てられるので、
  付けても使い勝手は落ちない。
- ADR との差（notes は §2 の後、`items` は未実装、`aborted` は追加）を PR 本文と rustdoc に書く。

### m6. 古い記述

- `src/lib.rs` L20 の「(ADR-0008 §4, under review)」、`README.md` L29 の「under review」、
  `examples/manual_check.rs` L335 の「ADR still under review」。ADR-0008 は Accepted rev 3 である。
- PR 本文の後半は、「Rules (ADR-0008 rev 2 §4)」「Manual check 11–13」「PRs #18/#19 are not merged yet」
  「FormatNotCopyable 7,770」などの古い内容である。冒頭の Update 節が上書きすると断っているが、
  レビューアーとマージ後の読者を混乱させる。古い節は削るか、rev 3 の内容と番号 14–16 に書き直す。

---

## Nit

- `Error::duplicate` の `other => UnsupportedContent(other.to_string())` は型を落とす。今は到達しないが、
  `#[expect]` 付きの `unreachable!` にするか、`Clone` できる variant を網羅したほうが意図が明確になる。
- 完全に覆われて削除されたランが `a:hlinkClick r:id` を持っていた場合、スライドの `.rels` に未使用の関係が残る。
  Phase 0 の `set_text` と同じ挙動で無害だが、rustdoc に一言あるとよい。

## 確認済み（問題なし）

- 規則 1–5:
  - 開始ランの `a:rPr` を保持する。
  - 完全に覆われたランは削除し、部分的に覆われた最後のランは切り詰めて `a:rPr` を保つ。
  - 空になったら削除する。ランが 1 つも残らない場合は最初のものを残す。
  - `\v` は `a:br` と新しいランになり、新しいランには `detach` によるリンクなしのサブツリーコピーが付く。
    挿入先の検査（`check`）は計画の時点で行う。
  - 接尾辞は最後の新しいランに入る。
  - `LinkSplitByBreak` は `hlinkClick` でも `hlinkMouseOver` でも起き、開始ランの中で一致が終わる場合だけに限られる。
- `insert_by` の `k` はスロット内の位置なので、`a:pPr` があっても位置はずれない（コーパス一巡と読み戻しで確認）。
- 計画段階の `slide_mut` / `shape_mut` / `text_body_mut` は木を変更しない（`xml_part_mut` は dirty を立てない）。
  したがって拒否、`Abort`、一致なしのいずれでもパッケージはバイト不変である（テストとプローブで確認）。
- `ascii_case_insensitive` は ASCII だけを畳むので、バイト位置が保たれる。非 ASCII の針（サロゲートペア）でも
  正しく一致する。パターンの正規化（NFC/NFD）は行わない。§4 は「literal」としているので仕様どおりで、
  rustdoc に一言あるとよい。
- `mc:AlternateContent`、`p:charRg`、`a14:m`、結合セルは拒否される。段落単位のアニメーションのロックでは拒否されない。

## 判定

- converged: **no**（Major 1 件: M1）。
- Accepted after Minor only: **no**。
- M1（O5 の python-pptx 側オラクルを CI に追加）と m1–m6 を直した版で round 2 を行う。
  実装の変更が要るのは m1 の 1 行（と、選択次第で m2 と m4）だけである。
