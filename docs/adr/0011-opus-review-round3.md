# ADR-0011 レビュー（Opus, round 3）

- 対象: PR #31 `docs/adr/0011-docx-phase2-structure.md` revision 3（a1fad6a、merge a966ab4）
- 照合先:
  - PR #45 `core/insert-with-ns`（2a76520。`crates/ooxml-core/src/lens.rs`、`tests/insert_ns.rs`、ADR-0002 note）
  - PR #38 `core/add-part`（31906a9。`package.rs` 352-391、`package/add.rs`）
  - `origin/main`（848b057）の ADR-0007（Accepted, rev 3）と ADR-0012（Accepted, rev 3）
- プローブ（`scratchpad/review-31r3-35r2/31/`。`python3 -I`、正規表現でルート開始タグだけを見る。XML パーサも実体解決も使わない）:
  - `probe_r.py` / `probe_r.out`: story パーツのルートに `r` の prefixed binding があるか
  - `probe_r2.py` / `probe_r2.out`: それが無いパーツの `headerReference` と `hyperlink`
- 実行確認: PR #45 の `cargo test -p ooxml-core --test insert_ns` は 6 passed（target は scratchpad）

## 結論（先に）

**round 2 の N1〜N9 はすべて解消した。**

- **N1:** 「作成時に宣言する挿入」に切り替えたことで、`declare_namespace` の到達不能問題は消えた。
  - core 側はすでに PR #45 が実装済みで、ADR の要件を満たしている。
  - `wp:inline` → `a:graphicFrameLocks` → `a:graphic` → `pic:pic` の各段で `insert_with_ns` / `get_or_insert_with_ns` を使えば、`insert_image` は組める。PR #45 のテスト `word_inline_picture_shape` が同じ形を通している。
- **N2:** 順序「dry run → plan → apply → ツリー編集」は、PR #38 の世代管理と合っている。
  - `xml_part_mut` だけが generation を進め、`xml_part` と `parsed_part` は進めない（`package.rs` 352-391）。

残りは Minor だけである。

- **R3-1:** §1 の要件文が PR #45 の実際の仕様とずれている（名前、prefix 規則、check の戻り値、`insert_at` 版）。
- **R3-2:** 新しく作ったノードの下への挿入は、事前に check できない。dry run の文言を直し、深さの上限を入れる。
- **R3-3:** `r` の宣言が必要になるのは画像だけではない（ハイパーリンクでも要る）。
- **R3-4:** ADR-0007 と ADR-0012 の状態表記が古い。ADR-0007 から委ねられた保護 opt-in の行き先も書かれていない。

## 解消状況

| ID | 指摘（round 2） | 状況 | 根拠（rev 3） |
|---|---|---|---|
| N1 | 名前空間宣言が core の挿入 API で実現できない | **解消**（文言のずれは R3-1） | L192-230, L312-324。PR #45 の `NsDecl` / `*_with_ns` / `check_*_with_ns` が要件を満たす。T2 は維持され、拒否は何も変えない |
| N2 | plan を先に作ると `StalePlan` | **解消** | L155-168。`RegistryPlan::new` は id と generation だけを記録し（`add.rs` 471-499）、`xml_part_mut` だけが generation を増やす（`package.rs` 377）。step 1 → 2 → 3 の間に `xml_part_mut` が無ければ apply は通る。ADR-0012 の `stage_part` を採らない理由も書かれた |
| N3 | T-OPC の target 存在チェックが入力の dangling で落ちる | 解消 | L665-675。追加分だけを判定し、Id の一意性も「追加分 × 入力」で判定する |
| N4 | rId が参照も避けるのか曖昧 | 解消 | L179-185。`RelIdReferenced` で拒否し、T-REG で検査する。大文字小文字についての古い記述は撤回された |
| N5 | PR 番号が古い | 解消（別の古い表記は R3-4） | L14, L776, L827 |
| N6 | `a`+数字が Word の形という記述 | 解消 | L556-560 |
| N7 | `lvlOverride` が `[detach]` に無い | 解消 | L640-644 |
| N8 | 対象外の manual check が残っている | 解消 | §12 から削除された |
| N9 | 後半の `titlePg` と注の番号の振り直し | 解消 | L427-434。structure 列から `NOTEREF` が外れた（L607-609） |

## 新規指摘（すべて Minor）

### R3-1. §1 の core 要件文を PR #45 に合わせる（L192-230, L828）

ADR は「names indicative」と断っており、PR #45 は ADR が実際に必要とするものを満たしている。それでも、次の 4 点は文言が実装と食い違っている。

1. **prefix の規則**（L211-214）
   - ADR: 「preferred prefix が未束縛か、**同じ URI に束縛済み**なら、それを使う」
   - PR #45: URI に **どの prefix でも**束縛があれば、それを使って何も宣言しない（`plan_declarations`。テストでは `xmlns:x="urn:a"` があると `x:graphic` になる）
   - 意味の上では PR #45 の方が冗長な宣言を作らず、良い。ADR を PR #45 の規則に合わせ、「Word の形」（L220）は「`a` がスコープに無い場合の Word の形」と限定する。
2. **check twin の戻り値**（L209-210）
   - ADR は「実際に使った prefix を返す」を要件にしている。PR #45 の `check_*_with_ns` は `Result<()>` を返す。
   - ADR-0011 の中に、返された prefix を使う箇所は無い。属性は `choose_attr_prefix` が自動で選ぶ。この要件は削除する。
3. **`insert_at_declaring`**（L203）
   - PR #45 には位置指定の版が無く、PR #26 の `insert_at` に対する拡張点として note に書いてあるだけである。
   - ADR-0011 の挿入は、どれも `insert_by` か、作成済みノードへの `insert` / `get_or_insert` である（L325-326 の `w:r`、§4 の `w:hyperlink`。後者は分割した後半の前に `before` 述語で置ける）。
   - 「`insert_by_with_ns` / `insert_with_ns` / `get_or_insert_with_ns` で足りる。位置指定版は不要」と明記する。必要なら、PR #26 に `_with_ns` を追加することを依存に書く。
4. **PR 番号**
   - L828 の「number to be added」を **PR #45** にする。
   - 「Depends on」と Consequences（L776-779）にも #45 を書く。
   - 逆方向の指摘: PR #45 の ADR-0002 note は「as ADR-0011 §1.3 requires」と書いているが、ADR-0011 に §1.3 は無い。§1「What else core needs」item 1 か §1.1 を指すように、PR #45 側で直す。

### R3-2. 新規ノードの下への挿入は、事前に check できない（L229, L323-324）

- 「Every insert of the operation is dry-run with the check twin」は、文字どおりには実行できない。
  - `check_*` は既存ノードの `LensMut` に対して呼ぶものである。`wp:inline` を作る前に `a:graphic` の check を呼ぶことはできない。
  - 変更前のツリーで check できるのは、最も外側の `w:r`（`check_insert_by`）だけである。
- ADR-0007 §7 は「同じ呼び出しで作ったノードへの挿入は `NotAllowed` 以外で失敗しない」としている。`*_with_ns` を使うと、失敗の種類が 2 つ増える。
  - `InvalidNamespaceDeclaration`: `NsDecl` は定数なので、静的に起こらない。
  - **`TooDeep`**: `insert_image` の部分木は、`w:p` から 9 段ある（`w:r/w:drawing/wp:inline/a:graphic/a:graphicData/pic:pic/pic:spPr/a:prstGeom/a:avLst`）。`check_insert_by::<R>` が見るのは 1 段目の深さだけである。
- **修正案:** L229 と L323 を次の趣旨に書き換える。
  - 「外側の `w:r` を `check_insert_by` で確かめる」
  - 「`w:p` の深さ + 部分木の高さ（画像は 9、ハイパーリンクは 2）≤ `max_depth` を確かめる」
  - 「内側の `*_with_ns` は定数の `NsDecl` で呼ぶので、これ以外に失敗しない」
- **関連（N2 の補足）:** step 2 と step 3 の間で次の 2 つを実行する場合は、`parsed_part`（`&self`）か `xml_part` で読む。`xml_part_mut` を呼ぶと、apply が `StalePlan` になる。
  - N4 の `r:*` 走査
  - §2 のレジストリの遅延走査（ヘッダーなど、他の story の `docPr` の走査）

  遅延走査は step 1 で済ませておく、と 1 行書いておく。

### R3-3. `r` の宣言はハイパーリンクにも要る（§4, L332-349）

- §1 の一般規則（L216-218）は、属性名の名前空間も対象にしている。しかし具体例は `insert_image` しか無い。
- `w:hyperlink/@r:id` を書くには、`r` が非空の prefix で束縛されている必要がある（ADR-0007 §7 n4 と同じ理由）。
- プローブ（`probe_r.out` / `probe_r2.out`。Transitional のみ）で、ルートに `r` の prefixed binding が無い story パーツ:

  | コーパス | 件数 | 内訳 |
  |---|---|---|
  | POI | 7 | `document.xml` 6、`footer.xml` 1 |
  | SDK | 6 | `document.xml`。ほかに Strict が 24 あるが、ADR の対象外 |
  | tpl | 2 | `footer.xml` |

  どれも `headerReference` を持たない。したがって、§6 のセクション区切りで参照を書き戻す処理は影響を受けない。
- **修正案:** §4 に「`w:hyperlink` は `NsDecl(r)` 付きで挿入する（`r` が束縛済みなら宣言は書かれない）」と 1 行書く。テストの fixture に、`r` を持たない POI のファイル（`55733.docx` など）を加える。

### R3-4. 依存先の状態が古い。ADR-0007 から委ねられた項目の行き先が無い

- **状態表記が古い**
  - L11 は ADR-0007 を「PR #25, under review」と書いている。ADR-0007 は main で Accepted（rev 3）である。
  - L22-23 の ADR-0012 も PR #30 で merge 済み（Accepted, rev 3）である。
  - ブランチは `origin/main` から #25、#30、#40、#41、#42 の分遅れている。merge-not-force-push の方針どおり、`origin/main` を merge して表記を直す。
- **委ねられた項目の行き先が無い**
  - ADR-0007 の Q10 と Alternatives 10 は「the enforced-protection opt-in and lifting A2 go to … ADR-0011」と書いている。
  - ADR-0011 が扱っているのは A2 の解除（L441）だけで、保護 opt-in については一言も無い。
  - 「Out of Phase2」（L109-）に「enforced-protection opt-in: 後の ADR へ」と書くか、Phase2 で扱うかを決める。ADR-0007 と ADR-0011 の間で、行き先が無いまま宙に浮かせないためである。

### 確認した上で問題なしとした点

- **ADR-0012 との整合**
  - ADR-0012 rev 3 はクローンを `stage_part` する方式を採り、ADR-0011 はインプレース編集 + 事前 dry run の方式を採る。どちらも PR #38 の契約（apply は `StalePlan` 以外で失敗しない）の範囲に収まっている。
  - `Default` の扱いも形式ごとの方針差にとどまり、矛盾ではない。
  - ADR-0014 rev 2 §12 も ADR-0011 と同じ 4 段の順序を採っており、形式をまたいで揃っている。
- **ADR-0007 との整合:** 次の 3 点は、Accepted 版 ADR-0007（§6.1, §7, §8.2）と矛盾しない。
  - structure 列（`TC`, `XE`）
  - `SameType` modulo skips
  - `check_insert` / `check_get_or_insert`（PR #45 が実装した）
- **rId の回避:** 2 段で成り立っている。
  - core: `.rels` の Id を大文字小文字を無視して避け、削除済みの Id も避ける（PR #38）。
  - ooxml-docx: `r:*` の参照値と一致したら `RelIdReferenced` で拒否する。

  実測で到達するケースは 0 件だが、安全網として妥当である。

## 判定

**Accepted after Minor only: yes.**

R3-1〜R3-4 は、どれも文言・参照・テスト fixture の修正で済み、設計は変わらない。N1 の core 要件は PR #45 で満たされている。PR #45 側の ADR-0002 note の節番号（R3-1 の 4）は、PR #45 のレビューで直す。
