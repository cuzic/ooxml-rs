# PR #122 レビュー（Opus, round 1）

対象: impl/core-set-leaf-text @ 46976f15db16e56107af58d71403aac5259ec99d

## 結論（先に）
経路の照合、拒否時のバイト同一性、T2、エスケープ、check と set の一致は、プローブで崩せなかった。
ただし、型付き名前空間の一覧が ADR の定義（生成器がスキーマを持つ名前空間）より広い。そのため、ADR-0015 §6.2 の表で
「Supported」とされている cover page の custom XML store（SDK 25 件）が `TypedNamespace` で拒否され、D3d が仕様どおりに実装できない（M1）。
converged: no（Major 1）。

## 前ラウンドの解消状況
round 1 のため、なし。

## Findings

### M1（Major）: `TYPED_ELEMENT_NAMESPACES` が cover page の store を拒否し、ADR-0015 §6.2 の「Supported」と矛盾する
- 根拠:
  - ADR-0015 L562 は、`customXml/itemN.xml` の store（「cover page 25, other 37」）への書き込みを Supported としている。
    L594-597 では、拒否の対象を「the namespaces the generator has schemas for」と定めている。
  - `name.rs:43` に `http://schemas.microsoft.com/office/2006/coverPageProps` がある。しかし生成器の allowlist（`codegen/allowlist/{dml,pml,props,sml,wml}.toml`）にも生成物にもこの名前空間はなく、`reference/openxml-sdk-data/schemas/` に置かれているだけである。
    PR の note（ADR 末尾）は、一覧を「vendored SDK and Transitional XSD targets」に言い換え、定義を広げている。
  - プローブ: `<CoverPageProperties xmlns="…/coverPageProps"><PublishDate/><Abstract/>…</CoverPageProperties>` に
    `[CoverPageProperties, Abstract]` を指定すると、check と set の両方が `Err(TypedNamespace)` を返す。Word の表紙の
    「要約」「発行日」「会社の住所」などのコントロールは、この store に bind している。
  - 同じ理由で、`name.rs:176` の bibliography（`b:Sources`、customXml の store として置かれる）や、SharePoint の
    `metadata/*`、`customXsn` も、生成器が型を持たないのに拒否される。
- 修正案: 一覧を、生成器が実際に型を生成する名前空間（allowlist の入力から導く。少なくとも coverPageProps を外す）に絞る。
  あわせて、cover page store への書き込みが成功するテストと、一覧が生成器の入力と同期していることを検査するテストを足す。
  cover page を拒否のままにしたい場合は、ADR-0015 §6.2 の表を `UnsupportedBinding` に直す必要があり、これは owner の判断になる。

### m1（Minor）: ADR-0015 の追記が main と衝突している
- 根拠: `git merge-tree HEAD origin/main` で `docs/adr/0015-docx-phase3-review-features.md` が CONFLICT になる。
  PR #99（マージ済み、`25bfff8`）が、同じ末尾に「Post-acceptance note (D3c, Issue #98)」を追加しているためである。
  どちらも追記なので、内容は衝突しない。
- 修正案: Codex が origin/main をマージし、D3c の note の後ろに #105 の note を置く。マージコミットのメッセージに、どう解消したかを書く。

### m2（Minor）: `xsi:nil` の値が boolean として解釈できないときは書き込みが通る
- 根拠: `leaf_text.rs:88` は `parse_bool` が `Some(true)` を返すときだけ拒否する。プローブでは `i:nil='yes'` が
  `Ok` になり、`<a i:nil='yes'>z</a>` と書き込まれる。入力はもともとスキーマに違反しているが、`nil` を宣言した要素に文字列を入れることになる。
- 修正案: `xsi:nil` が `false` と `0` 以外の値なら `NilElement` で拒否する（安全側に倒す）。そのテストを足す。

### m3（Minor）: fuzz で check と set の一致を確かめていない
- 根拠: `fuzz/fuzz_targets/` に `set_leaf_text` を呼ぶ target がない。check と set は `validate_leaf_text` を共有しているので、
  現状は一致している。ただし、今後この関数を分けたときに不一致を検出する仕組みがない。
- 修正案: 既存の `xml_t1` か `lens_ops` に、次を検査する処理を足す。任意の経路と値で `check` の結果と `set` の結果が一致すること。
  `Err` のときはシリアライズ結果が同一であること。`Ok` のときは再パースしたテキストが値と一致すること。

### n1（nit）: `value.rs` の `check_xml_chars` の後に空行がない
- `parse_str` の doc コメントの直前に空行を入れる。

## 確認して問題がなかった点（プローブ）
- 名前空間: 接頭辞を再束縛しても URI で照合する（`xmlns:p` の入れ子）。`xmlns=''` で既定の名前空間を解除した要素にも届く。
  同名の兄弟は index で選べ、`usize::MAX` は `NoSuchElement` になる。葉より深い経路も `NoSuchElement` になる。
- 自己終了タグ: 要素に `xmlns` や `xmlns:p` の宣言があっても、属性の表記（`&quot;`、空白）を保ったまま `<a …>new</a>` になる。
  ルート要素が自己終了タグで、前後に BOM、XML 宣言、コメントがあっても、前後のバイトは変わらない。
  同じ要素に x→y→"" と続けて書くと `<b  k = '1' />` に戻る。
- エスケープ: `]]>`、`&`、`<`、`\r`（`&#13;`）は正しくエスケープされ、再パースで値に戻る。`\t`、`\n`、`_x0041_`、U+0085 はそのまま出力される。
  U+0001 と U+FFFE は check と set の両方で同じ offset の `InvalidXmlChar` になる。
- 同じ値の書き込み: CDATA だけのテキスト、元の CRLF（`x\ny` との比較）は無変更になる。`x\r\ny` を書くと、差分は `&#13;` の部分だけになる。
- 拒否: 混在内容は `NotLeaf`。`nil='true'` は、値が空文字列でも `NilElement` になる。`nil='0'` と、XSI 以外の名前空間の `nil` は書き込みを通す。
  型付き名前空間の属性を持つだけの要素は通す。`xml:` 名前空間の要素は `TypedNamespace` になる。
  どの拒否でも、check と set のエラーの種類は一致し、出力は入力とバイト単位で同一だった。`Ok` で書いた後の `Err` も、その時点の状態を変えない。
- `lens.rs` の `mark_path` を `mark_path_dirty` に置き換えても、挙動は変わらない（同じループ）。
- CI: Rust、python-pptx、python-docx、openpyxl、LibreOffice がすべて pass（`gh pr checks 122`）。
- 他の open PR で `ooxml-core` の同じファイルや ADR-0015 を触るものはない（`gh pr list`）。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 3 / nit 1）
