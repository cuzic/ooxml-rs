# ADR-0011 レビュー（Opus, round 1）

- 対象: PR #31 `docs/adr/0011-docx-phase2-structure.md`（Proposed, rev 1）、`spikes/docx-phase2/`、`docs/roadmap/docx.md` の差分
- 照合先: ADR-0001/0002（post-acceptance notes 含む）、ADR-0004（main, Accepted）、ADR-0007 draft（`plan/docx-phase1`）、ADR-0009（`plan/xlsx-phase3`）、ADR-0012（`plan/pptx-phase2`）、`core/add-part` の WIP（a153161、`crates/ooxml-core/src/package/add.rs`）、PR #14 `remove_part`
- 追加プローブ: `scratchpad/review-pr31/probe.py`（zipfile + ElementTree、`python3 -I`、実体参照なし、解析のみ）。著者と同じコーパス fx/POI/SDK/d4j/tpl に対して実行。以下の件数はこのプローブで数えたもの

## 結論（先に）

レジストリ設計の土台（append-only、AC の両分岐を含む max + 1、既存の重複は修復しない、`numId` 0 は予約、`w14:paraId` は見送り）は妥当で、コーパスによる根拠も十分にある。ただし、**このままでは Accepted にできない**。

1. **§1 の core API が、実装中の `core/add-part`、ADR-0009、ADR-0012 のいずれとも食い違う**（B1）。API の形（`PartPlan`/`commit` と `check_*` twin）、メディアの `Default`、パーツ名の採番規則、dangling target の扱いの 4 点で、3 本の ADR と実装が 3〜4 通りに割れている。
2. **仕様どおりに実装すると Word の出力が壊れるか、意図と違う結果になる規則が 5 つある**。
   - セクション区切りの `w:type` を書き込む `sectPr` が逆（M1）
   - `a:graphicFrameLocks` で名前空間 `a` が未宣言になる（M2）
   - 新しいスタイル ID が、既存の dangling 参照を拾ってしまう（M3）
   - 非 ASCII のスタイル名から空の ID が生成される（M4）
   - 列挿入で `gridSpan`/`vMerge` まで複製され、グリッドが崩れる（M5）
3. **W2 の structure 列が、行削除で古くなるフィールドを許してしまう**（M6）。

どれも直し方ははっきりしていて、修正量は小さい。

## Findings

### B1. §1 の core API が `core/add-part`・ADR-0009・ADR-0012 と矛盾する（cross-ADR API conflict）

根拠（4 点）:

| 論点 | ADR-0011 §1 | `core/add-part`（a153161） | ADR-0009 §2 | ADR-0012 §2 |
|---|---|---|---|---|
| API の形 | `Package::plan(&mut self) -> PartPlan<'_>`、`add_part`/`add_relationship` を積んで `commit` | 即時の `add_part`/`add_related_part`/`add_relationship`/`set_content_type`/`next_free_part_name`。各操作に `&self` の `check_*` twin がある（`add.rs:584-760`） | `check_*` twin を使う | `check_*` + mutating call、validation は 1 つ（C1–C6） |
| メディアの content type | 新しい拡張子には **`Default` を追加**（§1.2）。`.rels` 用の `rels` Default も追加 | `Default` を追加する API は**ない**。一致する Default がなければ Override（`plan_new_name`, `add.rs:344-374`）。新しい `.rels` パーツにも Override | **「`Default` は決して追加も変更もしない」**（§2、xlsx の `bin` Default 衝突 110 件が理由） | 別の操作 C2 `add_default_content_type`。条件は「その拡張子のパーツで Override を持たないものが 1 つもない」こと |
| パーツ名の採番 | `Numbered{dir,stem,ext}`: **最小の空き n ≥ 1**（ギャップを再利用する） | `next_free_part_name(pattern, start)`: start 以上で最初の空き | `sheetId` から始めて空くまで増やす | C5: **既存の最大 index + 1**。ギャップは再利用しない |
| 「空き」の定義 | パーツがなく、Override がなく、**dangling を含む relationship target がない** | パーツと Override だけを見る。**relationship target は見ない**（`add.rs:740-750`） | 同等パーツなし | 同等パーツなし。セッション内で削除した名前は再利用しない |

その他の差:

- rId の最大値の拾い方: ADR-0011 は「`rId\d+` を大文字小文字無視で照合」とする。core は `strip_prefix("rId")` で大文字小文字を区別する（`next_relationship_id`）。たとえば `RID9` は core では max の対象にならない。衝突は起きないが、仕様の文言と一致しない。
- `PartPlan<'_>` は `&mut Package` を借用したままになる。そのため docx crate は plan を保持している間、Layer 2 の check（`package.xml_part(..)`）を同じ `Package` に対して実行できない。§1.1 の「plan → Layer 2 check → commit」の順序は、借用規則のせいでそのままでは書けない。core の `&self` twin 方式にはこの問題がない。
- dangling target は実在する。SDK `TestFiles__May_12_04.docx` には、存在しないパーツを指す internal target が 9 本ある（`word/#_THIS_WEEK_IN`）。ADR-0011 の条件のほうが安全側だが、core に実装されていない。

**影響:** ADR-0011 を Accepted にすると、ADR-0009/0012 と core 実装に反する規範的な core API が 1 つ増える。owner の Q1 は「PartPlan が xlsx にも合うか」と問うだけで、すでに 3 通りに割れている事実が書かれていない。

**修正案:**

- §1.1 を、`core/add-part` の形（即時操作 + `&self` の `check_*` twin + 1 ステップで part と relationship を作る `add_related_part`）に合わせて書き直す。
- Word Phase2 の各操作で新しく作るパーツは最大 1 つ（media / numbering / styles）で、twin で足りることを明記する。複数パーツを追加する操作（`set_header` + 画像など）が要るなら、そのときに pending-names 付きの planner を core 側の ADR で追加する。
- メディアの content type は次のどちらかに統一する。
  - (a) Override のみ。core が今すでにそう動き、ADR-0009 とも一致する。Word も Override 型のメディアを読む（SDK 2、tpl 13 ファイルの実例）。
  - (b) ADR-0012 の C2（安全条件付き）を共通 API にする。
  - いずれにしても「`add_part` が暗黙に Default を足す」案はやめる。
- パーツ名は 1 つの規則に統一し、ADR-0009/0012 の owner と合意する。
  - 推奨は「max + 1、かつ dangling target・Override・セッション内で削除した名前を避ける」。ADR-0011 の「最小の空き」は、削除後の stale な参照を拾う危険があり、§Alternatives 2 で rId に適用している理由と矛盾する。
  - dangling target の除外は core の `next_free_part_name`/`add_part` に入れる。

### M1. セクション区切り: `w:type` を書く `sectPr` が逆。`pgNumType/@start` も重複する

根拠:

- `w:type` は「このセクションが**直前のセクションに対して**どう始まるか」を表す（ECMA-376 Part 1 §17.6.22; [docx4j ECMA mirror](https://webapp.docx4java.org/OnlineDemo/ecma376/WordML/type_1.html)）。
- §6 は、S を段落 p で分割するとき「p の `pPr` に S の `sectPr` のコピーを置き、**コピーに `w:type` だけを設定**」としている。コピーが受け持つのは**前半**（p で終わるセクション）なので、指定した kind は前半の開始方法になり、2 つの半分の境界には効かない。
  - 例: S が nextPage（既定）で、`continuous` の区切りを入れた場合。後半は元の sectPr、つまり nextPage のままなので、境界は改ページになる。
- python-docx も、kind を**後ろ側（元の sentinel）**に書いている（`docx/document.py:146-147` の `new_sectPr = add_section_break(); new_sectPr.start_type = start_type`、`oxml/document.py:63-71`）。§13 はこれを引用しているのに、本文の規則は逆になっている。
- 同じ取り違えで `w:pgNumType/@w:start`（ページ番号の振り直し）が前半と後半の両方に残り、後半でもページ番号がリセットされる。該当する `sectPr`: POI 6、SDK 23、tpl 11（プローブ）。`lnNumType` の restart（SDK 3）、`footnotePr`/`endnotePr` の `numRestart` も同じ。

**修正案:**

- コピー（前半）は S の元の `type` と `pgNumType/@start` を保持する。
- 元の `sectPr`（後半）には `w:type=kind` を書き、`pgNumType/@start` を外す。後者は Word の挙動 *(u)* を manual check で確定させる。
- この場合、元の `sectPr` も属性を書き換えることになる。そのため `sectPrChange` の拒否は元の `sectPr` にもかかることを明記する。

### M2. `a:graphicFrameLocks` で名前空間 `a` が未宣言になる

根拠:

- §3 は「`a:graphic` が `a` を宣言」としている。しかし `wp:cNvGraphicFramePr/a:graphicFrameLocks` は `a:graphic` より**前の兄弟の子**なので、`a:graphic` での宣言はスコープ外になる。
- Word は `a:graphicFrameLocks` 自身にも `xmlns:a` を書く。プローブでは、ローカル宣言ありが POI 60/60、SDK 297/298、fx 34/34、d4j 6/6、tpl 40/49 だった（宣言なしの 9 件はルートで `a` を宣言している tpl の出力）。
- main パーツのルートで `a` を宣言しているファイルはほとんどない（著者の測定で POI 1/101、SDK 1/404）。したがって、仕様どおりの XML は名前空間整形式でなくなる。core のプレフィックス規則（ADR-0002 §3）がこれを検出して拒否するなら、今度はほぼ全ファイルで `insert_image` が失敗する。

**修正案:**

- `declare_namespace(a)` を、作成する `a:graphicFrameLocks` にも適用する。
- 一般則としては「作成ノードで、スコープにない `a`/`pic` を使う最も外側の要素ごとに宣言する」と書く。
- 同じ仕組みで `wp` もローカル宣言にすれば、`NamespaceNotInScope`（POI 5、SDK 6 ファイル）は不要になる。`r` がルートでバインドされていない story（自作ヘッダー等）も同様に扱う。

### M3. 「fresh」が定義の集合しか見ておらず、dangling 参照を新しい値が拾う

根拠:

- §2 の Style ids の走査範囲は `styles.xml` の `w:styleId` だけになっている。しかし POI `60329.docx` には、未定義のスタイル ID `EmptyCellLayoutStyle` を参照する `w:pStyle` が 22 個ある（プローブ。ADR-0007 §5 も同じ 22 件を記録している）。
- 呼び出し側が `add_style(name: "EmptyCellLayoutStyle")` などを実行すると、導出された ID が一致し、**既存の 22 段落の見た目が黙って変わる**。これは Layer 3 の「既存の意味を変えない」に反する。
- 同じ構造の問題がほかの ID 空間にもある。

  | ID 空間 | 規則が見ている範囲 | 抜けている参照 |
  |---|---|---|
  | rId（§1.2） | `.rels` の Id の max | story 内の `r:*` 属性 |
  | `numId` | `numbering.xml` の `w:num` | story と styles の `w:numPr/w:numId` |

  rId と numId については、今回のコーパスで dangling 参照は 0 件だった（rId: `r:*` 属性 2,805 個のうち dangling 0。numId: 0）。それでも §Alternatives 2 が rId のギャップ再利用を退けた理由（「stale な参照が残っているかもしれない」）は、max より上にも同じように当てはまる。numbering パーツがない文書で dangling な `numId=1` があれば、最初の `create_list` がそれを拾う。

**修正案:** 各レジストリの走査範囲を「定義 ∪ 参照」にする。

| ID 空間 | 走査範囲に加える参照 |
|---|---|
| styleId | 全 story と `styles.xml` の `pStyle`/`rStyle`/`tblStyle`、`basedOn`/`next`/`link`、`numbering.xml` の `w:lvl/w:pStyle`、`styleLink`/`numStyleLink` |
| numId | 全 story と styles の `numPr/numId` |
| rId | ソースパーツ内で `r` 名前空間にある属性値（PR #14 M1 の any-attribute 規則と同じ） |

rId の部分は core の `next_relationship_id` に関わるので、B1 の統一と一緒に決める。T-REG の「absent from its whole scope」も参照を含むように書き直す。

### M4. 非 ASCII のスタイル名から空のスタイル ID が生成される

根拠:

- §8 は「`[A-Za-z0-9]` 以外をすべて除去」とする。この規則では、名前全体がキリル文字・CJK の場合に ID が空文字列になる。プローブでは、除去すると空になる `w:name` が POI 89、tpl 30、fx 7 件あった（`Основной текст Знак`、`页眉 字符` など）。
- §8 の衝突規則は「既存 ID との衝突」のときに数字の接尾辞を付けるだけで、空の場合は未定義になっている。
- 実際の Word（ロシア語版・日本語版）は、非 ASCII の名前に `a`, `a0`, `a3`, `a8`, `aa` … を割り当てている。`a`/`aN` 形の styleId は POI 104、SDK 104、fx 34 件あった。`Überschrift 1 Zchn` → `berschrift1Zchn`（POI `Styles.docx`）の例は、ASCII 以外を除去するという仮説を裏付けている。

**修正案:**

- ASCII 英数字への除去は維持する（Q4 への回答を参照）。
- 結果が空のとき、または数字で始まるときは、Word と同じ形で `a` + 最小の空き接尾辞にする。
- 衝突判定は M3 の範囲で行う。

### M5. `insert_column`: 左隣の `tcPr` をコピーすると `gridSpan`/`vMerge`/`hMerge` まで複製される

根拠:

- §7.3 は「新しいセルは左隣の `tcPr` のコピーで、`tcW` を幅に設定」とする。左隣のセルが `gridSpan` > 1 なら、新しいセルも同じ span を持つ。その結果、その行のグリッド幅が `gridCol` の数を超え、§7 自身の前提である一貫したグリッドが崩れる。
- `gridSpan` > 1 のセルは POI 7,534、SDK 1,658 個ある。左隣の `vMerge` をコピーすると、新しい列に意図しない縦結合ができる。
- `at_grid_col = 0` の行には左隣がない。行頭は `gridBefore` > 0 の場合を除いて、cell boundary 扱いになる。

**修正案:**

- コピーからは `gridSpan`、`vMerge`、`hMerge`、`tcPrChange`、`cellIns`/`cellDel`/`cellMerge` を除く。
- 列 0 では右隣のセルを複製元にする。
- `hMerge`（旧形式の横結合）を含む表は `UnsupportedContext` で拒否する。
- `delete_column` で行にセルが 1 つも残らなくなる場合は拒否する（Word は `w:tc` のない `w:tr` を壊れた表として扱う *(u)*）。

### M6. W2 の structure 列が `NOTEREF` を許すが、`delete_rows` は脚注参照を消せる。変更履歴を含む行も黙って消える

根拠:

- §10 の structure 列は `TC`/`XE`/`NOTEREF` を許可している。§7.2 の `delete_rows` は、行の外にパートナーがあるアンカーやフィールドを拒否するが、**行の中で完結する `w:footnoteReference`/`w:endnoteReference`** は拒否しない。脚注参照は番号付きなので、これを消すと後続の脚注番号が変わり、`NOTEREF` の結果が古くなる。さらに `footnotes.xml` に孤立した注が残る。
  - 脚注参照を含む表の行: POI 3、SDK 7 ファイル。
  - 行の中で完結する `SEQ` フィールドを消した場合も、後続の `SEQ` の結果が古くなる。ただし `SEQ` は structure 列にないので、これは問題にならない。
- `delete_rows`、`merge_cells`、`delete_column` は、run レベルの `w:ins`/`w:del`/`w:moveFrom`/`w:moveTo` や、段落記号の `w:pPr/w:rPr/w:ins` を含む行やセルを、記録を残さずに削除する。これは変更履歴を黙って確定させるのと同じことになる。
  - 該当する行: SDK 3 ファイル（`Complex01.docx` ほか）。
  - W1 は `trackRevisions` がオンのときだけ働くので、ここは素通りする。
- §7.1 の `insert_rows` も、テンプレートセルの `pPr` から `pPrChange` は除くが、`pPr/rPr` 内の `ins`/`del` は除いていない。

**修正案:**

- 削除系の操作（`delete_rows`/`delete_column`/`merge_cells`）は、範囲内に脚注・文末脚注の参照、または revision 要素があれば拒否する（`InsideRevision` / 新しく `WouldRemoveNote`）。
- `insert_rows` の `pPr` コピーからは `rPr/ins|del|moveFrom|moveTo` も除く。

### Minor

- **m1. 描画 ID の走査範囲:**
  - `wp:docPr` だけでなく、story 内の `*:cNvPr/@id` すべてを max に含めるべき。`wps:cNvPr` が POI 8、tpl 10、fx 6 件、DrawingML main の `cNvPr` が SDK 29 件ある。これらはグループや図形内の ID で、Word は docPr と同じ連番から振っている *(u)*。
  - ADR-0012 は「パーツ内のすべての数値 `@id` の max」としており、保守性が揃っていない。
  - 2³¹ − 1 を超えるときは「最小の空き」ではなく `DrawingIdsExhausted` で拒否する。ADR-0012 の `ShapeIdsExhausted` と揃え、§2 の「ギャップは使わない」とも一貫させる（Q2）。
- **m2. 注釈 ID の和集合:**
  - 対象要素を列挙で固定し、「すべての `w:id`」と読める余地をなくす。`w:sdtPr/w:id` は乱数で負の値もある（SDK 1,948 個、うち負 21）ので、これを含めると max が 2³¹ 付近に跳ね、オーバーフロー時の規則がない。
  - `CT_Perm/@w:id` は `ST_String` 型なので、数値でない値は max の対象外にすることを明記する。
  - glossary を含めるかどうかを明記する（含めても害はない）。
- **m3. numbering:**
  - 新しい `numId` を `max(numId, numIdMacAtCleanup) + 1` にする（安全側。`numIdMacAtCleanup` は POI 3、SDK 13、tpl 2 ファイルにあり、SDK 4 件は max 以上）。
  - 新しい `w:num` は「最後の子として追加」ではなく、`numIdMacAtCleanup` の前（`num` スロットの末尾）に入れることを明記する。
  - `restart_list` が作る新しい `w:num` は、元の `num` にある**他のレベルの `lvlOverride` をコピー**しないと、他のレベルの見た目が変わる（`lvlOverride` を持つ `num`: POI 9、SDK 18、tpl 26）。
  - 「後続の段落の `numId` を付け替える」とき、スタイル経由で番号付けされている段落（段落自身に `numPr` がない）をどう扱うか、ADR-0007 の段落チェックに失敗する段落をどう扱うかが未定義。
- **m4. ブックマーク名:**
  - 「letter」が Unicode の文字を含むのかを明記する。コーパスにはキリル文字の名前があり（POI `WordWithAttachments.docx`: `язык`）、Word の UI 規則は「文字で始まり、文字・数字・`_` のみ、40 文字以内」（[WordTips](https://word.tips.net/T001014_Understanding_and_Using_Bookmarks.html)、[Microsoft Q&A](https://learn.microsoft.com/en-us/answers/a/11230517)）。
  - `remove_bookmark(name)` は、同名が複数ある場合（tpl 8、POI 1 ファイル）と、`REF`/`PAGEREF`/`w:hyperlink@w:anchor`/`HYPERLINK \l` が参照している場合は拒否する。
- **m5. 画像:**
  - dedup の候補は、パーツの content type が sniff した型と一致するものに限る。
  - 拡張子は、同じ型の既存 Default の拡張子を優先する（`jpg` か `jpeg` か）。
  - Phase3 以降で画像の置換や削除をする場合は、共有メディアに copy-on-write を適用し、全 story と glossary からの参照数を確認する。この 2 点を §3 に予告しておく。
  - 編集済み（`Replaced`）のパーツは現在のバイト列で hash する。
- **m6. ハイパーリンク:**
  - `rStyle "Hyperlink"` は styleId ではなく `w:name` で引く（ADR-0007 §5 の理由と同じ。日本語版・ロシア語版 Word の ID は `a`/`aN`）。
  - 既存の `w:hyperlink` の中への入れ子は拒否する。
  - core の `add_relationship` は空白を含む target を拒否する。「as given」と書く代わりに、拒否または percent-encode と書く。
- **m7. 表:**
  - `insert_rows` で `trPr/tblHeader` をコピーすると、新しい行も見出し行になる（`tblHeader` は SDK 138 個）。既定ではコピーしない。
  - `merge_cells` の `tcW` を合計幅に更新する。
- **m8. スコープのずれ:** Context の表と旧ロードマップには「new header/footer parts」「create tables」があるのに、§ には操作がない。Q6 とあわせて「Phase2 外」と明記する。
- **m9. ゲートの分類:** `split_run`/`insert_paragraph` は「everything else → structure」に入る。`split_run` は見た目を変えないので「none」でよい。
- **m10. T-OPC:**
  - 「every part has exactly one content type」は差分で判定することを明記する（もとから型のないパーツが POI に 1 ファイルある）。
  - 「パーツ名が既存の relationship target と衝突しない」を追加する。
- **m11. 名前空間のプレフィックス衝突:** `a` が別の URI にバインドされていても、ローカルで再宣言できる。拒否する代わりに別のプレフィックスを選ぶ選択肢を書く。
- **m12. ブランチ:** `plan/docx-phase2` は PR #13 のマージ前の main（c4e056f）から分岐しており、`crates/ooxml-docx` がない。memory の運用どおり `origin/main` をマージしてから再確認する。

## 推奨（Q1–Q8）

- **Q1:** `PartPlan` は採用しない。core を `core/add-part` の形に揃え、ADR-0009/0012 と同時に 1 枚の core 合意表を作る（B1）。表に載せる項目:
  - Default の方針（Override のみ、または C2）
  - パーツ名の規則（max + 1、dangling target・Override・削除済みの名前を避ける）
  - rId の大文字小文字
  - `r:*` 参照を考慮するかどうか
- **Q2:** 拒否する（`DrawingIdsExhausted`）。観測された max は 1,073,741,826 で、2³¹ まで十分な余裕がある。
- **Q3:** 40 文字以内、文字で始まり、文字・数字・`_` のみ。「文字」は Unicode の文字とする。比較は大文字小文字を区別しない。呼び出し側に `_` 始まりを許さないのは妥当。Word での確認は manual check に追加する。
- **Q4:** ASCII 英数字に除去する（Word の実例 `Tlotextu`、`berschrift1Zchn`）。空になったときは `a` + 最小の空き接尾辞にする（M4）。
- **Q5:** 提案どおり、新しいスタイルは `stylesWithEffects.xml` に書き足さない。
- **Q6:** Phase2 には入れない。入れる場合は、後続セクションへの明示的な参照、`titlePg`、`evenAndOddHeaders`（settings）を含む設計を別に立てる。
- **Q7:** `wp` と `pic` は Word 専用（pptx は `p:pic`、xlsx は `xdr:pic`）なので新しい `wpd.toml` に置く。`a:*` は `dml.toml` に置いて ADR-0005 の owner がレビューする。dry run は実装 PR の最初のコミットで構わない。
- **Q8:** owner が用意する Word 365 文書の `numbering.xml` からコピーし、fixture としてコミットする。コピー時の注意:
  - `nsid` は新しく発行する。
  - `w:tmpl`、`numStyleLink`/`styleLink`、`w15:restartNumberingAfterBreak` は除く。
  - 箇条書きの `lvlText`（U+F0B7 など）とその `rFonts`（Symbol/Wingdings/Courier New）はそのまま保持する。

## 判定

**Minor のみなら Accepted: no.** B1 と M1–M6 を直したうえで round 2 が必要。B1 は ADR-0009/0012 と合わせて core API を 1 つに確定させることが前提になる。
