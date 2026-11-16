# ADR-0001 レビュー（Opus, round 1）

対象: `docs/adr/0001-lossless-round-trip-architecture.md`（Status: Proposed, 2026-11-15）
方針: 読み取り専用レビュー。重大度は **[Blocker]**（コード着手前に ADR 修正が必要）/ **[Major]**（Phase0 のテスト設計か範囲記述に影響）/ **[Minor]** で分類。

## 結論（先に）

2層設計（OPC パート単位パススルー + パート内生XMLツリー/レンズ）の**方向性自体は妥当**で、openpyxl 型の破壊を避ける判断として正しい。ただし ADR は「パート単位で独立に dirty/passthrough を判定できる」ことを暗黙の前提にしており、実際の xlsx は**パート間で数値インデックスと名前による参照網**を持つため、その前提は成り立たない。この「パート間整合性」の責務を持つ層が ADR に存在しない（B1）。また「同じ基盤を共有する」という主張（B2）と「構造は保つがバイトは保たない」という中間状態（B3）は、どちらも現状の記述のままでは実装・テストの判定基準が定まらない。加えて、依存の MSRV が workspace 設定と矛盾しており、このままではビルドが通らない（B4）。

---

## B1 [Blocker] パート間整合性の責務を持つ層が無い（重点1）

### 根拠
- Layer 1: 「`set_part` で書かれたパートだけ再シリアライズ」（L50-53）。
- Layer 2: 「`Worksheet::cell(r, c)` は `<c>` ノードを find-or-create して値を読み書きさせる」（L80-82）。
- Phase0 スコープ: 生ツリーは「`sheet1.xml` only」（L118）、成功条件は「1セルを編集 → shared string table と styles が unchanged で残る」（L120-123）。

### なぜ問題か
xlsx のセル1個の編集は、典型的に次のパートへ波及する。ADR はこのどれにも言及していない。

| 編集 | 波及先パート | 何を更新する必要があるか |
|---|---|---|
| 新しい文字列を書く | `xl/sharedStrings.xml` | `<si>` 追加、`count`/`uniqueCount`、新インデックスを `sheet1.xml` の `<v>` に書く |
| 書式付きで書く | `xl/styles.xml` | `cellXfs` に `<xf>` 追加（+ `fonts`/`fills`/`numFmts` の id 採番、カスタム numFmt は 164 以上） |
| 数式セルを値で上書き | `xl/calcChain.xml` | 該当エントリ削除。残すと Excel が「修復」ダイアログを出す（openpyxl が calcChain を丸ごと捨てる理由がこれ） |
| 数式の参照元を書き換え | `xl/workbook.xml` | 他セルのキャッシュ値が stale になるため `<calcPr fullCalcOnLoad="1"/>` 等が必要 |
| シートを追加 | `workbook.xml`, `workbook.xml.rels`, `[Content_Types].xml`, `docProps/app.xml` | `<sheet>`、rId、Override、`TitlesOfParts`/`HeadingPairs` のシート名リスト |
| 新パートを追加（全般） | `[Content_Types].xml`, 親の `_rels` | OPC レベルの登録 |

具体的失敗シナリオ:
1. `ws.cell(1,1).set_string("新規")` → lens は `sheet1.xml` に `<c t="s"><v>?</v></c>` を作るが、`?` に入るインデックスを決める主体が無い。Phase0 で sharedStrings は生ツリー対象外（L118）なので、Layer 1 の判定では「触っていない」→ 元バイトのまま raw copy → **シートが存在しない sst インデックスを指すファイルが出力される**。Excel は修復ダイアログを出し、当該セルを消す。
2. Phase0 の成功条件（L120-123「shared string table ... survive unchanged」）は、**数値を書く場合か、既存文字列と同じインデックスを使う場合にしか成立しない**。つまり受け入れテストが、ちょうどこの問題を踏まない形に設計されている。テストは通るが、`write_string` を round-trip パスで使った瞬間に壊れる。
3. 逆方向: 「sst を整理して使われなくなった `<si>` を詰める」実装を誰かが善意で入れると、インデックスがずれ、**触っていない `sheet2.xml` 〜 `sheetN.xml` すべてを書き換えなければならなくなる**。これは Layer 1 のパススルー保証そのものを破壊する。ADR はこれを禁止する不変条件を書いていない。

### 改善の方向性
- 第3の責務として「ワークブック単位のインデックスレジストリ」（`SharedStringTable`, `StyleTable`, `ContentTypes`, `Relationships`, `CalcChain` ポリシー）を明示し、所有者を決める。OPC レベル（Content_Types / rels）は `ooxml-core::Package` の責務、SpreadsheetML レベル（sst / styles / calcChain / workbook.xml）は `ooxml-xlsx::Workbook` の責務、と分けるのが自然。
- **不変条件を ADR に明記する: 「既存インデックスは決して変更しない（append-only）」**。これが「他パートをパススルーのまま保てる」ための必要条件であり、2層設計が成立する根拠そのもの。sst の orphan エントリは Excel が許容するので、compaction は行わない。
- Phase0 で sst を生ツリー化しないなら、**編集パスでの新規文字列は `t="inlineStr"`（`<is><t>…</t></is>`）で書く**と ADR に明記する。これは Excel が受け付ける正規の形式で、sst に触れずに済む（Excel は次回保存時に sst へ移す）。そのうえで成功条件に「新規文字列を書いても sst パートが byte 一致で残る」を加えれば、テストが問題を回避しているのではなく、回避策を検証していることになる。
- calcChain: Phase0 では「数式セルを書き換えたら `calcChain.xml` をパッケージから削除し、`workbook.xml.rels` と Content_Types から除去する」（Excel が許容する最小解、openpyxl と同じ）か、「数式セルの上書きは Phase0 では Err を返す」かを決めて書く。

---

## B2 [Blocker] 「write-from-scratch と round-trip が同じ基盤を共有する」は、dedup と性能の2点で現状では看板に近い（重点2）

### 根拠
- 「value-typed Format/shared-strings を Hash+Eq で dedup し、保存直前にインデックスへ解決する」を rust_xlsxwriter から「directly」採用（L98-103）。
- 「"no existing file" is just the degenerate case of `Package` with no original parts … the exact same `Worksheet` lens … is used for generation」（L104-107）。
- 目標成熟度は rust_xlsxwriter / calamine 同級（L11-16）。

### なぜ問題か
**(a) dedup の解決タイミングが lens と両立しない。** rust_xlsxwriter はワークシートが文字列/Format の値を保持し、Packager が保存直前に sst/xf を構築してインデックスを確定する。一方 lens はツリーを「真実」として直接書き換える（L79-82）。ツリー上の `<v>` には何かが入っていなければならない:
- 保存時解決にすると、ツリーには保存前はプレースホルダが入る → 保存前に `ws.cell(1,1).value()` で読むと lens はプレースホルダを返すか、別の side table を引く必要がある。「ツリーが唯一の真実」が崩れる。
- 即時解決にすると、`write_string` の時点で sst へ `&mut` アクセスが要る → `Worksheet` lens と `SharedStringTable` を同時に可変借用することになり、Rust では `Rc<RefCell<…>>` か `Workbook` 経由の API（`wb.write_string(sheet, r, c, s)`）が必要。どちらを取るかで公開 API の形が決まるのに、ADR は触れていない。

**(b) 既存テーブルの seed は from-scratch の dedup とは別物。** 既存 `styles.xml` の `<xf>` を rust_xlsxwriter 型の値 `Format` に逆変換して Hash+Eq に載せるには、`Format` がモデル化していない属性（`xfId`、`quotePrefix`、`pivotButton`、`applyX` 系、テーマ色 + `tint`、font の `scheme`、`extLst`）を落とすか、全部モデル化するしかない。落とすと「異なる2つの既存 xf が同じ `Format` に潰れる」ため、新規書き込みが誤った既存 xf にマージされる（例: テーマ色の xf を RGB 指定の Format が再利用する）。sst も同様で、リッチテキスト `<si><r>…</r></si>` やルビ `<rPh>` を持つエントリはプレーン文字列と同一視してはいけないし、Excel 以外の producer は重複 `<si>` を平気で書く。
→ 実際に必要なのは「from-scratch: 値による dedup」「edit: 既存は生ノードのまま凍結、新規分だけ値 dedup して末尾 append」という**2つの seed モード**。共通化できるのは「append-only レジストリ + 新規分の値 dedup」という骨格だけ。

**(c) 性能。** `RawNode { name: String, attrs: Vec<(String,String)>, children: Vec<…> }`（L74-76）を1セルごとに持つと、`<c r="A1" s="1" t="s"><v>3</v></c>` 1個で String/Vec のヒープ確保が 6〜8 回、概算 200〜300 byte。100万行×10列 = 1000万セルで 2〜3 GB。rust_xlsxwriter は typed struct + 直接ストリーム書き出し（さらに constant_memory モード）、calamine は sheetData を DOM を作らずにストリーム解析する。「同じ lens で生成する」と、両クレートと同級の成熟度という目標（L11-16）と**桁で**衝突する。また ADR は read パス（calamine 相当）をどう実装するかに一切触れていない。

### 改善の方向性
- 「共有するもの / しないもの」を ADR に表で書く。現実的には: 共有 = `Package`（OPC + raw copy + Content_Types/rels）、XML エスケープ writer、append-only インデックスレジストリ。非共有 = 大量セルの表現（from-scratch はカラム指向 or 行バッファの typed 構造、edit は生ツリー）、read の高速パス（ストリーム）。
- 「from-scratch は degenerate case」を本当に成立させたいなら、**`Workbook::new()` = 埋め込みの最小テンプレート xlsx を `Package::open` する**、と定義する案がある。こうすれば from-scratch でも必須の既定スタイル（`fills` の index 0 `none` / index 1 `gray125` は必須）、theme、app.xml が「既存ファイル」として存在し、コードパスが真に1本になる。代償は rust_xlsxwriter のバイト比較テストスイートとは一致しなくなる点。どちらを取るか ADR で決めるべき。
- 生ツリーの大規模シート対策（sheetData だけ遅延/行単位パース、未訪問行は元バイト span のまま保持）を「Phase0 外だが設計上の制約として想定する」と1行書いておく。後から入れるとツリーの型が変わる。

---

## B3 [Blocker] Layer 2 の「構造は保つがバイトは保たない」は、データ構造と記述が矛盾しており、テストの判定基準が無い（重点3）

### 根拠
- L83-84: 「dirty でないノードは **original source bytes verbatim** を出力」。
- L85-89 と L192-194: 「Layer 2 は bytes ではなく structure を保つ」「編集したパート内の触っていない **sibling** ノードは byte-for-byte とは限らない（attribute quoting may normalize）」。
- L74-76: `RawNode { name, attrs, children, dirty }` — ソースのバイト範囲（span）を持つフィールドが無い。
- L187-195 の Consequences 段落に「within touched... no, *any* part …」という書きかけの自己訂正が残っている。

### なぜ問題か
1. **自己矛盾**: 非 dirty ノードを verbatim 出力するなら、触っていない sibling はバイト一致する。normalize され得るのは dirty ノード自身と、その祖先の開始/終了タグだけのはず。ADR は逆のことを言っている。
2. **データ構造が足りない**: 「元バイトをそのまま出す」には各ノードの span（さらに祖先の再出力のためには「開始タグ span / 子の範囲 / 終了タグ span」の3分割）が必要だが、`RawNode` 定義に無い。定義通り実装すると、全ノードを attrs から再レンダリングすることになり、その場合は sibling も normalize される → ADR の Consequences 側の記述が正しくなり、L83-84 が嘘になる。どちらを実装するかで工数もテスト方法も全く違う。
3. **dirty の伝播規則が無い**: `<c>` が dirty のとき `<row>` は? `<sheetData>` は? `<worksheet>` ルートは? ルートの開始タグを再レンダリングすると `xmlns:*` 宣言と `mc:Ignorable="x14ac xr xr2 xr3"` の並びや prefix を正確に再現しないといけない。quick-xml で attrs から再出力して「使われていない prefix 宣言」を整理する類の正規化が入ると、**Excel は mc:Ignorable が未宣言 prefix を指すファイルを開けない**。「attribute quoting may normalize」という軽い表現では済まない。
4. **whitespace の区別が無い**: 「whitespace may normalize」（L85, L129）が属性間空白のことか text node のことか書いていない。`<t xml:space="preserve"> 先頭空白</t>` の text 空白は意味を持つ。text 側を正規化すると文字列値が変わる。
5. **idempotence テストが Layer 2 を全く検証しない**: 「何も編集せず保存」では Layer 1 によって全パートが raw copy されるので、生ツリーはパースすらされないか、パースされても dirty 0 で span 出力されるだけ。L184-186 の「byte comparison of untouched zip entries」は Layer 1 しか測れない。Layer 2 の「構造は保たれる」には判定オラクルが定義されていない。「XML として等価」で比べるなら C14N 相当が必要で、C14N 自体が namespace 宣言の位置や prefix を正規化するため、上記 3. の破損を**見逃す**。
6. **lens が非局所的な編集を要求する**（同一パート内の sibling に波及）:
   - `<dimension ref="A1:C5"/>`: 範囲外に書いたら更新が必要。
   - `<row r="5" spans="1:3">`: 範囲外の列を追加したら stale（ヒントなので致命ではないが、更新方針を決める必要あり）。
   - セル/行の順序: `<c>` は列順、`<row>` は行順でなければならない。find-or-create は正しい位置への挿入が必要（O(n) 走査を避けるならインデックスを持ち、ツリー変更と同期させる必要がある）。
   - **共有数式**: `A1` が `<f t="shared" si="0" ref="A1:A10">B1*2</f>` のマスターで、`A2:A10` が `<f t="shared" si="0"/>`（本文なし）の従属セルのとき、`A1` を数値で上書きして `<f>` を消すと、A2:A10 は存在しないマスターを参照する → Excel 修復。lens は従属セルへマスターを移すか、従属セルを展開する必要がある。配列数式（`t="array"`）、マージセル内のセル書き込みも同種。
   - 読み取りが副作用を持ってはいけない: `cell(r,c)` が「find-or-create」なので、存在しないセルを読んだだけでノードが作られ dirty になり、そのパートがパススルー対象から外れる。

### 改善の方向性
- Phase0 の Layer 2 を「**非 dirty サブツリーは span から verbatim、dirty ノードとその祖先の開始/終了タグだけ再レンダリング**」と定義し直す。そのうえで、祖先タグも元の span のまま出す（子が dirty でも、開始タグ自体の attrs が変わっていなければ開始タグの span を流用する）ようにすれば、変わるのは dirty ノード本体だけになる。これは DomTrip の完全版よりずっと安く、しかも**バイト単位のオラクル**が手に入る:
  - T1: parse → 何もしない → serialize == 入力バイト（Layer 2 単体テスト。Layer 1 を経由させない）。
  - T2: 1セル編集 → serialize し、入力との byte diff が「その `<c>` 要素（+ `<dimension>`）の範囲内」に収まること。
- `RawNode` に span フィールドを入れ、dirty を「self の attrs/name が変わった」と「子リストが変わった」の2ビットに分けて ADR に書く。
- `cell()`（読み取り、`Option` を返す）と `cell_mut()` / `entry()`（作成）を分ける。
- 共有数式・配列数式・マージ範囲内への書き込みは、Phase0 では **検出して `Err` を返す**と明記する（黙って壊すより遥かに良い。pad2py の「正直に TODO/失敗させる」方針と同じ）。
- Consequences L187-195 の書きかけ段落は書き直す。

---

## B4 [Blocker] 依存クレートの MSRV が workspace 設定と矛盾している

### 根拠
- workspace `Cargo.toml`: `edition = "2021"`, `rust-version = "1.75"`, `zip = "9"`。
- ADR Sources L209: zip-9.6.0 `src/write.rs`。
- ローカル registry の `zip-9.6.0/Cargo.toml`: `edition = "2024"`, `rust-version = "1.88"`。実際に `raw_copy_file_rename_internal` 内で let-chain（`if let Some(..) = .. && ..`）を使っている。

### なぜ問題か
宣言した MSRV 1.75 ではビルドできない。CI で MSRV チェックを入れた瞬間に落ちるし、crates.io 公開時の `rust-version` が嘘になる。また `Cargo.lock` がまだ無く、`quick-xml = "0.42"` もローカル registry には 0.41 までしかない（未解決）。

### 改善の方向性
`rust-version` を 1.88 以上に上げる（edition は 2021 のままでもよい）か、zip のメジャーを下げて `raw_copy_file` の有無を確認する。ADR には「raw copy に依存するため MSRV は zip に従う」と1行書く。

---

## M1 [Major] 「bit-identical」は raw_copy_file の実際の挙動より強い主張

### 根拠
L53-57「the result is bit-identical down to the original compression artifacts」、L116-117「byte-exact for untouched parts (verifiable by zip-entry comparison)」。

### なぜ問題か
zip-9.6.0 の `raw_copy_file` は圧縮済みデータ・CRC・サイズ・名前・extra field をコピーするが、**ローカルファイルヘッダと central directory エントリは ZipWriter が書き直す**。特に、非暗号化エントリで元が data descriptor（general purpose bit 3）を使っていた場合、`raw_copy_file_rename_internal` は data descriptor を書かず、ヘッダにサイズを直接書く（data descriptor を残すのは暗号化エントリのみ）。ストリーミング型 producer（.NET `System.IO.Packaging` など）は bit 3 を使うことが多い。また version-made-by、エントリの物理的なオフセット、zip64 extra の有無も変わり得る。つまり「zip エントリをバイト比較」すると失敗し、テスト作者は「テストを緩める」方向に流れがちになる。

### 改善の方向性
保証の単位を「**(名前, 圧縮方式, CRC32, 圧縮済みペイロードのバイト列) が一致**」と厳密に定義し、テストもそれを比較する。zip コンテナ全体のバイト一致は保証しないと明記する。エントリ順序（元の順序を保つか、Packager 固定順にするか、L102-103 の「Packager が write order を固定」と衝突する）も決めておく。

---

## M2 [Major] 「touched」が「書いた」なのか「パースした」なのか未定義

### 根拠
L50-53 では「`set_part` で書いたパート」、L69 では「A part a caller does modify」、L79-82 では lens がツリーを操作する。

### なぜ問題か
lens 経由で `sheet2.xml` を**読んだだけ**の場合、パースはされているが変更はされていない。この場合にどちらで出力するかが決まっていない。ツリーから再出力すると（B3 の span 問題があるので）読んだだけでパートが変わり得る。また lens の変更が `Package` へどう伝わるか（lens が `set_part` を呼ぶのか、`Package` が parsed tree を所有して保存時に dirty を問い合わせるのか）が無い。後者でないと「1セル書くたびにパート全体を再シリアライズして set_part」になる。

### 改善の方向性
`Package` のパート状態を `Raw(bytes)` / `Parsed(tree)` / `Replaced(bytes)` の3状態にし、「保存時、`Parsed` かつ dirty なし → 元の raw エントリをコピー」と規則化して ADR に書く。

---

## M3 [Major] Alternatives の比較から、最も有力な対抗案が抜けている（重点4）

### (a) ooxmlsdk の採用/貢献が検討されていない
Sources L207 で「Rust、パート単位 dirty tracking、Apache POI コーパス 682/682 round-trip」と引用しているのに、Alternatives に「ooxmlsdk の上に構築する / ooxmlsdk に貢献する」が無い。Alternative 4 は rust_xlsxwriter + calamine のラップだけを退けており、まさに ADR が採用する Layer 1 を既に実装・検証済みの Rust クレートを使わない理由が書かれていない。レビュー者が最初に聞く質問はこれ。理由（API の粒度、生成コードの量・コンパイル時間、ライセンス、メンテ状況、パート内 fidelity の方式の違い等）を書くこと。

### (b) Alternative 1 の退け方が実質的に「Layer 1 なしの全面再モデル化」に対する反論になっている
openpyxl の事例（チャート消失・ピボットのテキスト化）の主因は**パート単位パススルーが無いこと**（チャート/ピボットパートをそもそも読まないので書き戻さない）で、パート内の再モデル化ではない。つまり引用された証拠は Layer 1 を支持するが、Layer 2（パート内の生ツリー）の必要性は支持していない。比較すべき中間案「Layer 1 パススルー + 編集対象パートは typed struct に再モデル化し、未知の要素/属性は `Vec<RawXml>` に退避」が検討されていない。この中間案は B2(c) の性能問題が出ず、rust_xlsxwriter 由来のコードをほぼそのまま使える。Layer 2 を選ぶなら「中間案では何が守れないか」（例: 要素の相対順序、`mc:AlternateContent` の位置、`extLst` が sheetData の前後どちらにあるか）を具体的に書く必要がある。

### (c) Alternative 2 の「premature」というコスト論は逆転している
B3 で述べた通り、非 dirty サブツリーを span から verbatim 出力する方式は、全ノードを attrs から再レンダリングして「構造だけ保つ」方式より**実装が単純で、テストオラクルも明確**。full fidelity の残りの差分は「dirty ノード自身の引用符スタイル」程度になる。「バイト一致は高価だから後回し」という前提が成り立っていない。

### (d) Alternative 3 の理由のうち「verification が不可能になる」は弱い
解凍後内容 + CRC32 で比較すれば検証は可能。本当の理由は CPU コストと、M1 で述べた「再圧縮しても保証の単位はどのみちペイロード」という点。さらに raw copy 自体がコンテナのバイト一致を与えないので、「raw copy ならバイト一致で検証できる」という対比は M1 の範囲に限定して書き直すべき。

---

## m1 [Minor] 対象外の入力を明示していない
- パスワード保護（open password）付き xlsx は zip ではなく CFB（OLE 複合ファイル）で暗号化されている。`Package::open` は zip として読めずに失敗する。Phase0 で非対応なら typed error（`Error::EncryptedPackage`）を返すと明記する。
- Strict OOXML（`http://purl.oclc.org/ooxml/...` 名前空間）と Transitional の扱い。lens が Transitional の名前空間 URI しか見ないと、Strict ファイルではセルを「見つからない」と判断し find-or-create で重複行を作る。
- `.xlsm`/`.xltx` の Content-Type 差（`workbook.xml` の Override の値）を保つこと。

## m2 [Minor] Phase0 fixture が自作のみである点の影響範囲
README は正直に「自分の仕様理解で作ったファイルは自分の仮定と矛盾しない」と書いているが、ADR 側の成功条件（L120-123）はこの fixture だけで「pattern proven」と判定する作りになっている。B3 の項目（共有数式、`mc:Ignorable`、`xml:space`、data descriptor 付きエントリ、`spans`）はどれも自作 fixture に入っていない。自作で良いので、最低限これらを含む fixture を追加するか、「Phase0 の proven はこれらを含まない」と成功条件側に書く。

## m3 [Minor] 文書品質
- L187-195 の「within touched... no, *any* part」は推敲漏れ（B3 で既出）。
- Alternative 4 の理由の前半「ユーザーが却下した」は意思決定の経緯であって技術的理由ではない。後半の技術的理由だけで十分成立するので、経緯は Context に移す方が ADR として読みやすい。

---

## 推奨する ADR 修正の優先順位
1. B1: ワークブック単位インデックスレジストリと **append-only 不変条件**を追加。Phase0 の新規文字列は `inlineStr` と明記。
2. B3: `RawNode` に span、dirty 2ビット、伝播規則。T1/T2 のバイトオラクルを成功条件に。共有数式等は `Err`。
3. B2: 共有/非共有の表。`Workbook::new()` をテンプレート open にするか否かを決める。
4. B4: MSRV 修正。
5. M1-M3: 保証単位の厳密化、パート状態の3値化、ooxmlsdk と中間案を Alternatives に追加。
