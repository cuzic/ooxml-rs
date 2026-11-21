# PR #102 レビュー（Opus, round 1）

対象: impl/pptx-phase3d-notes-master @ 86c19fedc56b951001b7673398d7de52cab2ed4f

## 結論（先に）
converged: yes。ADR-0016 §8.1 の 4 分岐、§8.2 のスケーリング、§8.3 のノート作成、§8.4 の `Notes` 加算は、
プローブで確かめた範囲では正しく動いた。拒否はすべて入力とバイト単位で同一で、局所性も保たれていた。
Blocker と Major は無い。Minor 3 件と nit 2 件は follow-up でよい。

## 前ラウンドの解消状況
初回のため無し。

## 確認したこと（プローブはリポジトリ外。コミットしていない）
- **公開 API**: `origin/main` には、ノートを作成する公開 API が別名でも存在しない（README は `ensure_notes_slide` を
  "not yet" と記載。`notes_mut` は `NoNotesSlide` を返すだけ）。Issue #101 の「#76 でマージ済み」は誤りで、#76 で入ったのはテンプレートだけ。
  PR は ADR-0012 §4.6 の名前 `ensure_notes_slide` で追加し、ADR-0016 §8 が便利関数として認めている
  `notes_mut_or_create` を加えている。既存の名前は変わっていない。`notes_mut` のシグネチャは同じで、
  `notes_handle` は `pub(crate)`。生成コード `pml.rs` の変更は追加の 136 行だけ。`Error` は
  `#[non_exhaustive]` なので、`NotesWithoutMaster` の追加は破壊的変更にならない。
- **マスターを作る場合**（`shp-connector-props`）: スライド 0 と 1 を続けて作成した。マスターは 1 つ（`notesMaster1` と `theme2`）、
  ノートは `notesSlide1` と `notesSlide2`、`app.xml` の `Notes` は 0→2、関係 `rId9` は既存の最大値 + 1 だった。
  保存して開き直すと、どちらのノートの本文も読み戻せた。同じセッションの中でも、`notes` `notes_mut` `deck().slide(0).notes()`
  が作成直後のノートを返した。キャッシュが古くなることは無かった。同じスライドで `ensure` をもう一度呼んでも、
  出力はバイト単位で同一だった。
- **既存マスターの再利用**（`sld-notes`）: ノートがあるスライドには何も足さず、スライド 1 には既存のマスターにつながる `notesSlide2` を作った。
- **関係とリストのずれ**: リストのエントリが slide の関係 `rId2` を指していた場合、正しい `notesMaster` の関係 `rId4` に直した。
  リストだけがあって関係が無い場合（`rId77` は存在しない）は、マスターを作成し、リストを新しい `rId78` に更新した。
  どちらの出力も開き直せた。
- **名前空間**: ルートで `r` が別の URI に束縛され、関係の名前空間が `rel:` だった場合、新しいエントリは
  `rel:id` で書かれた。`p:` を使わない既定名前空間の `presentation.xml` でも、接頭辞の無い
  `<notesMasterIdLst>` が `sldMasterIdLst` の直後に入った。
- **`mc:AlternateContent`**: `p:notesMasterIdLst` を `mc:Choice` で包んだ場合は `Core(UnresolvedSibling)` で拒否された。
  拒否の後もバイト単位で同一だった（m3 を参照）。
- **局所性**: 統合テスト `creates_master_and_notes_in_one_edit_with_local_changes` は、
  `presentation.xml` の差分がリストの span だけであること、`app.xml` の差分が `Notes` だけであること、
  他のパートがバイト単位で同一であることを確かめている。プローブでも同じ結果だった。
- **テーマ**: 埋め込みのノートマスターとテーマには `r:` 参照が無いので、
  新しく作るマスター → テーマの関係 1 本で完結する。テーマ番号は最大値 + 1 で、テスト `theme10`→`theme11` で確認されている。
- **CI**: `gh pr checks 102` は 86c19fe で全件 pass（Rust 1.99 の test・clippy・codegen、python-pptx、python-docx、openpyxl、LibreOffice）。
- **他の open PR**: #99（docx）と #121（xlsx）も `codegen/report.txt` と `tools/lo-check/expected.json` に触れている。
  変更する行は別の名前空間・別のキーなので、衝突しても生成物の再生成と JSON の並べ直しで済む。公開名の干渉は無い。

## Findings

### m1（Minor）: スライドに宛先の無い `notesSlide` 関係があると、2 本目の `notesSlide` 関係を足す
- 根拠: `slide1.xml.rels` に `rId9 → ../notesSlides/notesSlide7.xml`（パートは存在しない）を足して `notes_mut_or_create(0)` を呼ぶと、`Ok` が返る。
  出力のスライドは `rId9`（宛先なし）と `rId10 → notesSlide8.xml` の 2 本の `notesSlide` 関係を持つ。
  `crate::notes::notes_part` は宛先が存在する関係だけを見るので、ぶら下がった関係は無いものとして扱われる。
  入力の時点で既に壊れている（コーパスでは 2 件）ため、Major にはしない。それでも、スライド 1 枚にノートの関係が 2 本ある状態を `Ok` で書いている。
- 修正案: `notesSlide` の関係があるのに宛先が無い場合は拒否する（新しい拒否 + Err ⇒ byte-identical テスト）。または、その関係の `Target` を新しいパートに向け直して 1 本に保つ。どちらにするかを ADR の note に書く。

### m2（Minor）: 本文プレースホルダーの無い既存マスターに対する分岐（Open question）にテストが無い
- 根拠: `notes.rs:207-209` の `if !has_body` を消す変異は、どのテストでも落ちない（`tests/notes_creation.rs` に、本文プレースホルダーの無いマスターを使う入力が無い）。
  プローブ（`sld-notes` のマスターで `type="body"` を `ftr` に変更）では、`<p:ph type="body"/>` と空の `p:spPr` を持つ本文が作られ、編集もできた。
  ただし継承元が無いので、本文には位置も大きさも無い。挙動は PR 本文の Open question のとおりだが、その経路が固定されていない。
- 修正案: この入力の統合テストを 1 件足す（本文があり、`body_mut` が成功すること）。位置が無い点も ADR の note に一言書く。

### m3（Minor）: `mc:AlternateContent` の中にある `p:notesMasterIdLst` は、汎用のコアエラーで拒否される
- 根拠: 上のプローブ。`Err(Core(UnresolvedSibling(p:presentation)))` が返り、出力はバイト単位で同一なので安全側に倒れている。
  ただ、利用者には原因がわからない。この形の入力に対するテストも無い。
- 修正案: `UnsupportedContent` などの説明的なエラーに変え、Err ⇒ byte-identical テストを 1 件足す。

### n1（nit）: `crates/ooxml-pptx/src/notes.rs` のモジュール文書が古い
- 根拠: `notes.rs:7-8` の "Creating a notes slide … is not part of Phase 1" が、作成 API へのリンクを持たないまま残っている。
- 修正案: `Presentation::ensure_notes_slide` へのリンクを 1 文足す。

### n2（nit）: ノートが既にあっても、`notes_mut_or_create` は既存マスターのルートを検査する
- 根拠: `notes.rs:82-83`。マスターのルートが `p:notesMaster` でない入力では、`notes_mut` は成功するが、
  `notes_mut_or_create` は拒否する。拒否なので安全側の振る舞いで、出力も変わらない。
- 修正案: 意図どおりなら、`notes_mut_or_create` の文書に一言書く。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 3 / nit 2）
