# PR #76 レビュー（Opus, round 1）

対象: impl/pptx-phase1-notes @ 9d1a5c8e347fc6454e24bafc5adf3e4caa39e0d2

## 結論（先に）

ノートの読み書き（`notes` / `notes_mut` / `body_mut`）、拒否時のバイト同一性、局所性（T2 にノート本文を追加）、python-pptx オラクル（T3 の編集・置換、ノート本文の読み取り）、#56 の r2-m2 / r2-m3 の対応は、どれも挙動で確かめて問題なかった。
一方、`Scope::SlidesAndNotes` で複数のスライドが同じノートパートを参照すると、同じ計画を同じ木に二度適用する。その結果、`Ok` のまま本文が壊れる（M1）。owner の決定（ノートパートごとに一度だけ処理する）がまだ実装されていない。
また、`Deck` 側のノート読み取りが、ルート要素が `p:notes` であるかを確かめていない（M2）。
**converged: no**。

## 前ラウンドの解消状況

PR #76 の round 1 なので、前ラウンドの指摘はない。#56 round 2 から持ち越した項目は次のとおり。

- **r2-m2: 解消.** 次の試験が足された。
  - `click_index_exact_sequence`（Z1・Z2。`None` を含めた列をそのまま比べる）
  - `layout_placeholder_inside_alternate_content`（Z6）
  - `slide_rect_nested_flipped_and_invalid`（Z9・Z10）
  - `master_values_report_the_slide_match_kind`（Z19）
  - `theme_reference_shape_and_coordinate_raw`（Z21・Z24）
  - `transition_kind_attribute_namespaces`（Z22）
  - `zh-HK` → `Hant`（Z26）
  - 古い `click_index_counts_click_groups` は、いまも `filter_map` で比べている。ただし新しい厳密な試験が同じ規則を守っているので、問題ない。
- **r2-m3: 解消.** `resolve.rs:803–838` は次の場合に `Ok(None)` を返す。
  - 不正な `rot`、`flip*`
  - `off`/`ext` が無い、または不正
  - `chOff`/`chExt` が不正
  - `chExt` の 0
  
  doc にもこの扱いが追記された。9 通りの不正入力を試験で確かめている。`rot="21600000"` が変換なしとして扱われることも確かめている。

## Findings

### M1（Major）: `SlidesAndNotes` で共有ノートパートを二度処理し、`Ok` で本文を壊す（owner 決定: パートごとに一度だけ処理）

- **根拠.**
  - `presentation.rs:265–270` の `scope_units` は、スライドごとにノートの単位を足すだけで、パート名で重複を除いていない。
  - 計画はすべて元の木の上で作られる。適用（`presentation.rs:409–` の `'apply`）は同じパートに 2 回行われる。
  - 2 回目の適用では、1 回目で変わった木に、元の木の子要素の位置を前提にした計画を当てる。
- **プローブ.** `poi/sample.pptx` の slide2 の notesSlide 関係を `notesSlide1.xml` に向けて、replace を試した。結果はどれも `Ok`。
  - 段落 `fo|o|!|tail` で `"foo"→"X"` → 本文が `"Xtail"` になった。`!` の run を黙って削除している。
  - `"a-b"` で `"-"→"\v"` → `"a\vb\vb"` になった。本文が重複している。
  - 同じ run の中の置換（`"foo bar"`）は見た目では壊れない。それでも `replaced` が 2 件になる。
- **find も重複する.** find も同じ一致を 2 件返す。既存の試験 `presentation.rs:555`（`assert_eq!(matches.len(), 2)`）は、この重複を期待値として固定している。
- **修正案.**
  1. `scope_units` で、ノートの単位をパート名で重複除去する。`BTreeSet<String>` で最初の出現だけを残し、`TextLocation.slide` は最初に参照したスライドにする。
  2. find と replace の両方に、この扱いを適用する。
  3. `presentation.rs:555` の期待値を 1 件に直す。
  4. 成功時の試験を追加する。条件は、run をまたぐ一致と `\v` を含む置換。共有パートが一度だけ書き換わり、`replaced.len()` が 1 であることを確かめる。
  5. ADR-0008 の post-acceptance note と PR 本文の Open questions を「パートごとに一度（owner 決定）」に更新する。

### M2（Major）: `DeckSlide::notes()` / `notes_sheet()` がノートでないパートをノートとして返す（fail closed 違反）

- **根拠.**
  - `deck.rs:196–197` は、notesSlide 関係の先が XML として読めるか（`parses`）しか見ていない。ルート要素が `p:notes` かは確かめていない。
  - プローブとして、slide2 の notesSlide 関係を `../slides/slide1.xml` に向けた。結果は次のとおり。
    - `Presentation::notes(1)` と `find(SlidesAndNotes)` は `Err(WrongElement{expected: notes, found: sld})` を返す。正しい挙動。
    - `deck.slide(1).notes()` は `Some("ppt/slides/slide1.xml")` を返す。`notes_sheet()` も、そのスライドを `SheetKind::Notes` の文脈として解決する。
  - その結果、`NotesRef::text()` が、スライドの body プレースホルダーの文字列をノートとして返し得る。編集側の API と読み取り側の API で、結論が食い違っている。
- **修正案.**
  - `load` の `filter` に、ルートが `p:notes` であることの確認を足す。たとえば `pkg.xml_part(n).is_ok_and(|d| d.root().is(pml::NotesSlide::QNAME))` とする。
  - 違う場合はノート無し（`None`）として扱う。
  - 上のプローブの入力で試験を 1 つ追加する。

### m1（Minor）: 公開 enum への `#[non_exhaustive]` の追加が記録されていない

- **根拠.** `Role`、`TextStyleKind`、`MatchKind`、`MapSource`（`resolve.rs:190, 202, 216, 476`）に `#[non_exhaustive]` が付いた。同時に variant も増えた。
  - これは既存の利用者による網羅的な `match` を壊す API 変更である。
  - 方向は正しいが、ADR-0008 の note にも PR 本文にも書かれていない。
  - 生成型の公開名は変わっていない。この PR は `ooxml-schema` の生成物に触れていない。
- **修正案.** ADR-0008 の post-acceptance note に、次の 2 点を 1 行ずつ足す。
  - 4 つの enum が `non_exhaustive` になったこと
  - 追加された variant（`NotesMaster`、`Notes`、`OnMaster`、`NotesOverride`/`NotesMaster`、`ChainLink::NotesMaster`）

### m2（Minor）: `txBody` の無い body プレースホルダーを飛ばして、次の body を採る

- **根拠.** `notes.rs:72` は、`p:txBody` の無い最初の body プレースホルダーを `continue` で飛ばし、後ろの body を採る。
  - ADR §2 と python-pptx の `notes_placeholder` は「最初の body プレースホルダー」と定めている。
  - オラクル（`readback.py`）は、その最初の body プレースホルダーの `txBody` が無ければ `None` にする。
  - この PR の実装とは差が出る。ただし、コーパスで body が 2 つあるノートは 0 件である。
- **修正案.** 最初の body プレースホルダーで決め打ちする。`txBody` が無ければ `NoNotesBody` とする。または、この差を ADR の note に明記する。

### nit

- `deck.rs:305`: `Deck::link` で部品が見つからないとき、常に `ChainLink::Theme` を返す。layout、master、notes master が欠けた場合も Theme と報告されてしまう。到達しない経路という前提なら `expect` にするか、呼び出し側から欠けたリンクの種類を渡す。
- `resolve.rs:625`: スライドの経路に `TextStyleKind::Notes => "otherStyle"` があるが、到達しない。`unreachable!` にするか、コメントを付ける。
- `presentation.rs` の `fault::FAIL_AFTER_SLIDE` は、名前のとおりスライドの番号で発火する。ノートの単位でも発火するので、doc に「単位（スライドまたはそのノート）の後」と書く。

## 判定

converged: no（Blocker 0 / Major 2 / Minor 2 / nit 3）
