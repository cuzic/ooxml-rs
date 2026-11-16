# ADR-0007 レビュー（Opus, round 1）

対象: PR #25 `plan/docx-phase1`（`3f702aa`）、`docs/adr/0007-docx-phase1-template-filling.md`（Proposed, rev 1）、
`spikes/docx-phase1/`、`docs/roadmap/docx.md`。参照: ADR-0004 rev 4（`impl/docx-phase0`）、PR #18
（`core/detach-insert`）。プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr25/`
（`probe1.py`〜`probe4.py`、Phase0 クレートに対する Rust プローブ `probe/`、スパイクの再実行結果 `rerun/`、
allowlist ドライラン `dry.log`）。コーパスは作者がダウンロードしたものを `p1common.Pkg` 経由で読んだだけで、
エンティティの解決はしていない。

## 結論（先に）

- **骨格は健全。** 「Phase0 の `set_run_text` を組み合わせる」「run グループ／透明／アンカー／境界の 4 分類」
  「最初の一致文字の書式」「既定は全件成功か全件不変（skip は opt-in で報告する）」という方針は正しい。
  ADR が数字を出している比較対象の各ライブラリより厳密でもある。スパイクの数値は再現した
  （`replace_sim` の tpl/poi/fx、`stories_cells` の tpl/poi は JSON が一致。違いは `--examples` の有無だけ）。
  allowlist も 66 エントリで、PR #18 のツリー上でドライランが通り、`ooxml-schema --features wml` もコンパイルできた。
- **ただし Major が 4 件ある。**
  - M1: `replace_at` が古い `Match` を受け付ける。
  - M2: 原子性の主張が成り立たない。コーパスにすでに実例がある。
  - M3: 置換が一致範囲の**外**の run 内容まで正規化する。
  - M4: style 列が `SEQ \s` を許してしまい、健全でない。

  どれも修正は小さく、設計の作り直しは要らない。
- **IgnoreInvisible の既定値**は維持してよい。ただし「3 つとも不可視」という根拠は、`w:hint` については
  偽になる場合がある（m1）。小さな絞り込みを入れれば根拠どおりになる。
- **判定: Minor だけ直せば Accepted、とは言えない（no）。** M1〜M4 を反映した rev 2 を round 2 で確認したい。

## Findings

### Blocker

なし。

### Major

#### M1. `replace_at(&Match)` が古い `Match` を検出できず、黙って別の範囲を置換する

- **問題:** `Match { para, range, status }` は `Paragraph::text()` のバイト範囲しか持たない（§7）。
  `find` → `replace_at(m1, 長い値)` → `replace_at(m2, …)` と呼ぶと、同じ段落の `m2.range` はずれる。
  さらに、`replace` や `set_paragraph_text` の後に古い `Match` を使うと、別の文字列を置換してしまう。
  エラーにならず、黙って内容が変わる。
- **根拠:** §7 の API 案。§1.2 は「インデックスは不変」と書くが、それは run の番号の話で、テキストのオフセットは変わる。
- **推奨:** `Match` に一致した文字列（または長さ＋ハッシュ）と、`Document` の編集世代カウンタを持たせる。
  `replace_at` は、世代が違うか `text()[range]` が needle と一致しなければ `Err(StaleMatch)` にする。
  別案は `replace_at` を外して `replace` に `Scope::Paragraph` を足すこと。T4 に「`find` → 別の編集 →
  `replace_at`」のケースを入れる。

#### M2. 「チェックの後は書き込みが失敗しない」（§0, §7）は誤り。`UnresolvedSibling` が途中で起きる

- **問題:** §7 は、事前チェック後に失敗しうるのは `TooDeep` と `NamespaceNotInScope` だけで、それは
  `check_insert_under` と深さで事前に確かめる、と書く。しかし core の `LensMut::place()`
  （`crates/ooxml-core/src/lens.rs:600-631`、PR #18 ツリー）は、親の**既存の子**が slot 順になっていないか
  `mc:AlternateContent` があれば、`insert` のたびに `UnresolvedSibling` を返す。
  この配置判定は非公開で、事前に確かめる公開 API がない。
  - `get_or_insert` は既存の要素を見つけたときは配置を通らないので成功する。
  - このため `format_paragraph(p, { alignment: Set(Center), keep_next: Set(true) })` を、`jc` はあるが
    `keepNext` はない `w:pPr` に対して呼ぶと、`jc` を書き換えた後で `keepNext` の挿入が失敗し、
    一部だけ書き換わった状態が残る。`format_paragraph_runs`（複数 run）も同じ。
- **根拠:** プローブ `probe3.py`。tpl の書き込み可能ファイルに、`w:pPr` の子が SDK の slot 順でない段落が
  9 個ある（docxtemplater の `expected-multi-section.docx` など。`['sectPr', 'pStyle', 'bidi', 'jc', 'rPr']`）。
  §4 が引く「ADR-0002 の調査で 0 件」は POI だけの数字。
  - 一方、run のテキスト書き込み（Phase0 の `write_pieces`）は、内容を先に全部消すので run 内の順序の乱れの
    影響を受けない。プローブ `probe/` で `<w:r><w:t>ab</w:t><w:rPr><w:b/></w:rPr></w:r>` に
    `"x\ty"` を書くと成功した。つまり問題は §4/§5 の書式とスタイルの書き込みに限られる。
- **推奨:** 次のどちらかにする。
  - (a) ドメイン側で、書き込み対象になる `w:rPr`/`w:pPr`/`w:settings`/`w:p` 全部について、生成された slot 表で
    子の順序と AC の有無を事前に確かめ、だめなら書き込み前に `Core(UnresolvedSibling)` を返す。
  - (b) core に配置の dry-run（例: `LensMut::check_insert::<C>()`）を加算的に足す。

  あわせて、§7 の「失敗しうるもの」の列挙に `UnresolvedSibling` と `AlreadyPresent` を加える。T4 に上の tpl
  パターンの単体文書を入れる。

#### M3. 置換が `r₀`/`rₖ` の一致範囲**外**の内容を正規化する（T7 では検出できない）

- **問題:** §1.2 は、変更する run を「一致前のテキスト＋置換文字列＋一致後のテキスト」で丸ごと
  `set_run_text` し直す。Phase0 の写像（ADR-0004 §3.2 step 1/5）では、次の要素が文字列化され、元に戻らない:
  - `w:noBreakHyphen` → `w:t` 内のリテラル U+2011
  - `w:softHyphen` → リテラル U+00AD
  - `w:cr` → `w:br`

  Phase0 では呼び出し側が run 全体を指定したので受け入れられていた（ADR-0004 で owner 了承済み）。Phase1 では、
  呼び出し側が触れていない文字のマークアップが変わる。リテラル U+2011 は、フォントに字形がなければ別フォントで
  描かれる。U+00AD を Word がどう読むかも未確認 *(u)*。
  T7 の (i)(ii) は `Paragraph::text()` と文字ごとの `rPr` で比べるので、両者を同じ文字として扱い、この変化を検出しない。
- **根拠:** Phase0 クレートでのプローブ（`probe/`）で、`<w:t>ab</w:t><w:noBreakHyphen/><w:t>c</w:t>` が
  `<w:t>ab‑c</w:t>` になった。`probe1.py` では、受理された cross-run のボーダー needle のうち、`r₀`/`rₖ` が
  これらの要素を含むものが POI 12、SDK 26。単一 run の一致も含めればさらに多い。
- **推奨:** 次のどちらかにする。
  - (a) 変更する run の中で、一致した文字を含む `w:t`/`w:tab`/`w:br` だけを書き換え、ほかの内容の子要素は
    そのまま残す（子要素単位の局所編集）。
  - (b) 一致範囲の外に `w:cr`/`w:noBreakHyphen`/`w:softHyphen` を持つ run を変更する場合は
    `Err(UnsupportedRunContent)` で拒否する。

  Phase1 には (b) のほうが小さい。どちらを採っても、T7 に「一致範囲外の内容の子要素のバイト列が同一」という
  オラクル (iv) を足す。

#### M4. §6.1 の **style** 列は `SEQ` を許すが、`SEQ \s` は見出しレベルに依存する

- **問題:** `SEQ id \s n` は「レベル n の見出しの後で番号を振り直す」フィールド。
  `set_paragraph_style` で見出しスタイル（アウトラインレベル）を付けたり外したりすると、結果が古くなる。
  ADR の理由付け（「フィールドや段落の数と順序に依存」）は `\s` を見落としている。
  ブックマーク名付きの `SEQ`（「ブックマークに最も近い SEQ の結果を表示する」）は、ブックマークが動かないので問題ない。
- **根拠:**
  - [MS-OI29500 §17.16.5.56 SEQ](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/a591c060-2800-4699-b1f4-dbb0c4374557):
    "In Word, the \s switch resets the sequence following a heading of the specified level."
  - [Microsoft Support: SEQ field](https://support.microsoft.com/en-us/word/field-codes-seq-sequence-field)
  - コーパスにも実例がある: SDK `complex2005_12rtm.docx` に `SEQ Figure \* ARABIC \s 1` が 20 個
    （`TOC` もあるので結局拒否されるが、パターンは実在する）。
- **推奨:** style 列は `SEQ` を `\s` が付いていないときだけ許す（スイッチを解析する）。または style 列から
  `SEQ` を外す。style 列で救えるのはコーパスで 1 ファイルだけなので、style 列そのものを Phase1 から外すのも妥当
  （Q3 参照）。text/append/format 列は `\s` があっても健全（見出しを変えないため）。

### Minor

- **m1. `w:hint` を不可視とする前提は、一致に非 ASCII かつ非 CJK の文字があると成り立たない。**
  §1.3 の根拠は「needle が作者に一様に見えていたか」。ところが `hint="eastAsia"` は、両方の文字範囲に属する
  ’ … – などのフォントスロットを変える。
  - 根拠: `probe2.py`。SDK で hint の違いだけで受理されたボーダー needle 3,011 のうち 95 は、hint が `r₀` と
    異なる run に、そうした文字を含んでいた（例 `can’t`、`1，2，…`、`IC – L`）。tpl では 14 のうち 3
    （U+00A0）。
  - `w:lang/@w:val` は自動ハイフネーション（`autoHyphenation` が有効なとき）の辞書を選ぶので、
    「描画を変えない」も言い過ぎ。
  - 推奨: hint の違いを無視するのは、hint が異なる run に属する一致文字がすべて ASCII か CJK 範囲のときだけにする。
    そうすれば `{{…}}` のようなプレースホルダには影響しない。`lang/@w:val` の説明は「校正とハイフネーション」に直す。
- **m2. T7 の参照分類器は独立ではなく、スパイクも仕様からずれている。**
  §9 は「`replace_sim.py` の `Stream` の移植」と「fixture ごとの件数がスパイクと一致すること」を求める。
  これは同じ作者の実装を写すことになり、独立したオラクルにならない。さらにスパイクは ADR と次の点で食い違う。
  - `Stream` が `w:del`/`w:moveFrom` の run のテキストを含める（ADR-0004 の `text()` は除外）。POI 3、SDK 3、d4j 1 run。
  - `_rpr_norm` が子要素をソートしてから比べる（§1.3 の正準形は子の順序を無視しない）。
  - `w:ruby` を run 内容として扱い、`rubyBase` に降りない（ADR-0004 §3.1 は降りる）。
  - xml:space の集計が `refusal(True)`（smartTag を settable とみなす）を使っている。docstring の「§1.6」も古い。

  推奨: 参照分類器は §1.1 の表から**別の担当者（またはエージェント）が書く**。スパイクの件数は参考値にとどめ、
  一致は条件にしない。スパイクを使い続けるなら上の 4 点を直す。
- **m3. 上書きする属性の除去は同じ要素の中だけで、継承値を見ていない。**
  §4 の「上書きする属性を除く」は、同じ要素の中については正しい。
  - `w:color/@themeColor`、`rFonts/@*Theme`、`ind/@*Chars`、`spacing/@*Lines`、`@*Autospacing` は、
    それぞれ ECMA の記述どおり明示値より優先される。
  - ただし、スタイルや docDefaults から継承した `firstLineChars`/`beforeLines` が直接書式の twips に勝つかは
    確かめていない *(u)*。
  - 根拠: SDK では、Word が書いた直接書式 `firstLineChars="0"`（継承値を打ち消す形）が 80 件あり、
    スタイル側が `*Chars`・直接書式が twips だけの段落が 4 件ある（`probe4.py`）。
  - 推奨: manual_check に「スタイル側の `firstLineChars` ＋ `first_line` の Set」を加える。結果によっては、
    Word と同じように `*Chars="0"` を書くかどうか決める。
  - 合わせて、cs スロットのテーマ属性名は **`w:cstheme`**（小文字の t。SDK の `CT_Fonts`）であることを、表に
    名前で書く。
- **m4. `w:fitText/@w:id` もコピーされる。**
  §8.2 の「ID を持つ要素はスキップ後に残らない」は誤り。SDK の `CT_FitText` は `w:val`, `w:id` を持つ。
  `eastAsianLayout` を拒否した理由（グループに加わる）と同じ理由で、`fitText` も `UnsupportedMarkProperties`
  にするか、コピーは Word の入力時の挙動と同じだと明記する。コーパスに該当例はない。
- **m5. §8.2 の生成ルールは書いてあるとおりに実装すると成立しない。**
  「`from` から skip を除いた lowering が `to` と等しい」とあるが、`w:rPrChange` は**両方**の型にある
  （QName は同じで型が違う）。`from` 側だけから除くと slot 数が合わない（50 対 51）。
  「両側から skip を除いて比べ、`to` 側で除いた要素はコピー結果にも現れない」と書き直す。
  `oMath` は SDK では `CT_ParaRPr` にしかない。README の「57 slots」と、SDK の子要素 58 の差も説明しておく。
- **m6. `set_paragraph_text` と `set_cell_text` の仕様に穴がある。**
  - 書式ルールの指定がない（`ReplaceOptions::formatting` を受け取るか、既定値を明記する）。
  - 「run がない段落」の判定を、直下の `w:r` で行うのか、フロー全体で行うのか。
    `w:hyperlink`、`m:oMath`、子のない `w:fldSimple` だけの段落は `ParagraphNotSimple` になるのか。
  - run がない段落に `""` を書いたら、空の run を作らない no-op にするのか。
  - スペーサー用セルの `sz=2` のようなマーク書式もコピーされることを、Consequences に書く。
- **m7. `CellRef.row` が行レベルの `w:sdt`/`w:customXml` の中の行をどう番号付けするか未定義。**
  表示上の行とずれる。`gridBefore` は `w:trPr` にあり allowlist にないが、生の木を読むなら「raw」と明記する。
- **m8. `Story::sections()` は「使われる」と「表示される」を区別すべき。**
  `w:titlePg` がない節の `first` ヘッダー、`settings/w:evenAndOddHeaders` がないときの `even` ヘッダーは
  表示されない。表示されるかどうかも返すとよい。
- **m9. `updateFields` の往復保存。**
  [MS-OI29500 §17.15.1.90](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/d8bc14c0-5eeb-49c1-9ba7-8edf4c33976f)
  に "Word does not roundtrip the updateFields element in all circumstances" とある。
  Q5 の manual check に「更新して保存した後も要素が残るか」と「Word for Mac / Word Online の挙動」を加える。
  プロンプトの文言は、ネット上の一次資料では確認できなかった。
- **m10. 細かい点。**
  - `find` の `scope` 引数と `ReplaceOptions::scope` が重複している。
  - `style_ids_by_name` の大文字小文字の比較方法（ASCII か Unicode の単純ケースフォールディングか）を明記する。
  - 重複 ID を `UnknownStyle` で返すのは名前と合わない（`AmbiguousStyle` を推奨）。
  - §6.3 の「POI は `on` を書く」は出典の行を再確認する。

### 確認して問題なしとした点

- **グループ化。**
  - 縮退ブックマークを透明にする規則と、端にあるアンカーの内外の扱い（始点 s のマーカーは置換の前、
    終点 e のマーカーは後）は、`r₀` に値を入れる方式と一致している。
  - `proofErr` を残しても害はない。
  - パラグラフ直下の要素の分類は、catch-all を含めて `EG_PContent` 全体を覆っている。
  - run の中身は R4 の catch-all で覆われている（`w:sym`、`w:footnoteReference`、`w:ptab`、AC、ruby、
    改ページ用の `br` はすべて境界）。
- **差分の局所性。** 空になった run を残す方式は `CT_R` に照らして妥当で、インデックスも安定する。
- **既定の「全件成功か全件不変」と、opt-in で報告付きの skip** の分け方は妥当。
- **ストーリー。**
  - セパレータ注の除外、注・コメントのマーク run の扱い、cross-part ID を不変に保つ点、ヘッダーの継承の記述
    （§17.10.5）は妥当。
  - W1/W2/W3 を文書全体に、R1 のフィールド深さをストーリーごとに数えるのも正しい。
- **セル。** `vMerge` で `val` がないものを continue とみなすのは、仕様の既定値どおり。
- **マーク書式のコピー。** 型の差は `ins`/`del`/`moveFrom`/`moveTo`/`w14:conflict*`/`oMath`/`rPrChange` の型
  だけで、共通の子の順序も同じ（SDK JSON で確認）。
- **`w:color` の theme 属性**、**`rFonts` の `*Theme`**、**`ind`/`spacing` の上書き属性**を同じ要素内で除去するのは、
  ECMA の記述どおり。`b`/`bCs` を独立に扱う点も妥当。
- **スタイル。** `w:name` の英語名で引くことと、大文字小文字を無視すること（組み込みは `heading 1` のように小文字）は妥当。
- **allowlist。** write 29 ＋ read 37 ＝ 66 で重複なし。ドライランで全エントリが診断なしで lowering でき、コンパイルも通った（`dry.log`）。

## 推奨（Q1–Q10）

- **Q1（既定の書式ルール）:** `IgnoreInvisible` を既定のまま採用する。ただし m1 の絞り込み（hint の違いは、
  ASCII か CJK の文字にしか関わらないときだけ無視）を入れる。`Identical` を既定にすると、SDK の 22 ポイントを
  失うのに、プレースホルダの安全性は上がらない。
- **Q2（端の空白）:** manual_check の結果が出るまで `UnpreservedWhitespace` を維持する。影響は tpl の 3/517 で小さい。
- **Q3（操作ごとの W2）:**
  - text/append/format の列は出荷してよい（小さく、すべて列挙でき、テストもしやすい）。
  - style 列は M4 を直すか、Phase1 では外す（救えるのは 1 ファイル）。私は「外す」を推す。
- **Q4（`IF`）:** 延期を推す。コーパスで通る例は 0 件で、新しいトークナイザの攻撃面と保守コストに見合わない。
  Q6 で実際の差し込み印刷テンプレートが手に入ってから入れればよい。
- **Q5（`updateFields`）:** owner による確認項目に m9 の 2 点を加える。結果が出るまでも、opt-in の設計（すでに on
  でも明示が必要）はそのままでよい。
- **Q6（テンプレート fixture）:**
  - docxtpl（LGPL-2.1）と、ライセンスが不明な docxtemplater の examples は同梱しない。
  - owner が作る Word 365 テンプレート 3〜5 本（日本語、ヘッダー、表、脚注、分割された `{{…}}`、`_GoBack` を含む）
    を推す。それまでは手作りの単体文書で代用する。
- **Q7（`roxmltree` を dev-dependency に）:** 賛成。docx-rs と同じく正確なバージョンで固定する。
  ただし T7 の独立性は m2 の運用で担保する。
- **Q8（§8.2 の生成ルール）:** 賛成。ただし m5 の文言修正と、m4 の `fitText` の扱いを決めてから入れる。
- **Q9（`jc` は `left`/`right`）:** 賛成。初版の `ST_Jc` との互換性があり、コーパスでも多い。
- **Q10（ADR の範囲外）:**
  - `Document::new()` は独立した ADR にする。
  - 強制保護の opt-in と A2（最後の `sectPr` の移動）は、Phase2 の構造編集 ADR にまとめる。どちらもコーパスでの
    需要はほぼない。

## 判定

**Accepted after Minor only: no.**
M1（古い `Match`）、M2（原子性）、M3（一致範囲外の正規化）、M4（`SEQ \s`）を rev 2 で反映し、round 2 で確認したい。
Minor は rev 2 で一緒に直すか、理由を付けて見送ればよい。
