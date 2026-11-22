# PR #144 レビュー（Opus, round 1）

対象: impl/pptx-phase4a1-add-comment @ b2098c5bb1cf23bd33d207124dade8654b886f2f

## 結論（先に）
converged: no。挙動のプローブでは壊れたファイル、ぶら下がり参照、`Err` で出力が変わる例は見つからなかった。
出力の形は PowerPoint の fixture と一致し、T2 の局所性も守られている。
ただし、Q5（オーナー決定）の作成者照合のうち、ADR §11.2 と §11.7 が名指しで求めるテストと変異のうち 2 つが欠けている。
欠けているのは Local の照合と、`providerId` を持たない作成者の扱い。その変異はテスト全体を通り抜ける（M1）。
F8 の後で変わるのはまさにこの経路なので、Major とした。ほかは Minor 4 件と nit 2 件。

## 前ラウンドの解消状況
なし（round 1）。

## 確認したこと（問題なし）
- **出力の形**
  - スライドへのアンカーは F1（`powerpoint-slide-comment.pptx`）とバイトの形まで一致する。
    `pc:sldMkLst xmlns:pc` の後に `docMk` と `sldMk cId sldId` が続き、`p188:pos` は無い。
  - 図形へのアンカーは F4 の 5 種類（`picMk`、`cxnSpMk`、`graphicFrameMk`、`grpSpMk`、入れ子の `spMk`）に対して、平らな `ac:deMkLst[docMk, sldMk, <kind>Mk]` を書く。
    `pc` は各モニカーで宣言し、`creationId` は図形の `a16:creationId` をそのまま写す。
  - 新しいパートの名前は `modernComment_{sldId:X}_{cId:X}`。ルートは `a`、`r`、`p188` を宣言し、XML 宣言は `standalone="yes"` で CRLF が続く。
  - `p14:creationId` は `p:cSld/p:extLst` の末尾に入り、`xmlns:p14` は要素の上で宣言する。既存の `p:cSld/p:extLst` に他の `p:ext` があれば、その後ろに追記する。
  - `commentRel` は `p:sld` の末尾の `p:extLst` に入る。`r` がスコープに無いスライドでは、`r` をその場で宣言する。
  - 時刻は `…T09:50:45.123` の形。9999-12-31T23:59:59.999 は `Ok`、10000 年は `InvalidCommentTime` で、出力はバイト一致。
  - 作成者の属性の順序は `id name initials userId providerId` で fixture と同じ。頭文字は `AL`、`山太`、`C`。
- **局所性（T2）**: `poi/sample.pptx` で確かめた。
  - 1 件目で変わるのは、CT、`presentation.xml.rels`、新しい `authors.xml`、新しいコメントパート、スライドの `.rels`、スライドの 6 つ。
  - 2 件目（作成者が同じ）で変わるのは、コメントパートだけ。
  - 3 件目（新しい Account の作成者）で変わるのは、コメントパートと `authors.xml` だけ。
  - どちらのパートも「元の内容と、閉じタグの前に追記した要素」になっていた（`starts_with` / `ends_with` で確認）。
- **作成者の登録表**: どれも期待どおりに動いた。
  - Account は (`userId`, `providerId`) で照合し、表示名が違っても再利用する。`authors.xml` はバイト一致。
  - Local は name で照合し、`userId="Anonymous_X"` の既存の作成者も再利用する。
  - 名前が同じでも、Account の作成者を Local として再利用することはない。
  - `providerId` の無い作成者は照合されず、新しい項目が追加される。
  - `Account{provider_id:"None"}` と空の `userId` は、どちらも `InvalidCommentAuthor` で、出力はバイト一致。
  - 既定名前空間（`xmlns=` が p188）の `authorLst` には、`xmlns:p188` を要素の上で宣言して追記する。
- **一意性**
  - id の走査はコンテントタイプで行い、関係のつながっていないパートも対象にする（既存のテスト）。
  - `delete_slide` が予約する id も、その後の割り当てで避けられた。一時的な単体テストで確かめた。削除したスライドのコメントの GUID を乱数源に 2 回返すと、割り当ては 3 回目と 4 回目の値になった。
  - 別のスライドのコメントパートが壊れていても、`delete_slide` は `Ok` を返す。
  - ルートにある古いパートが正確に同じ名前を持つと、`Core(RegistryRefused{NameInUse})` で拒否し、出力はバイト一致。
- **拒否**: 次の拒否はすべて `Err` で、出力はバイト一致だった。
  - `LegacyCommentsInDeck`: `mixed-deck-input.pptx`、`powerpoint-legacy-deck-comment.pptx`、45545 の 3 枚目
  - `CommentPartNotEditable`: 共有されたパート、モダンの関係が 2 本、関係の無い `p:ext{6950BFC3}`、パートのルートで `ac` が別の URI に束縛されているとき
  - `AuthorsPartNotEditable`: `authors.xml` が解析できないとき、author の中に AC があるとき、authors の関係が 2 本あるとき
  - `UnsupportedContent`: `p14:creationId` が置き場所の外にあるとき、`-1`、`4294967296`、名前空間の違う `creationId`
  - そのほか: `CreationIdsExhausted`、`InsideAlternateContent`、`AmbiguousShapeId`、`p:contentPart`（`UnsupportedCommentAnchor`）
  - text-range のスレッドを持つパート（F5）への追記は `Ok`（ADR §5、触らないアンカーは許す）。
- **checker（`comments.py`）**
  - I3、I4、I6、I12、I13 は、新しい id、新しいスレッドと作成者、`creationId` の変わったスライドだけを入力と比べる。既存の違反は失敗にしない。
  - CI の python-pptx ジョブで "5 modern comment cases passed" と出ている。
- **コーパス**
  - allowlist は (sha256, step, variant) の組。未知の variant は `Unexpected` になって失敗し、古くなった項目も失敗する。
  - 固定コーパスは手元に無く、全体の再実行はしていない。
  - 同梱の fixture だけで再実行した（`COMMENT_CORPUS` を空のディレクトリにした）。結果は 33 パッケージ、追加 48 件、拒否 14 件、未許可 0 件。古くなった項目が 233 件出たが、外部コーパスを省いたためで想定どおり。
- **衝突と公開 API**
  - `git merge-tree` で origin/main と衝突しない。head は main を含んでいる。
  - `ooxml-core` の差分は無い。
  - 生成物の差分は追加だけで、削除行は 0。`pml.rs` は +505 で allowlist の 8 項目にあたる。`dml.rs` は +61 で、`GraphicData` の `ChildOf` とアクセサにあたる。
  - `tools/lo-check/expected.json` は #148 の後ろに 5 項目を追加しているだけ。
  - CI は 5 ジョブすべて緑。

## Findings
### M1（Major）: Q5 の Local 照合と `providerId` の無い作成者について、ADR が求めるテストと変異が無く、変異が通り抜ける
- 根拠:
  - ADR §11.2 は次の 2 つの author-key テストを名指しで求めている。
    - "a `Local` author with an equal name but another `userId` is reused"
    - "an existing author without `providerId` is never reused"
  - §11.7 は変異 "the Local key including `userId`" も求めている。
  - しかし Local の作成者が登場するテストと fixture は、すべて `userId` = name になっている（`powerpoint-shape-comment.pptx` の `Ada Lovelace` と、新しく作る作成者）。
  - `comments.rs:540-544` に次の 2 つの変異を入れ、`cargo test -p ooxml-pptx` を全部実行した。どちらも全テストが通った。
    - `attr(entry,"name")==Some(&author.name) && attr(entry,"userId")==Some(user)`（Local の照合に userId を含める）
    - `attr(entry,"providerId").or(Some("None"))`（`providerId` の無い作成者を Local として扱う）
  - `comments.py` の I13 も、同じ理由で両方を見逃す。
  - 挙動そのものは正しい（上のプローブ）。ただし F8 で PowerPoint の Local の `userId` が name 以外（たとえば `Anonymous_<name>`）だと分かれば、変わるのはこの照合の経路になる。
- 修正案: `SYNTHETIC:` のテストを 2 つ追加する。どちらも `powerpoint-slide-comment.pptx` の作成者を書き換えて作る。
  1. `providerId="None"`、`userId="Anonymous_X"` に書き換え、同じ name の Local の作成者が再利用されて、`authors.xml` がバイト一致することを確かめる。
  2. `providerId` を取り除き、同じ (`userId`, `AD`) の Account でも、同じ name の Local でも新しい項目が追加されることを確かめる。

### m1（Minor）: 既存のコメントパートがあり `p14:creationId` の無いスライドを `UnsupportedContent` で拒否しているが、ADR に無い拒否である
- 根拠:
  - `comments.rs:649-653` の拒否。
  - ADR の §3.2、§9、§11.6 が挙げる `UnsupportedContent` の理由は、「置き場所の外」と「u32 でない」の 2 つだけ。
  - この形は PowerPoint 自身が書く。legacy を変換すると `modernComment_1F3_0.xml` と `cId="0"` になる（post-acceptance item 10 と 11）。
  - このため `powerpoint-mixed-deck-resaved.pptx` が allowlist に 2 行入っている。
  - 安全側の拒否で、出力はバイト一致した。
- 修正案: ADR の post-acceptance note と §9 に、この拒否を記録する。または、既存パートに合わせて `cId="0"` を書くかどうかをオーナーに決めてもらう。

### m2（Minor）: 小文字の hex のパート名の変異（§11.7）が通り抜ける
- 根拠:
  - テストと oracle のケースで新しく作るパートの名前は、どれも数字だけになる（`modernComment_100_1`、M9 の `102_1`）。
  - `{slide_id:x}_{c_id:x}` の変異を入れても、`cargo test -p ooxml-pptx` は全部通った。
  - Python の I10 も、数字だけの名前では大文字と小文字を区別できない。
- 修正案: 新しく作るパートの名前に A–F が入るケースを 1 つ足す。たとえば `creationId` を持ちコメントの無いスライドへの追加。

### m3（Minor）: ADR §11.2 の「§9 のすべての行で Err ⇒ byte-identical」のテストが一部しか無い
- 根拠:
  - 単体テストが扱っているのは、legacy、`ShapeNotFound`、`Random`、入力の検証 5 種、CT、`extLst`、AC、author の id の重複、u32 でない値、`UnresolvedSibling`、id の走査。
  - ADR が名指しする次の項目にはテストが無い。
    - 置き場所の外の `p14:creationId`
    - 共有されたコメントパート
    - モダンの関係が 2 本ある場合
    - 関係の無い `p:ext{6950BFC3}`
    - 名前空間の接頭辞の衝突
    - authors の AC、関係 2 本、解析できない場合
    - `InsideAlternateContent`、`AmbiguousShapeId`、`p:contentPart`、`CreationIdsExhausted`、10000 年
    - `delete_slide` の側（壊れたパートがあっても `Ok`、id の予約）。`slides.rs` の新しい経路にテストが無い。
  - 上のプローブでは、すべて期待どおりに動いた。
- 修正案: プローブと同じ入力を `refuses(..)` の表に足す。`delete_slide` については、`add_comment_with_random` で予約済みの GUID を引かせるテストを足す。

### m4（Minor）: §11.7 の変異「`p14` をルートで宣言する」を検出するテストが無い
- 根拠:
  - Rust のテストは `xmlns:p14` を宣言している場所を見ていない。
  - `comments.py` は ElementTree を使うので、名前空間の宣言の位置が見えない。
  - 現在の出力は正しい（要素の上で宣言している）。
- 修正案: 新しいスライドの XML に `<p14:creationId xmlns:p14="…" val=` が含まれることを、バイト列で確かめる。

### n1（nit）: `tools/lo-check` の `odp_annotations` が死んだコードになっている
- 根拠: このキーを使う `expected.json` の項目が無い。post-acceptance note の LibreOffice の節では、「モダンコメントにはこのキーを足さない」としている。
- 修正案: キーを消すか、使い道（legacy のケースなど）をコメントに書く。

### n2（nit）: 既存の `p:sld/p:extLst` が `r` を別の URI に束縛し直しているとき、`commentRel` が `xmlns:r1` と `r1:id` になる
- 根拠: 要求したのは `NsDecl("r", R_NS)` だが、lens は名前を付け替える。XML としては正しいが、PowerPoint の綴り（`r:id`）とは違う。この入力はコーパスで 0 件。
- 修正案: 対応しなくてよい。気になるなら、要素の上の `r` の再宣言を lens に許させる。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 4 / nit 2）
