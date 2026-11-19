# PR #85 レビュー（Opus, round 2）

対象: impl/docx-phase2-part3 @ a11ac3dd137c3cbcbd951dfb4c5f31dc4ec01d2d

## 結論（先に）
converged: no。round 1 の M1 と m1〜m6 はプローブで直っていることを確かめた。
main の取り込みにも問題はない。ただし a11ac3d が #82 の `tools/lo-check/expected.json` を丸ごと（372 行）
このブランチに追加している。#82 のツール本体が無い状態で同じファイルを足すため、#82 と add/add 衝突になる（Major）。
コード側の新しい問題は、安全側の過剰拒否と入力が不正なときの端のケースだけ（Minor）。

## 前ラウンドの解消状況
- M1: 解消。再開始するレベルの override をコピーし、`startOverride` だけを置き換える。
  `w:lvl` だけの override には `startOverride` が `w:lvl` の前に入る（スキーマ順）。`ilvl="00"` も同じレベルとして扱う。
- m1: 解消。`noBreakHyphen`/`softHyphen`/`cr`/`lastRenderedPageBreak`/`tab`/`br` はそのまま移動し、
  `w:t` だけを 2 つに分ける。移動先の順序（右半分の `w:t` が移動した子要素の前）と `xml:space` の付与
  （`ab cd` を `ab ` と `cd` に分けると、`ab ` の `w:t` に preserve が付く）は正しい。
  `NotMovable` での拒否は ZIP 全体がバイト一致した。拒否が起きるのは `w:r` 自身に xml:space がある場合と、
  子が run で宣言された別名の接頭辞を使う場合だけだった。`w:r` が `xmlns:w` や無関係な `xmlns:x` を宣言していても通る。
  ただし過剰な拒否が 1 件ある（m1 参照）。
- m3: 解消。範囲の外にあるフィールド（複合フィールド、`fldSimple`）は許す。`pPrChange` も許す。
  端点がハイパーリンクの中にあると `UnsupportedContext`、ハイパーリンクが範囲の途中にあるだけなら `Ok`。
  範囲にフィールドの境界が入ると `InsideField`。拒否の理由は doc に書かれている。
- m4: 解消。名前の検査、参照の検査、削除の対象は story だけになった。id の最大値の走査には glossary が残る（テスト有り）。
- m6: 解消。`clickAndTypeStyle`/`defaultTableStyle` を使用中として数える。ADR §2 の表も更新されている。
- m2: 解消。`fixture_sweep` を追加し、`docx-readback` ジョブも CI で通っている。sweep の bookmark は段落範囲だけなので、
  run の分割はスイープの対象外だった（n1）。
- m5: 解消。`w:CT_RPr/w:rPr` の detach の注記と、分割で run の番号が 1 つずれることを書いた doc が追加された。
- main の取り込み（42d2d06）: `--remerge-diff` は空で、手書きの解消は無い。生成された `wml.rs`/`dml.rs` の公開名を
  origin/main（9795b76）と比べた。削除も改名も無く、追加だけだった（wml +525、dml +5）。

## Findings

### M1（Major）: #82 の `tools/lo-check/expected.json` を丸ごとコピーしているため、#82 と add/add 衝突になる
- 根拠: a11ac3d は `tools/lo-check/expected.json` を新しいファイルとして追加する（372 行。#82 の 3623a03 版にエントリ 1 件を足したもの）。
  ツール本体（`check.py`、`src/main.rs`、`docs/testing/libreoffice-check.md`）は追加しない。
  `git merge-tree origin/ci/libreoffice-check HEAD` の結果は `CONFLICT (add/add): tools/lo-check/expected.json`。
  後からマージされる側が衝突を手で解く必要がある。さらに、このファイルを読むものが無いまま main に入る。
  #82 の manifest が変わると、このコピーは古くなる。追記の指示は「#82 が main に入ったら、そのエントリを追加する」だった。
  エントリの中身は正しい: 生成した `29-bookmark-list-style.docx` で 10 個の XPath がすべて期待どおりの件数になった。
- 修正案: このブランチから `tools/lo-check/expected.json` を削除する（`git rm`。revert でもよい）。
  エントリ（`expected.json:67-80`）は PR 本文に残し、#82 と #85 のうち後にマージされる側でエントリ 1 件として追加する。

### m1（Minor）: `w:r` に `xml:space` がある run の分割を、移動する子に空白が無くても拒否する
- 根拠: `crates/ooxml-docx/src/bookmarks.rs:382-405`。新しい run は `xml:space` を持たない。そのため、移動する `w:t` が
  run の preserve を継承しているだけで `NotMovable { reason: XmlSpace }` になる。
  プローブ: `<w:r xml:space="preserve"><w:t>ab</w:t><w:tab/><w:t>cd</w:t></w:r>` の "cd" に `add_bookmark` を実行すると拒否される。
  拒否時の出力はバイト一致し、安全側である。一方、同じ run で `w:t` の途中を分ける場合は `Ok` になる。
- 修正案: 元の run が `xml:space` を持つときは、新しい run にも同じ属性をコピーしてから子を移動する。
  直さない場合は、この拒否を `add_bookmark` の doc に書く。

### m2（Minor）: 同じ `ilvl` の `lvlOverride` が 2 つあると、`restart_list` は最初の 1 つだけを書き換える
- 根拠: `crates/ooxml-docx/src/numbering.rs:250`。入力 `<w:lvlOverride w:ilvl='0'><w:startOverride w:val='4'/></w:lvlOverride><w:lvlOverride w:ilvl='0'><w:startOverride w:val='9'/></w:lvlOverride>`
  に `restart_list(p, 3)` を実行すると、新しい num は `3` と `9` の 2 つの override を持つ。
  後ろの override を使うアプリでは、`Ok` なのに開始値が 9 になる。round 1 の実装は同じレベルの override をすべて捨てていたので、
  この入力の扱いは今回の修正で変わった。入力は不正だが、スキーマは `ilvl` の一意性を求めていない。
- 修正案: 同じレベルの override が 2 つ以上あれば、書き込み前に `InvalidNumbering` で拒否する（バイト一致のテストを付ける）。

### n1（nit）: `fixture_sweep` の bookmark は段落範囲（`whole(i)`）だけで、run の分割をスイープで通さない
- `crates/ooxml-docx/tests/phase2_registries.rs` の `fixture_sweep`。各 fixture で最初の段落の中ほどの文字列を
  `BookmarkRange::Match` で 1 回試すと、m1 の移動処理（`NotMovable` の拒否を含む）を実データで確かめられる。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 2 / nit 1）
