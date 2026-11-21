# PR #141 レビュー（Opus, round 1）

対象: `fixtures/pptx-p4a0-comments` @ `b0470dfa54eb63af5eff833c47ed2292a18c1f6b`

## 結論（先に）
converged: **yes**。Blocker と Major はない。
新しい 9 個の `.pptx` を全部 unzip して、全 part（UTF-16 と binary を含む）を調べた。所有者の実名、アカウント名、メール、テナント名、Windows のユーザー名・パス、マシン名は出てこない。
ADR ノートの主張（`complete="100000"`、入れ子のフラットなチェーン、legacy → modern 変換、`.000` の脱落）は fixture の中身と一致した。survey の再実行はバイト単位で一致する。LibreOffice baseline も手元の 25.2.7.2 で再現できた。
残りは Minor 4 件と nit 5 件。うち 2 件はプライバシーの衛生上の指摘で、いずれも識別子の漏えいではない。

## 前ラウンドの解消状況
初回ラウンドのため、なし。

## プライバシー確認（最優先）
- **検索した語:** 所有者のハンドルと氏名（ローマ字の大文字・小文字）、`@`、`S::`、`C:\Users`/`\Users\`、マシン名、`onmicrosoft`、一般的なメールドメイン。全 part を UTF-8・UTF-16LE・UTF-16BE で、binary は `strings` でも調べた。ヒットなし。
- **メールアドレス:** 見つかったのは `example.com` のダミーと、`docProps/custom.xml` の `MailAddress` だけ。後者は 45545 由来の 2 ファイルにあり、`poi/45545_Comment.pptx` と同じバイト列（第三者が公開済みのファイルで、既にリポジトリにある）。
- **作者まわり:** `authors.xml` の `userId` は全部ダミー（`S::*@example.com::<作った GUID>`）か、POI 由来の `Anonymous_XPVMWARE01`。legacy の `p15:presenceInfo` もダミーだった。
- **core.xml / app.xml:** core.xml の creator と lastModifiedBy は `Author One` か POI 原本の値。app.xml の `Company` と `Manager` は空か POI 原本の値。
- **custom.xml:** 45545 由来の 2 ファイルにだけあり、原本と同じ内容。
- **画像:** サムネイル 10 枚と `image1.png` を目視した。fixture の見出しテキスト、図形、POI 原本の表紙しか写っていない。
- **埋め込み:** `oleObject*.bin` は原本と同一。
- **zip:** comment と extra はなく、日時は 1980-01-01。
- **作者 `id`（`{FE64…}`）:** README が書くとおり、実アカウントに対して PowerPoint が導出した値である。ただし復号はできない。
  - 入力の `userId` には、推測できないオブジェクト GUID が含まれる。
  - 既知の 3 組（`Anonymous_XPVMWARE01`/`None` と、2 つのダミー `S::`/`AD`）で、MD5・SHA 系・BLAKE2 の連結（区切り 14 種、大小文字、UTF-8/16、GUID のバイト順 2 通り）と UUIDv3/v5 を試した。どれも一致しなかった。
  - メールやテナント GUID は含まれていない。この値は #123 で既に公開されている。
  - よって Blocker ではない。Minor m1 を参照。

## Findings

### m1（Minor）: 残した作者 `id` は、アカウントを横断して結び付けられる安定した仮名で、fixture 内の値とも整合しない
- 根拠: README（project 節の箇条書き）は `{FE64…}` を「実アカウントに対する PowerPoint の値」と明記している。ADR ノート item 5 は、この値が (`userId`, `providerId`) から決定的に導出されると述べている。
  - そのため、このアカウントが PowerPoint でコメントした他の文書は、すべて同じ `authorId` を持つ。公開リポジトリが、その値と所有者を結び付けてしまう。
  - さらに、fixture の `userId` はダミーに置き換えたので、(`userId`, `providerId`) → `id` の組は PowerPoint の導出と合わない。後で導出アルゴリズムを検証する人が、この組を使うと誤る。
- 修正案（所有者の判断）: ダミーの `S::author.one@example.com::1d0c…`/`AD` で COM `Add2` のプローブを 1 回実行し、PowerPoint が返す `id` を 10 ファイル全部に入れる。
  - こうすれば、相関に使える値が消え、fixture も自己整合する。
  - #123 の値は履歴に残るが、これ以上広がらない。
  - 置き換えない場合は、README のその行に「相関に使える値を意図して残した」と書く。

### m2（Minor）: ADR item 7 と README が、実名の表示名の形を説明している
- 根拠: ADR item 7 は「実名の表示名には空白がなく、イニシャルは CJK 2 文字で、名の 1 字目、姓の 1 字目の順」と書いている。README も「a Japanese display name」「two CJK characters」と書いている。
  - 個人を特定はできないが、結論（PowerPoint は署名済みアカウントのイニシャルを `name` からではなくアカウントから取る）には要らない情報である。
- 修正案: 「署名済みアカウントのイニシャルは `name` から作られず、アカウントの値が使われた」だけを残す。

### m3（Minor）: item 5・9・10 の根拠の一部が、リポジトリにない
- 根拠:
  - item 5 の「別の `name` でも同じ id」と「`providerId="None"` だと別の id（`{84BB…}`）」は、COM で作ったプローブ 5 件（item 6）に依っている。このプローブはどこにも保存されておらず、README にも手順がない。
  - item 9 の「`.000` が落ちる」と item 10 の混在デッキは、Linux で手作りした入力を前提にしている。この入力はコミットされていない。
  - 入力の `Probe User` の作者 `id` が記録されていない。出力では `{9D15…}` である。PowerPoint が作者 id を読み込み時に導出し直すのか、入力の値を保ったのかを判別できない。これは M10 と Q1 の論点に直結する。
- 修正案（どちらか、または両方）:
  - 手作りの入力（第三者部分は 45545 のまま）を `project/` か `spikes/pptx-phase4a/` に置き、生成スクリプトか差分を README に書く。
  - 入力の `authorId` と、プローブ 5 件の（`name`, `userId`, `providerId`）→ `id` の対応表を README か ADR に書く。

### m4（Minor）: item 10 が、変換のきっかけを切り分けた対照を明示していない
- 根拠: `powerpoint-legacy-deck-comment.pptx` は、PowerPoint が 45545 を保存しても legacy コメントを変換しなかった例である。`comment1.xml` と `comment2.xml` は原本とバイト単位で同じ。つまりこのファイルは、「変換のきっかけは modern part（と `authors.xml`）の存在」という主張の対照になっている。
  - しかし item 10 はそう書いていない。そのため、読み手は「PowerPoint は 45545 をいつでも変換する」可能性を排除できず、提案 3（デッキ単位での拒否）の根拠が弱く見える。
- 修正案: item 10 に「対照: 同じデッキを legacy コメントだけで保存した `powerpoint-legacy-deck-comment.pptx` では変換されない」と 1 行足す。

### n1（nit）: item 6 の「変換された id は non-RFC variant」が半分しか正しくない
- `{8CF1…-3F99-0A98-…}` は variant `0` だが、`{6772…-FC2A-A7BA-…}` は variant `A`（RFC）である。「version nibble は 3 と F、variant は一方が non-RFC」に直す。

### n2（nit）: item 7 の「PowerPoint has no such rule to match」は言い過ぎ
- 署名済みアカウントのイニシャルは「名 + 姓」の順で、ディレクトリの値から作られていると読める。これは §4 の例（`山田 太郎` → `山太`、姓 + 名）と逆の順である。
- 結論（§4 の既定を維持し、M4 で記録する）は変わらない。「署名済みアカウントでは Office がアカウントから決め、COM では呼び出し側の値をそのまま使う。`name` から作る規則は観測されない」と書けば正確になる。

### n3（nit）: §11.5 の `odp_annotations` キーが `office:annotation` を比較すると書いている
- Impress の ODP は `officeooo:annotation` を出力する。手元で確認した: 45545 → 2 件、modern fixture → 0 件。ノート側の記述は正しい。
- キーを後で復活させるときのために、§11.5 の要素名を `officeooo:annotation` に直す。

### n4（nit）: LibreOffice 節の「all six files」がどれを指すか曖昧
- 2 つの baseline と F2〜F5 を数えると、F3 を 1 件と 2 件のどちらで数えるかで 6 にも 7 にもなる。ファイル名を列挙する。

### n5（nit）: M8 は「45545 の slide 2」を指しているが、slide 2 には legacy コメント（`comment2.xml`）がある
- これは既存の不一致で、提案 3 で M8 を書き直すときに一緒に直せる。

## 確認したこと（問題なし）
- SHA-256: README のブロック 33 件で `sha256sum -c` を実行し、全部 OK。サイズと合計（2,834,424）も一致した。
- 各行と F 番号の対応:
  - F1: `pc:sldMkLst` で、`pos` はない。
  - F2: 2 人目の作者は `authorLst` の末尾に追加され、`initials=""`。返信は `txBody` の前にある。
  - F3: 2 ファイルの差は `status="resolved"` と `complete="100000"` だけで、id は同じ。`active` はない。
  - F4: 各 kind の `id`/`creationId` は、スライド上の `a16:creationId` と一致する。入れ子の楕円は `[docMk, sldMk, spMk id=3]` で、`grpSpMk` はない。
  - F5: `cp=42`/`len=15`/`context len=58` は本文と COM の `Characters(43,15)` に合う。
  - F9: 2 つ目のスレッドは末尾に追加されている。
- F6 の 2 ファイルを `poi/45545_Comment.pptx` と比べた:
  - legacy の方は `comment3.xml` と `p:cmAuthor id=1`（presenceInfo はダミー）が増えただけで、`comment1/2` は同一。
  - resaved の方では、`comment1/2` が `modernComment_1F3_0`/`_1D8_0`（`cId="0"`）になっている。`p188:pos` は `p:pos` × 1587.5 で、`created` は `dt` と一致する（ミリ秒も保たれる）。作者は `Anonymous_XPVMWARE01`/`None` になり、`commentAuthors.xml` は残る。手作りのスレッドの `created` は `…T21:30:00`（小数なし）。
- survey: `python3 -I spikes/pptx-phase4a/comments_survey.py reference/fixtures/pptx/project` の出力は、`results/comments-p4a0-project.txt` とバイト単位で同一。ノートの数（10 デッキ、16 `p188:cm`、2 返信、11 作者、13/13、15/15）とも合う。
- LibreOffice 25.2.7.2（`5cbfd1ab`）で再現した:
  - ODP の `officeooo:annotation` は、shape、modern-comments、F2、resaved のいずれも 0 件。対照の 45545 は 2 件。
  - F2 を `--convert-to pptx` すると、comment と authors の part がなくなる。
  - §11.5 のフォールバック（「開いて保存し直せる」に縮小し、post-acceptance note に書く）は、書かれたとおりに適用されている。
- 3 つの Proposed は、どれも Owner decisions に書かれたフォールバックに沿っている（Q2 は converting、Q7 は `active` なし）。所有者の確認待ちと明記されている。
- ライブラリのコードは変わっていない（変更は `docs/`、`reference/`、`spikes/` だけ）。`cargo test -p ooxml-pptx` は手元で通った（21 suite、失敗 0）。CI はレビュー時点で Rust ジョブ以外が pass で、Rust ジョブは pending だった。
- 同じファイルに触れる open PR はない。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 4 / nit 5）
