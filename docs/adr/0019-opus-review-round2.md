# ADR-0019 レビュー（Opus, round 2）

対象: PR #135 `plan/pptx-phase4a-comments` @ `c708c7726a0eeba05c19bee2dcd96c71edea002b`
（`docs/adr/0019-pptx-phase4a-modern-comments.md` revision 2 と `spikes/pptx-phase4a/README.md`）。

照合したもの:

- round 1（`0019-opus-review-round1.md`）。
- revision 2 の差分（`3349514..c708c77`）。
- `crates/ooxml-pptx/src/text.rs` の文字規則表（ADR-0005 §3）。
- `read.rs` L367–373（`a:br` → `\u{b}`）。
- `deckread.rs` の `modern_text`。
- `slides.rs` の `drop_comment_rel` と `set_creation_id`。
- `crates/ooxml-core/src/package/add.rs`（`Exact`）。

## 結論（先に）

round 1 の Major（M1）は設計として解消された。Minor の m1〜m10 と nit の n1〜n3 も、すべて本文に反映されている。

revision 2 で新しく見つかった点は、すべて Minor か nit である。主なものは 3 つ:

- 本文テキストの文字規則が ADR-0005 の `TextBodyMut::set_text` と食い違う。
  読んだ本文をそのまま書き戻すと、`a:br` を含む場合に `Err` になる。
  また、同じ本文を書き直したときに何も変えない（no-op）という規則が無い。
- 読めないパートでの拒否の範囲が、必要以上に広い。
- 「作成・編集した要素はすべての不変条件を満たす」という一文が、m4 で消した偽陽性を返信の経路で呼び戻す。

Blocker と Major が 0 なので **converged: yes**。Minor は P4a-1/P4a-2 の実装 PR か post-acceptance note で直してよい。

## 前ラウンドの解消状況

- **M1**: 解消。
  - §6 の手順 3 は、`drop_comment_rel_for(doc, rid)` で r:id が一致する `p:ext` だけを消す。
    `p:extLst` は、空になったときだけ消す。
  - 同じ `p:ext` にほかの子があれば `CommentPartNotEditable` で拒否する。
    この判定は staged clone の段階、つまり `apply` の前に行われる。
  - どの `commentRel` もその Id を名指ししていなければ、slide は編集しない。
  - テストも揃っている: 局所性表に「ほかの `commentRel` はバイト一致」の行がある。
    §11.2 に 2 関係の SYNTHETIC テストが、§11.7 に変異「r:id で絞らない」がある。
- **m1**: 解消。
  Context 表と §4 は「受理を確認した形」に直り、既定の作者形は F8（サインアウト、作者属性は書き換えない）で決める。
- **m2**: 解消。§3.1 が `Exact` の再利用と同じ名前の再取得を正しく書き、§11.2 にテストがある。
- **m3**: 解消。`insert_creation_id` の位置・`p14` の局所宣言・拒否条件 3 つが決まり、局所性表に行が足された。
- **m4**: 一部に問題が残る。I3・I4・I6・I12 は入力と出力を比べる形になった。
  ただし、新しく足された一文が別の偽陽性を生む（下の m12）。
- **m5**: 解消。
  `odp_annotations` は P4a-1 で実装し、P4a-0 で基準の実行をする。代替のオラクルも用意された。
  ただし代替のオラクルは実際には弱い（下の n4）。
- **m6**: 一部に問題が残る。
  - `lang`・`initials`・`created` の範囲の検証、`closed` のスレッドへの全操作の拒否、書けないアンカーを持つ既存スレッドの操作の許可は、どれも妥当。
    アンカーのバイト一致は、局所性で確認する。
  - `\r` の正規化は、結論は妥当だが根拠が不正確（下の m11）。
- **m7**: 解消。`authorLst` は `author*` だけを許し、名前空間は URI で判断する。
- **m8**: 一部に問題が残る。
  - `delete_slide` は読めないパートを読み飛ばす。新しい `Err` は増えない。
  - add の走査での拒否は、範囲が広すぎる（下の m13）。
- **m9**: 解消。拒否は variant ごとの allowlist になり、それ以外の拒否でスイープを失敗させる。
  allowlist の一項目の参照先が未定義（下の n5）。
- **m10**: 解消。F8、F9、M10 と、Q1・Q5 との対応が書かれた。
- **n1〜n3**: 解消。
- **Owner questions**:
  - Q1 には、作者 id の注意書き（M10 で確かめる。コメントと返信の id には影響しない）が付いた。賛成。
  - Q5 は種類ごとの問いに書き直され、トレードオフと代替案が並んだ。M10 の後で決める。賛成。

## Findings

### m11（Minor）: 本文の文字規則が ADR-0005 の `TextBodyMut::set_text` とずれていて、読んだ本文を書き戻すと `Err` になる

- 根拠:
  - `\r` の根拠は「生の `\r` はパーサの改行正規化で `\n` になり、往復できない」となっている。
    しかし、文字参照の `&#xD;` で書けば往復できる。この理屈だけでは正規化を選ぶ理由にならない。
    本当の根拠は既存の規則にある。`text.rs` の表は、`TextBodyMut::set_text` で `\r\n` と `\r` を `\n` にしている（ADR-0005 §3）。
  - 同じ表は、`\v` を `a:br` にしている。一方 ADR-0019 §5 は、`\v` を `InvalidXmlChar` で拒否する。
  - 読み出し側の `read.rs` L370 は、`a:br` を `\u{b}` として返す。
    §5 の「プレーンな本文」の条件は `a:br` を許している。
  - その結果、`a:br` を含むコメントを `Slide::comments()` で読み、その文字列のまま `set_comment_text` を呼ぶと `InvalidXmlChar` になる。
    読んだ値をそのまま書き戻すと失敗する。
- 修正案: コメント本文は `TextBodyMut::set_text` と同じ表に従う（`\v` → `a:br`、`\r\n`/`\r` → `\n`）。
  §5 の `\r` の根拠は、この先例に差し替える。

### m12（Minor）: 同じ本文での `set_comment_text` に no-op の規則が無く、バイト一致しない

- 根拠:
  - 編集は「対象の `a:p` の子を置き換える」。最初の run の `a:rPr` だけを残すので、2 段落目以降の `a:pPr` や `a:rPr`
    （たとえば `lang`）、`a:endParaRPr` は、本文が同じでも書き直される。
  - `set_comment_resolved` には「すでにその状態なら no-op（バイト不変）」の規則がある（§5 Status）。
    本文の編集には、それに当たる規則が無い。
  - `\r` を正規化するので、no-op の判定は「呼び出し側の文字列を正規化したもの」と「読み出した本文」を比べる形でないと、
    `"a\r\nb"` を渡したときに同じ本文でも書き直しが起きる。
- 修正案: §5 に 1 行足す。
  - 正規化した新しい本文が `Slide::comments()` の返す本文と等しければ、`Ok` を返して何も変えない。
  - §11.2 にテストを足す: 同じ本文と、`\r\n` 版の同じ本文で、出力がバイト一致すること。

### m13（Minor）: 「読めないパートが 1 つでもあれば add を拒否」は範囲が広い

- 根拠:
  - §5 は、id の一意性を確かめる走査で読めないパートに当たると、別の slide のパートでも書き込みを拒否する。
    この走査は reply でも走る（新しい id を作るため）。そのため、壊れたパートが 1 つあると、デッキ全体で add と reply ができなくなる。
  - 何を「読めない」とするかが決まっていない。
    §9 の `CommentPartNotEditable` の理由（AC、未知の子、子の順序の誤り）まで含めると、走査はただの読み取りなのに、
    編集の可否の条件で拒否されることになる。
  - 「comments content type」が modern だけを指すのかも書かれていない。
    legacy の `…presentationml.comments+xml` には GUID が無い。
  - §8 は「予約は約 2^-122 に対する保険」と言って `delete_slide` で読み飛ばしを許している。
    add で拒否する理由（id が不明）と、この説明が釣り合っていない。
- 修正案: 走査の対象は modern の comments と authors の CT に限る。拒否は**パースできない**ときだけにする。
  AC や未知の子があっても、`@id` は集める。
  走査での拒否は、理由を `reason` に入れて区別する（たとえば `"id scan: <part> unparsable"`）。
  §8 との非対称（add は fail closed、delete は保険）は、意図として 1 行書く。

### m14（Minor）: 「作成・編集した要素はすべての不変条件を満たす」が、返信で I3・I6 の偽陽性を呼び戻す

- 根拠:
  - §11.3 に足された一文は「Elements this session created or edited must satisfy every invariant outright」である。
  - 返信は既存の `p188:cm` を編集する（`replyLst` を足す）。
  - `modern-comments.pptx` のスレッドに返信すると、その `cm` のアンカーは全ゼロの `creationId` のままである。
    これで I6 が落ちる。
  - `authorId` が解決できない既存のスレッドに返信すると、I3 が落ちる。
    §9 は、こうした既存のスレッドへの操作を許している。
- 修正案: 「作成した要素はすべてを満たす。編集した要素は、書いた属性と子だけを満たす」に直す。

### n4（nit）: §11.5 の代替オラクルの「バイト一致で通過」は成り立たない見込み

- 根拠:
  - LibreOffice は pptx を書き出すとき、自分の modern パートと legacy パートを作り直す（`pptx-epptooxml.cxx` L2342–2355）。
    入力のコメントパートをそのまま通すことはない。
  - `modern-comments.pptx` は LibreOffice の取り込みテストのデータ（`sd/qa/unit/data/pptx/`）なので、25.2 は取り込める見込みが高い。
- 修正案: 代替のオラクルは、最初から「エラーなく開ける」だけにしておく。

### n5（nit）: §11.6 の allowlist の「survey lists as non-conforming」が、どの出力を指すか決まっていない

- 根拠: `comments_survey.py` は集計を出すだけで、デッキごとの不適合の一覧を出さない。
- 修正案: スイープの allowlist は、デッキ名と variant の組の一覧として固定ファイルに置く。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 4 / nit 2）
