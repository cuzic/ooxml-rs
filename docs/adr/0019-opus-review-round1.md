# ADR-0019 レビュー（Opus, round 1）

対象: PR #135 `plan/pptx-phase4a-comments` @ `a0636b723bbe7f2e55fa6308366b4078e608705a`
（`docs/adr/0019-pptx-phase4a-modern-comments.md` revision 1 + `spikes/pptx-phase4a/`）。

照合したもの:

- ADR-0005 §6、ADR-0008 §3.3（`deckread.rs` の `classify_anchor`、`refs.rs`）、ADR-0012（§3 の id 規則と post-acceptance note、
  `slides.rs` の `duplicate_slide`・`delete_slide`・`drop_comment_rel`・`set_creation_id`・`next_shape_id`）、
  ADR-0016 P3e、ADR-0017 §5 と Q4。
- core の `PartNameRequest::Exact`（`crates/ooxml-core/src/package/add.rs`）。
- SDK データ `schemas_microsoft_com_office_powerpoint_2018_8_main.json`（`cmLst`・`authorLst`・`cm`・`reply`・`author`）。
- 2 つのモダンコメント fixture の中身（展開して読んだ）と `reference/fixtures/pptx/README.md` の来歴。
- `tools/lo-check/check.py`。
- スパイク: コーパスは再取得していない。`comments_survey.py` を `reference/fixtures/pptx`（24 件）で再実行し、
  モダンコメント 2 件・作者 3 件など fixture 由来の数値が結果ファイルと一致することを確かめた。走査ロジックも読んだ。
- `gh pr list`: 干渉する open PR は無い。#134 が `reference/fixtures/pptx/README.md` と `manual_check.rs` に触るが、
  P4a-0/P4a-1 のマージ順の問題にとどまる。

## 結論（先に）

設計の骨格（1 操作 1 plan、拒否はすべて `apply` 前、モダンのみ書く、テキスト範囲アンカーは拒否、作者は追記のみ、
P4a-0 で fixture を先に作る）は妥当。ADR-0012 との相互作用（複製・削除・`next_shape_id`）も、
次の 1 点を除いて正しく閉じている。**Major が 1 件ある**。
§6 が最後のスレッドを消すときに既存の `drop_comment_rel` を使う。この関数は slide 上の `p188:commentRel` を
**すべて**消すので、modern の関係を 2 本持つ slide（§9 は add だけを拒否し、delete は許す）で、
残る方のパートへの参照を黙って落とす。
ほかは Minor で、主なものは 3 つ。

- 既定の作者形 `Local` の根拠は、手で書き換えた fixture にある。
- オラクルの不変条件のいくつかは、§9 が許す既存の違反で落ちる。
- LibreOffice オラクルの道具が、まだ存在しない。

**converged: no**。

## Findings

### M1（Major）: 最後のスレッドの削除で、別パートを指す `commentRel` まで消える

- 根拠:
  - §6 の手順 3 は「the slide's `p:ext{6950BFC3-…}` is removed, and `p:extLst` too when empty (the existing `drop_comment_rel`)」。
  - `slides.rs` の `drop_comment_rel`（L1053–1076）は、`p188:commentRel` を子に持つ `p:ext` を **r:id を見ずに全部**消す。
    これは複製用の関数で、コピー側ではすべてのコメント関係を捨てるので、それで正しかった。
  - §9 の `CommentPartNotEditable` の「the slide has two or more modern relationships」は **(add only)**。
    reply・edit・delete は、modern の関係が 2 本ある slide でも許される。
  - 再現の筋道: 関係が `rId2`→A と `rId3`→B の 2 本で、`commentRel` も 2 つある slide で、A の最後のスレッドを消す。
    すると、A・`rId2`・`commentRel(rId2)` に加えて `commentRel(rId3)` も消える。結果は `Ok` で、
    B は関係だけを残し、slide からの参照を失う。
    PowerPoint が `commentRel` から B を辿るなら B のコメントは表示されなくなるが、*(u)*。
    §1 の「Never repair」にも、T2 の局所性にも反する。
  - コーパスには該当する slide が 0 件なので、実害は合成入力でしか出ない。それでも規則として誤りである。
- 修正案: 削除する関係の `Id` と一致する `r:id` を持つ `commentRel` だけを消す（`p:ext` は空になったときだけ消す）。
  もう一つの道は、modern の関係が 2 本以上ある slide での「最後のスレッドの削除」を `CommentPartNotEditable` で拒否すること。
  どちらにしても、§11 の 2 に合成入力の `Err` テストか局所性テストを足し、§11 の 7 に変異
  「`commentRel` を r:id で絞らない」を足す。

### m1（Minor）: 既定の作者形 `Local`（`userId` = name、`providerId="None"`）は、PowerPoint 自身が書いた形として示されていない

- 根拠: fixture README（`project/powerpoint-shape-comment.pptx`）に
  「Only the identity was changed afterwards … userId became `Ada Lovelace` and providerId became `None`」とある。
  ADR の Context 表（authors 行）と §4 の「PowerPoint's offline form」は、この書き換え後の値を PowerPoint の形として数えている。
  - 独立した根拠は、legacy の `p15:presenceInfo` の `None` 4 件だけである。
  - 書き換えたファイルが開けて保存後も残る、という**受理**の根拠はある。ただし、PowerPoint が**書く**形かどうかは示されていない。
  - 作者 id は書き換え前のアカウントから派生したもののままで、identity と id の組が実物と食い違っている。
  - P4a-0 も「identity anonymised」で同じ手順を繰り返すので、この点は P4a-0 を経ても確かめられない。
- 修正案: Context 表と §4 を「受理を確認した形（書き換えた fixture）。PowerPoint が書く形かは *(u)*」に直す。
  P4a-0 に F8 を足す: サインアウトした状態（ローカルアカウント、捨て名）でコメントし、作者属性は書き換えずに収める。

### m2（Minor）: §3.1 の「名前が使用中なら拒否」の説明が core と合わない。同じ session での add→最後を delete→add のテストが要る

- 根拠: §3.1 は、core が拒否する場合に「names removed in this session」を含めている。
  一方、`PartNameRequest::Exact` の doc（`add.rs` L100–102）は「A name removed earlier in the plan or session may be reused」。
  §6 で part を消しても slide の `sldId` と `cId` は残る（`p14:creationId` も残す）ので、同じ slide に次の add をすると
  **まさに同じ名前**を要求する。
  core が再利用を許すので動作は正しいが、ADR の「`sldId` は session で再利用されないので衝突しない」という理屈では、
  このケースを説明できていない。
- 修正案: §3.1 の文を直し、P4a-2 に「同じ session で add → 最後を delete → add」のテストを足す。
  確かめることは 3 つ: 同名の part が再作成されること、slide の `rId` が除去済みの番号より上で続くこと、
  `Override` がちょうど 1 本であること。

### m3（Minor）: §3.2 の `p14:creationId` の追加が、既存の `set_creation_id` では書けない形なのに仕様が足りない

- 根拠: `set_creation_id`（L1094–）は、既存の `p14:creationId` の値を**書き換えるだけ**である。
  `p:cSld/p:extLst` が無い slide（`add_slide` の slide はすべてそう）には `UnsupportedContent` を返す。
  §3.2 は置き場所を述べるだけで、次の点を決めていない。
  - `p:cSld/p:extLst` を新しく作るときの位置。`bg?, spTree, custDataLst?, controls?` の後になる。
  - `p:ext{BB962C8B-…}` の追加と、`p14` の宣言の置き方。fixture では `p14:creationId` の上で局所的に宣言している。
  - `has_creation_id` は木全体を歩く。`p14:creationId` が正規の位置の外にあるときに、
    ADR-0012 と同じく拒否するのか、その値を `cId` に使うのか。
  - 既存の値が u32 として読めないときの扱い。
  §11 の 1 の局所性表の「maybe one `p14:creationId` ext」にも、`p:cSld/p:extLst` を新しく作る場合が含まれていない。
- 修正案: 次の 4 点を §3.2 に明記する。
  - 新関数 `insert_creation_id`: lens の順序規則で挿入し、`p14` は局所的に宣言する。
  - 正規の位置の外にある `creationId` は `UnsupportedContent` で拒否する（ADR-0012 と同じ）。
  - 読めない値も拒否する。
  - 局所性表の行に「`p:cSld/p:extLst` を新しく作る場合」を足す。

### m4（Minor）: XML 不変条件 I3・I4・I6・I12 が、§9 で許した既存の違反に対して偽陽性を出す

- 根拠: 4 つの条件それぞれに、§9 が拒否しない既存の違反がある。
  - **I6**（`ac:*Mk@creationId` = その shape の `a16:creationId`）:
    `libreoffice/modern-comments.pptx` は全ゼロの `creationId` を持つので、無変更のコメントで落ちる。
  - **I3**: §9 は「an unresolved `authorId` on an existing comment」を拒否しないと明記している。
  - **I12**（`p14:creationId` が slide 間で一意）: XML を丸ごと複製して slide を作る道具の出力では、重複がよくある。
    スイープの入力にあれば落ちる。
  - **I4**: 入力に重複した GUID があれば同じことが起きる。
- 修正案: 4 つとも「新規・編集した要素について」または「入力に無い違反を新しく作らない」に限定する。
  チェッカーには、入力と出力の両方を渡す。

### m5（Minor）: LibreOffice オラクル（§11 の 5）の道具が存在しない。`comments` キーは docx 専用

- 根拠: `tools/lo-check/check.py` L280–282 の `comments` キーは、python-docx の `Document(path).comments` で読んで return する。
  pptx に付けると python-docx で開こうとして失敗する。
  ODP に変換して `office:annotation` を比べる経路も、まだ無い。
  ADR はこれを既存のワークフローのように書いている。
  pinned の LibreOffice 25.2.7.2 が p188 を取り込むかどうかも、まだ確認されていない。
  LibreOffice のテストデータに fixture があるので取り込める見込みは高いが、確認は要る。
- 修正案: 次の 3 点を §11 の 5 と Delivery に書く。
  - 新しいキー（例: `odp_annotations`）を作る。
  - その実装を P4a-1 か P4a-2 のスコープに入れる。
  - 最初に `project/powerpoint-shape-comment.pptx`（無編集）で基準の実行をして、取り込まれることを確かめる。

### m6（Minor）: 入力検証と、既存スレッドに対する操作の範囲が抜けている

- 根拠: 次の 3 群が決まっていない。
  - 入力検証（§4・§5）:
    - `NewComment::lang` と `CommentAuthor::initials` の検証が無い。XML で使えない文字や空文字が通る。
      §4 の検証は name・userId・providerId だけである。
    - `created` が 1970 年より前、または 9999 年より後のときのエラーが決まっていない。
    - `\r` の扱いが決まっていない。生の文字として書くと、パーサの改行正規化で `\n` になる。
  - 既存のスレッドへの reply・edit・delete で、アンカーが §1 で書けない種類（`ac:txMkLst`、layout、`p188:unknownAnchor`）のとき、
    許すのか拒否するのかが書かれていない。アンカーには触らないので、許すのが自然である。
  - `status="closed"` のスレッドへの reply・edit・delete が、`CommentClosed` になるのかが書かれていない。
- 修正案: §5 と §9 に 1 行ずつ足す。
  lang・initials は `InvalidXmlChar` か `InvalidCommentAuthor`、範囲外の時刻は新しい variant か `InvalidCommentAuthor` 相当にする。
  `\r` は拒否するか、`\r\n` を `\n` として扱う。

### m7（Minor）: §4 の「`authorLst` が `p188:extLst` で終わるなら、その前に挿入する」は schema と食い違う

- 根拠: SDK データでは、`p188:CT_AuthorList` は `author*` だけで、`extLst` を持たない。
  `cmLst` も `cm*` だけで、§3.1 はこちらの未知の子を拒否している。§4 だけが扱いを変えている。
  また、§3.1 の「接頭辞ではなく URI で判断する」規則（ADR-0017 M3 の教訓）が、authors パートへの追記には書かれていない。
- 修正案: `authorLst` の未知の子は `AuthorsPartNotEditable` にする。
  名前空間の宣言の規則を §4 にも書く（§9 の「same structural reasons」に任せない）。

### m8（Minor）: 削除済み id を session で予約するため、`delete_slide` の挙動が変わる。読めないパートの扱いが無い

- 根拠: §8 は、session の id 集合を「最初のコメント書き込み、または modern パートを消す `delete_slide`」で作る、としている。
  これで ADR-0012 の `delete_slide` が、モダンコメントパートを読むようになる。
  そのパートが壊れているとき、`delete_slide` が新しく `Err` を返すのか、読み飛ばすのかが書かれていない。
  新しく `Err` を返すなら、既存の API の後退になる。
  §5 の全パートの走査も、別の slide の壊れたコメントパートで拒否するのか、読み飛ばすのかが書かれていない。
- 修正案: `delete_slide` では、読めないパートを読み飛ばす（v4 の衝突確率は約 2^-122 で、予約はテスト用の決定的なソース向けの保険）。
  add の走査では、壊れたパートで `CommentPartNotEditable` を返して fail closed にする。
  両方を §8 と §9 に明記する。

### m9（Minor）: コーパススイープの「期待する拒否」が手順と矛盾していて、想定外の拒否を検出できない

- 根拠: §11 の 6 は「slide 1（または §7 が許す最初の slide）」を対象にするので、`LegacyCommentsOnSlide` は 0 件になるはずである。
  それなのに、期待値には「5 つの legacy deck のコメント付き slide でだけ出る」と書いてある。
  「最初の top-level `p:sp` が AC の中」は、§2 の定義（`p:spTree` の直接の子）では起こらない。
  本当に起こりうる拒否が並んでいない:
  - top-level の `p:sp` が無い slide
  - 正規の位置の外にある `creationId`
  - `CreationIdsExhausted`
- 修正案: 期待する拒否を variant 別の allowlist にし、それ以外の拒否が 1 件でもあればスイープを失敗させる。

### m10（Minor）: P4a-0 に足りない fixture と確認がある（Q1・Q5 の残るリスク）

- 根拠: §3.1 は、新しいスレッドを既存パートの末尾に追記する。
  PowerPoint が 1 枚の slide に 2 つ目のスレッドを足すときの順序（末尾か、位置順か）を示す fixture が無い。
  また、PowerPoint の作者 id はアカウントから派生しているように見える（v4 ではない）。
  `Account` の作者を v4 の id で書いた場合、実際の本人が PowerPoint で開いたときの挙動が分からない。
  自分のコメントとして編集・解決できるか、同じ `userId` の作者を重ねて追加するか、のどちらもまだ確かめていない。
- 修正案: F9（1 枚の slide に 2 スレッド）を P4a-0 に足す。手動確認に M10 を足す: M4 の `Account` 作者のアカウントで開き、
  自分のコメントとして扱われるか、`authors.xml` に作者が重複して加わるかを記録する。

### n1（nit）: Context 表の「comments 4/4 が PowerPoint の形」の内訳

- 根拠: 4 つの id のうち 3 つは `libreoffice/modern-comments.pptx` のものである。
  このファイルは producer が PowerPoint 16 で、作者属性が欠けている。
  v4 という結論は変わらないが、PowerPoint の project fixture から来たのは 1 件だと書いておく。

### n2（nit）: スパイクが AC の両方の分岐の shape を数えている

- 根拠: `comments_survey.py` L307–309 は、`mc:AlternateContent` の Choice と Fallback をどちらも歩く。
  そのため、`shapes.*` の総数（7,629、うち 84 % が `a16` 無し）がやや多く数えられている。
  結論は変わらない。数え方の注記を README に 1 行足す。

### n3（nit）: 頭文字の規則が、日本語の名前で PowerPoint と合うか分からない

- 根拠: §4 の規則で「山田 太郎」は `山太` になる。PowerPoint の規則は *(u)*。
  P4a-0 の F2 で、2 人目の作者に非 ASCII の名前を使うと、まとめて確かめられる。

## Owner questions について

- **Q1（id の生成）**: 推奨（ランダム v4 とテスト用の決定的なソース、全パートと session に対する一意性の確認）に賛成。
  - コメント id は、PowerPoint が書いたもの 1 件とそれ以外 3 件のどれも v4 である。
  - ADR-0017 の Q4 と同じ判断になる。
  - 内容から導く v8 は、同じテンプレートに 2 回コメントしたときに衝突する。
  残るリスクは作者 id だけで、PowerPoint が作者 id を派生させている場合に起きる。これは m10 の M10 で確かめる。
- **Q5（作者の照合キー）**: 推奨自体は受け入れてよいが、問いの立て方を直したい。
  - `Local` では `userId` = name なので、2 つの案はまったく同じ動きになる。差が出るのは、`Account` の表示名が変わった場合だけである。
  - PowerPoint は、presence に `userId`/`providerId` を使っている。そのため、本人の判定もこの 2 つで行っている可能性がある。
  - そうだとすると、同じアカウントの作者が 2 件並ぶ方が、表示名が古いままの作者より害が大きいかもしれない。
  - M10 の結果を見てから決める、と書き直すことを勧める。
- Q2・Q3・Q4・Q6・Q7・Q8 は、推奨と後で見直す条件のどちらも妥当。
  Q4（`creationId` の追加）は m3 の仕様を補えばよい。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 10 / nit 3）
