# PR #13 レビュー（Opus, ADR-0004 round 2 / 実装 round 1）

## 対象・方針

- 対象: PR #13（branch `impl/docx-phase0`, HEAD `91c997b`）。`docs/adr/0004-docx-phase0.md` revision 2 と `crates/ooxml-docx`（src / tests / examples）、`codegen/allowlist/wml.toml`、`codegen/overrides.toml`、`spikes/docx/` rev 2、roadmap、fixtures README。前提として ADR-0001/0002（post-acceptance notes 含む）、`crates/ooxml-core/README.md`、round 1 レビュー（`0004-opus-review-round1.md`）を読んだ。
- 方法: 読み取り専用の敵対的レビュー。worktree を scratchpad（`review-docx-p0/repo`）に複製して、次のことをした。
  - 既存テストを全部実行した（全 green）。
  - 敵対的プローブ `tests/adversarial.rs`（12本）を足して実行した。
  - `document.rs` にミューテーションを入れ、オラクルが実際に落ちるかを確かめた。
  - release ビルドで深い入れ子の時間とメモリを測った。
  - POI 129 件と SDK 非 Strict 444 件を `python3 -I`（zipfile/ElementTree、DTD を含む part は読み飛ばし）で走査した。
  
  fixture は parse だけに使った。リポジトリはこのファイル以外触っていない。
- 重大度:
  - **Blocker/Major**: 破損、黙ったデータ変更、保証や拒否の欠落、設計の誤り。
  - **Minor**: それ以外。
  - 判断が分かれるものには「判断」と付けた。

## 結論（先に）

round 1 の M1〜M3 と m1〜m9 は、実質すべて解消した（m5 と m8 は一部）。実装も ADR rev 2 の記述と一致していた。

- 確認できたこと:
  - 拒否はすべてミューテーションより前で起きる。
  - 書き込み経路は `body → (tbl→tr→tc)* → p → hyperlink* → r` だけをたどる。
  - 次の入力でも破損と黙った変更は出なかった。
    - 接頭辞の変形: 既定名前空間、`x:` 接頭辞で `w` を別 URI に束縛したもの、`w` を `w:p` 上でだけ宣言したもの。
    - テキスト: `]]>`、`&`、`<`、U+0085、U+2028、U+007F、`\r\n`。
    - run の形: 空の run、`rPr` だけの run、`rPr` が内容の後ろにある run、`proofErr`/bookmark に挟まれた run、`lastRenderedPageBreak` を持つ run。
    - 文脈: hyperlink 内のフィールド、フィールド内の hyperlink、表のセル内で閉じていないフィールド、footnotes/comments/テキストボックス内のフィールド。
  - T2 は「別の run に書く」ミューテーションで確かに落ちた。

ただし Major が2件ある。

1. **N1 深い入れ子でメモリが二乗に増え、スタックオーバーフローでプロセスが落ちる。** ADR §7 は「書き込みが 3〜4 s」としか書いていない。実際には open の時点で問題が出る。
   - 5.7 KB の docx の open で RSS が 4.7 GB になる。
   - 20 KB の docx では stack overflow で abort する（捕捉できない）。
   - 原因は ooxml-docx の側（run ごとに完全な path を保存していること、再帰による走査）にある。ただ、もっとも安く全体を直せるのは core のネスト深さ上限で、今入れるべき。
2. **N2 W2 がコードの先頭トークンだけで分類している。** そのため `INFO NumWords`、`INFO NumChars`、`DOCPROPERTY Words`/`Characters` などが safe として通る。同じ値を返す `NUMWORDS`/`NUMCHARS` は refused なので、W2 自身の基準（ADR L415-417）と食い違う。

どちらも局所的に直せ、設計の方向は変わらない。

## round-1 指摘の解消状況

| ID | 内容 | 状況 | 根拠 |
|---|---|---|---|
| M1 | 入れ子フィールドで外側のコードを捨てる | **解消** | `fields.rs:97-125` はスタックで追い、全階層を分類する。nested result の漏れ防止（`separated`）と未終端の分類もある。`tests/gates.rs:75-90` が IF{MERGEFIELD}、={PAGE}、HYPERLINK{REF}、{PAGE{DATE}} の漏れを確かめている。`sdk-complex2010` は `["=","SEQ","TC"]` になる（`fixtures.rs:130`）。spike rev 2 も直っている |
| M2 | `[read]` の可変降下と型なし走査 | **解消** | 読み取りは `RawNode::indexed_elements` を使う（`body.rs`）。書き込みは `into_child_at` で allowlist 上だけを降りる（`document.rs:245-277`）。ADR §3.4 と §4 の `ChildOf` に関する記述も訂正済み。core に足したのは `parsed_part` だけ |
| M3 | 文書保護のゲートが無い | **解消** | W3（`document.rs:102-112`）がある。fixture（POI blob の SHA-1 一致）と `gates.rs:43-64` の変種テストもある。別系統の保護は新規 m3 で扱う |
| m1 | テキスト対応表、空白ノード、own text | **解消** | `text.rs:map_text`（`\r\n`/`\r` → `w:br`）、`check_text_only` は空白ノードを無視しコメント/PI を拒否、no-op は `document.rs:205`。`editing.rs` が byte 単位で確かめている。`xml:space` の no-op の細部は新規 m6 |
| m2 | R1 の状態の範囲 | **解消** | 本文全体で1つのカウンタを持つ（`body.rs:197-212`）。未終端なら fail closed になり、append も拒否される（`gates.rs:111-130`、プローブ P09）。fldChar を認識できない場合の穴は新規 m1 |
| m3 | 分母の数え直し | **解消** | ADR §6 の表と `fixtures.rs:159-199` が一致する（3,418/3,455、hyperlink 308） |
| m4 | 到達範囲と `RunContext` | **解消** | ADR §3.1 に表がある。`gates.rs:133-158` がすべての variant を返す入力を持つ。R3 の fixture も観測できるものに直った |
| m5 | safe 基準の言い方、操作ごとの判定 | **一部** | 言い方は直った（ADR L415-417）。操作ごとの判定は Q2 に残っている。分類器がこの基準を守れていない（新規 N2） |
| m6 | W1 の fail closed と8通りのテスト | **解消** | `document.rs:99` は `unwrap_or(true)`。`gates.rs:30-40` は on 5通り（invalid を含む）、off 3通り、要素なし、part なしを確かめている。`True`、`" true"` も on になった（P05） |
| m7 | T5 の手順 | **解消** | `readback.rs` は `document.children` だけを読む。preserve は T2 で構造的に確かめている。T2 がその分岐を通らない点は新規 m4 |
| m8 | 古いまま残すキャッシュの一覧 | **ほぼ解消** | ADR §5 に一覧がある。`docProps/core.xml` が抜けている（新規 m5） |
| m9 | DTD はパーサのイベントで検出する | **解消** | core の `DocType` で検出する。`gates.rs:193-205`（10 kB のコメントの後の DOCTYPE、header 内の DTD）。XXE fixture も O2 で止まる |

## 指摘

### N1 [Major] 深い入れ子: open でメモリが二乗に増え、深さ 40,000 で stack overflow abort。ADR §7 の注記は過小評価

**根拠**
- `body.rs:111,119` は、`RunInfo`/`ParaInfo` ごとに part root からの完全な path（`Vec<usize>`）を持つ。`body.rs:181,209` は `self.path.clone()`。合計の長さは Σdepth なので、表の入れ子の深さを d とするとメモリは O(d²) になる。
- 走査は再帰で書かれている（`body.rs:166-195`、`fields.rs:97-125`）。
- 再現は scratch の `examples/gen_deep.rs`。Word と同じ形（各 `w:tc` の末尾に `w:p`）で、表を D 段入れ子にした。release ビルドで `examples/deep.rs` を実行して測った。

| D | docx（zip） | core parse だけの RSS | `Document::from_bytes` の RSS | open の時間 |
|---|---|---|---|---|
| 2,500 | 2.1 KB | 14 MB | 309 MB | 0.09 s |
| 5,000 | 3.3 KB | 26 MB | 1.2 GB | 0.32 s |
| 10,000 | 5.7 KB | 48 MB | **4.7 GB** | 1.19 s |
| 40,000 | 20 KB | （core の parse は通る） | **`thread 'main' has overflowed its stack` → abort** | — |

- POI の `deep-table-cell.docx`（17 KB）は RSS 1.2 GB で、1回の編集に 3.97〜4.02 s かかった（ADR の値を再現）。
- 参考: core 単体でも深さ 100,000 段（300,000 要素）の parse で stack overflow になる。
- 実ファイルの最大要素深さを測った。この fuzz ケースを除けば 33（POI `bug59058`）、中央値は 9（556 ファイル）。

**なぜ問題か**
- 数 KB の未信頼入力で、プロセスが GB 単位のメモリを確保するか、捕捉できない abort で落ちる。
- ADR-0001 以来、DTD、暗号化、Strict を「正直に型付きで拒否する」方針をとっている。それに対して、これは拒否されずにプロセスごと落ちる。
- ADR §7 L520-523 と README の注記は「書き込みが二乗」とだけ書いている。実際には open（読むだけの利用者も含む）で起きる。

**提案（task 4 への回答: core の修正は今要る。ただし lens の書き直しではなく深さの上限）**
- core のパーサにネスト深さの上限を入れる。例: 要素深さ 512 か 1,024 で `Error::TooDeep { limit }`。実ファイルとの余裕は 15 倍以上ある。これだけで二乗の時間（lens）、二乗のメモリ（docx の path）、再帰によるスタックの3つがすべて有界になり、xlsx/pptx にも効く。
- ooxml-docx 側は「O5 `Core(TooDeep)`」を §6 に足し、`deep-table-cell` 相当の合成テストで型付きの拒否を確かめる。
- path 表現の圧縮（親 index と子 index の arena）は Phase1 でよい。上限があれば Σdepth ≤ 上限 × run 数で線形に収まる。
- 書き込みの O(d²)（lens が path を毎回たどり直すこと）は、上限の下では無害。注記は「上限によって有界」と書き直す。

### N2 [Major] W2 の分類が先頭トークンだけで、統計値を返す `INFO`/`DOCPROPERTY` が safe として通る

**根拠**
- `fields.rs:55-61` はコードを先頭トークンだけで決める。`fields.rs:27-28` は `INFO`、`DOCPROPERTY`、`FILESIZE` を SAFE にしている。
- プローブ P04: `<w:p>{ INFO NumWords }</w:p><w:p>body</w:p>` の結果。
  - `INFO NumWords`、`INFO NumChars`、`DOCPROPERTY Words`、`DOCPROPERTY "Characters"`、`FILESIZE` → `write_gate() == Ok(())`。
  - `NUMWORDS` → `Err(StaleFieldRisk { codes: ["NUMWORDS"] })`。
- ADR L415-417 の基準は「キャッシュされた結果が Phase0 の変えられないものか、ページ割り（Word がレイアウトのたびに再計算する）だけに依存する」。`INFO NumWords` と `DOCPROPERTY Words`/`Characters`/`CharactersWithSpaces`/`Lines`/`Paragraphs`/`Bytes` は、`NUMWORDS`/`NUMCHARS` と同じ統計値を返す。Word はこれらをレイアウトでは再計算しない（F9 か印刷時の更新だけ）。
- コーパスでの出現は 0 件（POI と SDK を走査）。影響を受ける実ファイルは少ない。それでも W2 が保証すると言っている内容の穴にあたる。

**提案**
- `INFO` と `DOCPROPERTY` は引数込みで分類する。
  - 第1引数（引用符を外して大文字にしたもの）が `NUMWORDS`/`NUMCHARS`/`WORDS`/`CHARACTERS`/`CHARACTERSWITHSPACES`/`LINES`/`PARAGRAPHS`/`BYTES`/`FILESIZE` なら refused。
  - `NUMPAGES`/`PAGES` はページ割りなので safe のまま。
  - 引数が無いか解析できないときは refused（fail closed）。
- `FILESIZE`（判断）は、Phase0 の保存でファイルサイズが変わる。`SAVEDATE`（core.xml の更新日時と同じく古いまま）と同じ扱いにするなら safe のままでよい。どちらにするかを ADR に一行書く。
- `gates.rs::field_classification` に上の5ケースを足す。

### m1 [Minor] 認識できない `w:fldChar` で R1 と W2 が fail open になる

**根拠**
- `fields.rs:113`（`_ => {}`）と `body.rs:202-206` は、`fldCharType` が無いもの、名前空間なしの `fldCharType`、大文字の `Begin` を無視する。`fields.rs:115-118` も、開いたフィールドが無いときの `w:instrText` を捨てる。
- P03: `<w:fldChar/>` + `TOC \o` + separate + `RESULT` + end の場合、`write_gate()` は `Ok` になり、`RESULT` の run は `in_field == false` で settable になる。
- P07: 段落直下の `mc:AlternateContent` の中にある begin/separate は本文の走査では見えない。そのため、外にある結果の run が settable になる。W2 は両方の branch を数えて PAGE を safe とする。
- 実際の Word はこれらを書かない（スキーマ違反）。ただ、W1 や W3 の不正値を「on とみなす」ADR 全体の方針と食い違う。

**提案**
- 次のいずれかがあれば、W2 で `<malformed field>` として文書を拒否する。
  - 値が begin/separate/end 以外の `w:fldChar`。
  - 開いたフィールドが無いところの `w:instrText`/`w:delInstrText`。
  - 段落レベルの `mc:AlternateContent` の中の `w:fldChar`。
- この3つの合成テストを足す。

### m2 [Minor] 最後の節の `sectPr` が最後の段落の `pPr` にあり、本文直下に `sectPr` が無いとき、append が新しい節を作る

**根拠**
- P01: 本文が `<w:p>…last…</w:p><w:p><w:pPr><w:sectPr><w:pgSz … w:orient="landscape"/></w:sectPr></w:pPr></w:p>` で、本文直下に `sectPr` が無い場合。追加した段落は、その段落の**後ろ**に入る（`…</w:p><w:p><w:r><w:t>NEW</w:t></w:r></w:p>`）。
- Word の意味では、追加した段落は `sectPr` を持たない新しい最後の節に属し、既定の用紙・余白・節区切りになる *(u)*。たとえば横向きの文書の最後に、縦向きの新しいページができる。
- ADR §3.2 L253-257 は、本文直下の `sectPr` がある場合（`poi-Headers`）しか扱っていない。コーパスでの出現は 0 件。

**提案**
- 本文直下に `sectPr` が無く、最後のブロックが `pPr/sectPr` を持つ段落なら、型付きで拒否する（例: `Error::NoBodySectPr`）。ADR の A 行に1行足す。
- 本文直下に `sectPr` が無い場合でも、最後の段落が節区切りでなければ、今の挙動（`poi-55733`）のままでよい。

### m3 [Minor, 判断] W3 以外の保護信号と、重複要素の読み方

**根拠**
- P05:
  - `w:writeProtection`（「書き込みパスワード」のハッシュあり、または `w:recommended="1"`）では、`write_gates()` は `[]` になる。SDK のコーパスに `write_protected.docx`（recommended）が1件ある。
  - 「最終版にする」（`docProps/custom.xml` の `_MarkAsFinal`）も見ていない（コーパスでは 0 件）。
- `documentProtection` や `trackRevisions` が2個あると、生成 view は最初の1個しか見ない。
  - `<w:documentProtection w:edit="none"/><w:documentProtection w:edit="readOnly" w:enforcement="1"/>` は `[]`。
  - `<w:trackRevisions w:val="0"/><w:trackRevisions/>` も `[]`。
  - どちらも `maxOccurs=1` の違反で、Word が開くかは未確認。

**提案**
- `writeProtection` は内容の編集制限ではない（上書き保存の鍵）。拒否まではしなくてよいと考える。代わりに `Document::write_protection() -> Option<..>` のように報告し、ADR §5 か §6 に「拒否しない保護」として `writeProtection` と `_MarkAsFinal` を明記する。
- 重複は「どれか1つでも on なら on」とする（fail closed）。
- Phase1 では、`readOnly` と `w:permStart`/`permEnd`（全員が編集できる範囲）を組み合わせたテンプレートがよく使われることを、opt-in の検討材料として roadmap に書いておく。

### m4 [Minor] オラクルの検出力: `rPr` の保持と二重空白の preserve は fixture 全体のオラクルで検出できない

**根拠（task 3、ミューテーションの結果）**

| ミューテーション（`document.rs`） | T1–T6（`oracles`, `readback`, `fixtures`, `gates`） | 落ちるテスト |
|---|---|---|
| M2: 編集のたびに `w:rPr` も消す（3,423 run の書式が黙って消える） | **すべて pass** | `editing.rs` の手書きの byte 比較3件だけ |
| M1: domain 側の二重空白の `preserve` を消す | `t2_every_settable_run` は **pass**。`t2_append_is_a_pure_insertion` が fail | append と `editing.rs` |
| M4: 隣の run に書く | T2 と T5 が fail | （正しく検出） |

- T2 の2つ目のテキストは `" {own}\tx  y "`（`oracles.rs:142`）。tab で区切ると、二重空白を含む片 `x  y ` は末尾の空白で必ず core の preserve を通る。そのため domain の二重空白の規則（端は空白でないが中に二重空白）を T2 は一度も通らない。
- T2 は `run.text()` を、再パースしていないメモリ上の木から読む（`oracles.rs:152`）。直列化の誤りを見るのは T5（ASCII の marker 41件）と T1b（既存テキスト）だけ。
- T6 は core の slot 挿入で構造上いつも成り立つ。core の回帰テストとしては意味があるが、domain の誤りは検出できない。ADR はそう書いておけばよい。

**提案**
- T2 で、編集前後の run の開始タグと `w:rPr` の部分の byte が同じであることを確かめる。span の中のどの部分が変わってよいかを固定する。
- 2つ目のテキストを `"a  b\t {own} "` のように、端が空白でない二重空白の片を含む形にする。
- fixture ごとに、編集を終えた後の `to_bytes()` を再度 `Document::from_bytes` し、各 run の `text()` を期待値と比べる。

### m5 [Minor] §5 の「古いまま残すキャッシュ」に `docProps/core.xml` が無い

- `dcterms:modified`、`cp:lastModifiedBy`、`cp:revision` が抜けている。SAFE の `SAVEDATE`/`LASTSAVEDBY`/`REVNUM` は、これらと矛盾しないので safe と言える。その対応関係を一覧に足す。
- `docProps/custom.xml` も、`DOCPROPERTY` の参照元として足す。

### m6 [Minor, 判断] no-op 判定が `xml:space` を見ない

- P10: `<w:t> a</w:t>`（preserve なし）に対する `set_run_text(.., " a")` は `Ok` の no-op で、byte も変わらない。Word は先頭の空白を落として表示する *(u)* ので、呼び出し側が明示的に求めた文字列と表示が食い違う。
- ADR §3.2 step 6 の「no-op は producer の書いたとおりに残す」は own text の恒等性（T2）のための選択で、筋は通っている。
- 代案: `needs_preserve(text)` で preserve が無い `w:t` があれば no-op にせず、属性を1つ足す。T2 の own-text を byte 一致にする条件に「preserve が必要なのに無い `w:t` を含まない」を加える。どちらにするかを ADR に書く。

## task 4: 深い入れ子の O(depth²) 注記について

N1 を参照。注記どおり書き込み時間だけの問題なら、受け入れてよかった。実際には次の3つが重なっており、受け入れられない。

- open の時点でメモリが O(d²) になる（ooxml-docx の path 保存による）。
- 再帰による stack overflow abort（ooxml-docx は深さ約 40k、core は約 100k 段）。
- 入力はわずか数 KB で、zip での圧縮率も高い。

core にネスト深さの上限（型付きエラー）を今入れるのが、最小で効く修正である。lens を path ベースでなくする書き直しは不要。

## task 5: owner の質問 2〜3 への推奨

### Q2 操作ごとの W2 → **Phase1 でやる。ただし「内容に依存しない」コードと操作の組み合わせに絞る**

- `set_run_text`（フィールドの外の run の文字列置換）と `append_paragraph`（スタイルなしの段落を末尾に追加）のどちらでも結果が変わらないコード:
  - `SEQ`、`TC`、`XE`、`NOTEREF`、`LISTNUM`、`AUTONUM*`
  - 理由: どれもフィールドの個数・順序、命令の文字列、注の番号だけで決まる。Phase0 の2つの操作はそのどれも変えない。
- `append` に限って変わらないコード: `TOC`、`PAGEREF`、`REF`。ただし次の注意がある。
  - 既定の段落スタイルに `w:outlineLvl` があり、TOC が `\u` を持つ場合、追加した段落が目次に入る。
  - 文末脚注は追加した段落の後ろに移るので、それを指す `PAGEREF` は変わりうる。
  - これらを確かめる手間を考えると、Phase1 の後半か `updateFields` の opt-in と一緒に扱うのが妥当。
- `set_run_text` で変わりうるコード: `TOC`、`REF`、`PAGEREF`、`STYLEREF`、`=`、`IF`、`NUMWORDS`（N2 の `INFO`/`DOCPROPERTY` の統計値も）。これらは refused のままにする。
- 実装は `fields.rs` の2列の表（code → {SetRun, Append} の可否）で済む。今の SAFE は「両方とも可」の特別な場合にあたる。テストは表を総当たりする。

### Q3 `IF` と `updateFields` → **`updateFields` だけを解除の条件にしない。`IF` 自身は構文で判定する**

- `IF` の結果が Phase0 の編集で変わらないのは、次の2つが成り立つときに限る。
  - (a) 入れ子のフィールドがすべて safe である。
  - (b) 比較の両辺に**ブックマーク名（裸の識別子）が無い**。`{ IF bm = "x" … }` の `bm` は、ブックマークの内容への参照になる。
- 差し込み印刷の定型 `{ IF { MERGEFIELD G } = "F" "Ms." "Mr." }` は (a) と (b) を満たす。Phase1 で、命令を字句に分ける tokenizer（引用符、数値、演算子、入れ子フィールドのプレースホルダ）で判定すれば、安全に通せる。
- `updateFields` の opt-in には別の役割がある。(a) と (b) を満たさない文書（`REF`/`TOC` を含む）を、利用者が「Word で開いたときに更新される」ことを承知して書き換える手段にする。Word 以外の reader では古いまま残るので、これを既定にはしない（ADR の Alternatives 4 のとおり）。
- 補足: `settings/w:mailMerge` がある差し込み印刷のメイン文書では、差し込みの実行時に `IF` と `MERGEFIELD` は必ず評価し直される。Phase1 の判断材料として記録しておく価値がある *(u)*。

## 判定

- **converged（Blocker/Major が無い）: no**
  - Major は N1（深さの上限と O5 の拒否）と N2（`INFO`/`DOCPROPERTY` の引数込み分類）の2件。どちらも数十行の局所的な修正で済み、設計の方向は変わらない。
- **ADR を Minor の修正だけで Accepted にできるか: no**
  - N1 と N2 は、ADR §6/§7 の本文（O5 の行、W2 の分類規則、深さの注記）を直す必要がある。
  - 直したあとの次ラウンドで確かめるのは、N1 と N2 の修正と、それぞれのテスト（深い入れ子が型付きで拒否されること、統計値フィールドが refused になること）だけで足りる見込み。
  - Minor（m1〜m6）は同時に直しても後回しでもよい。

### 再現手順

- プローブ一式: `<scratchpad>/review-docx-p0/repo/crates/ooxml-docx/tests/adversarial.rs`（P01〜P12）、`examples/gen_deep.rs`、`examples/deep.rs`。
- 実行: `cargo test -p ooxml-docx --test adversarial -- --nocapture --test-threads 1`、`cargo build --release -p ooxml-docx --example gen_deep --example deep`。
- 最小の再現:
  - N2: `docx("<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> INFO NumWords </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>3</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>", None, None)` の `write_gate()` が `Ok`。
  - N1: `"<w:tbl><w:tr><w:tc>" × D` + `<w:p/>` + `("</w:tc></w:tr></w:tbl><w:p/>") × D` で、D = 10,000 のとき RSS が 4.7 GB、D = 40,000 のとき stack overflow で abort。
  - m2: プローブ P01。
  - m1: プローブ P03 と P07。
