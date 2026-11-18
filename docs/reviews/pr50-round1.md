# PR #50 レビュー（Opus, round 1）

対象: `impl/xlsx-phase2` @ `6442f15`（ADR-0006 Accepted rev 3 の part 1: shared-string / style registry、`Format`、style-only 書き込み、preserving setters、日付、`set_bool`/`set_error`）。
検証はすべて `git clone` した複製で行った（worktree は読み取りのみ）。

- 場所: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr50/`（`repo/` は検証用、`mutrepo/` は mutation 用）
- 保存物（`keep/`）:
  - `probe_review.rs`: 再現テスト P1–P10。
  - `probe_corpus.rs`: preserving setter・日付・named style を POI コーパスで掃くテスト。
  - `mutate*.py`: 作者の 27 mutant に、私の 26 mutant（R01–R26）を足したもの。
  - `mut.log`、`mut_corpus.log`、`sweep.log`: 実行ログ。
- コーパス: POI `test-data/spreadsheet`（47dcff76、366 ファイル、`p5/poi-xlsx`）。Python は `python3 -I` でだけ使った。

## 結論（先に）

**未収束。Blocker 0、Major 5、Minor 9。**

骨格は ADR-0006 どおりで、堅い。次の点はすべて確認できた。

- 追記専用。既存エントリのバイトは変わらない。
- 下から上への exact-form reuse。
- reserved entry の詰め込みと dangling 検査。
- `numFmtId` をパッケージ全体の最大値より上に割り当てる。
- `count` を差分でなく census から数え直す。stripping された `t="s"` も数える。
- Err ⇒ バイト同一、poisoning。
- 属性名の leak 修正。

コーパス sweep は作者の主張どおり 326 ok / 0 fail だった。私が追加した preserving setter の sweep でも失敗は 0 件だった。

Major は次の 5 件である。いずれも公開 API から黙って起き、テストでは捕まっていない。

- 不正な日付が `<v>NaN</v>` になる。
- `Builtin(id ≥ 164)` を使うと、後の custom 割り当てがそのセルの表示を変える。
- `Font` に `charset` がない。そのため日本語ブックの `with_default_font_of` が Calibri 11 に落ちる。
- 存在しないセルへの preserving 編集が、列や行のスタイルを捨てる。
- style 書き込みのたびに O(styles) または O(全数式) の処理が走る。

## 実施した検証

| 項目 | 結果 |
| --- | --- |
| `cargo test -p ooxml-xlsx --release` | 全通過 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 警告なし |
| コーパス sweep（`registry_sweep_over_a_local_corpus`） | 366 files、**ok 326 / fail 0**、refused 41（作者の主張と一致） |
| 追加の sweep `probe_corpus.rs`。既存の styled cell に `set_num_format`、`set_datetime`、`set_named_style("Normal")` を順に適用し、calamine で font、fill、border、alignment が不変か、numFmt が適用されたか、日付が往復するかを見た | ok 98、merged range での拒否 17（Phase1 の value guard として正しい）、`NoSuchStyle` 19（Normal を持たないファイル）、**不一致・panic 0** |
| 作者の mutant 27 個の再実行 | 25 killed、2 survived（M17、M23）。どちらも作者の言うとおり等価だった。M17 は core の挿入が AC sibling を拒否する。M23 は `plan_registries` が reuse の場合も含めて `check_insertable` を呼ぶ。**作者の 25/27 は再現した** |
| 私の mutant 26 個（R01–R26） | phase2+lib で 13 killed。R25 は full suite（`e12_rich_value_cells`）で kill。R14 と R17 は CI 外のコーパス sweep でだけ kill。R09 は等価（`INFO` は allowlist 外なので、どのみち sensitive になる）。**survived 9 件**: R01、R07、R11、R12、R13、R15、R18、R20、R22（m8） |
| rust_xlsxwriter oracle の正当性 | `rust_xlsxwriter-0.102.0/src/workbook.rs` の `prepare_num_formats` を確認した。すべての code を 164 から番号付けしている（`set_num_format` は文字列を保存するだけ）。`set_num_format_index(2)` で比べ、id 2 の写像を別に assert する方式は正当。表をエントリ単位で完全一致させており、ADR の「属性順を正規化した比較」より強い |
| manual_check 22–34 | ADR の XL 10(a)–(g) と Q2・Q5 に対応している（読んで確認）。(f) は Arial 10 のブック（62272）だけで、日本語の既定フォントを持つブックの項目がない（M3 を参照） |
| attribute-name leak | `Box::leak` は crate にも core にも残っていない。`XF_ATTRS` などの閉じた集合に写像し、未知の名前は opaque として扱う |
| origin/main との差 | main が #44（move-subtree）、#45、#46 の分だけ先行している。`git merge-tree` で衝突がないことを確認した |

## Findings

### Major

**M1. `ExcelDateTime` のフィールドが `pub` なので検証を迂回でき、`<v>NaN</v>` や別の日付が黙って書かれる**（`registry/dates.rs` 5–19 行、`setters.rs` `set_datetime`）

- 根拠: P1 を実行した。
  - `ExcelDateTime { year: 2026, month: 2, day: 31, hour: 99, minute: 0, second: f64::NAN }` で `set_datetime` は `Ok` になる。シートには `<c r="A1" s="1"><v>NaN</v></c>` が書かれる。`set_number` が NaN を拒否するのとは非対称である。
  - `second: 0.0` にすると `46088.125` が書かれる（2026-03-03 の 3:00 に相当）。2/31 と 99 時が黙って繰り上がる。
  - `year: 0` は「時刻のみ」を表す内部の目印で、それも外から作れる。
- 修正案: フィールドを private にして getter を付ける。あるいは `serial()` の先頭で `from_ymd_hms` と同じ検証を行い、`None`（`DateOutOfRange`）を返す。あわせて `set_datetime*` で `serial.is_finite()` を確かめる。

**M2. `NumFormat::Builtin(id)` が任意の `u16` を受け付ける。id ≥ 164 は未定義の参照になり、後で割り当てる custom id がその参照に意味を与えてしまう**（`registry/styles.rs` `num_fmt_id`）

- 根拠: P8 を実行した。テンプレートで A1 に `set_num_format_index(165)`、B1 に `"0.0000"`、C1 に `"0.00000"` を設定した。出力は `<numFmt numFmtId="164" formatCode="0.0000"/><numFmt numFmtId="165" formatCode="0.00000"/>` で、A1 の xf（numFmtId=165）は **C1 と共有されて reuse される**。A1 の表示形式が黙って「0.00000」に変わる。
- これは ADR §2/§5.3（review M3）の「dangling な id に意味を与えない」に反する。`next_numfmt` は activation 時の走査で決まり、セッション中に `Builtin` で使われた id は勘定に入らない。
- `Builtin(999)` も未定義の `numFmtId="999"` を書く（P5）。
- 修正案: `Builtin` は 0–163 に限る（できれば ECMA の built-in 集合 0–49 に限る）。それ以外は `InvalidFormat` にする。164 以上を許すなら、`numFmts` に定義済みの id だけを受け付け、使った id を `next_numfmt` の下限に反映する。回帰テストとして P8 を入れる。

**M3. `Font` に `charset`（と ADR §3 の `vert_align`、`condense`、`extend`）がない。東アジアの既定フォントで `default_font()` が `None` になり、`with_default_font_of` が黙って Calibri 11 になる**（`registry/format.rs` `Font`、`setters.rs` `font_from_node`）

- 根拠:
  - P2: テンプレートの font 0 を Excel 日本語版の既定（`<name val="游ゴシック"/><family val="2"/><charset val="128"/><scheme val="minor"/>`）にした。すると `default_font()` は `Ok(None)` を返し、`Format::with_default_font_of(&mut wb).set_bold()` は Calibri 11 bold になった。
  - コーパスでは `default_font()` が `None` のファイルが 43 件あった。そのうち **41 件の原因が `charset`** だった（`keep/fonts.py`）。日本語・中国語・韓国語の Excel ファイルは事実上すべてこれに当たる。
- ADR §3 は `Font` に `charset`、`vert_align`、`condense`、`extend` を含めている。PR の「Deviations」にも書かれていない。ADR が示す推奨パターン（n5、XL 10(f)）が、owner の主たるロケールで機能しない。
- 修正案: 次の 3 点を行う。
  - `Font` に `charset: Option<u8>`、`vert_align`、`condense`、`extend` を追加する。
  - それらを `build_font` で `CT_Font` の順に出力し、`font_from_node` で読む。
  - XL 項目 (f) に日本語の既定フォントを持つブックを加える。
- 少なくとも、`None` のときに黙って Calibri にせず、`Err` を返すか doc で明記すること。

**M4. 存在しないセルへの preserving setter、`set_named_style`、`set_datetime` が、行の `s`（`customFormat="1"`）と `col/@style` を無視して xf 0 から派生する**（`edit.rs` `plan_registries` の `current_s` が `None => (false, 0)`）

- 根拠: P3 を実行した。B 列を `col style="1"`（bold）、1 行目を `s="1" customFormat="1"` にした。
  - `B5.set_num_format("0.00")` と `D1.set_num_format("0.00")` は、どちらも `<xf numFmtId="2" fontId="0" …>` を参照した。bold が消える。
  - `B6.set_datetime` も xf 0 から派生した（numFmt 14）。
- Excel は存在しないセルを行や列のスタイルで表示するので、見た目が変わる。`set_num_format` の doc にある「keeping every other part of the cell's current style」に反する。「日付」列だけに列書式を付けたテンプレートは日本の業務 Excel で典型的なので、`set_datetime` の既定動作で表示が変わる。
- Phase0 の `set_number` も存在しないセルを xf 0 にするが、これは既存の挙動である。ADR が Phase4 に回したのは列や行のスタイルを**書く**ことで、読むことではない。
- 修正案: セルが無いとき、実効 `s` を「行の `customFormat="1"` の `s`」→「その列を覆う `col/@style`」→「0」の順で決め、derive・named・EnsureDate の `current` に渡す。少なくとも doc に明記し、テストを入れる。

**M5. style 書き込みのたびに O(スタイル表) と O(全数式) の処理が走り、大きなブックで実用的でない速度になる**

- 根拠: P9、P10 を release、負荷のある共有ホストで実行した。
  - **(a)** `Styles::plan` は毎回 `self.clone()` する。中身は全テーブルの `reuse: HashMap<String,u32>`、`xfs`、`csx_infos`、`codes` などである。cellXfs が 5,001 のブックで同じ `Format` を 5,000 回書くと **22.3 s**（4.4 ms/回）かかる。xf が 1 個なら 0.12 s である。
  - **(b)** `style_sensitive()` は style-only 書き込みのたびに、census の全数式と全 definedName を `lex` し直す。数式 20,000 個のブックで `set_style_index` を 2,000 回呼ぶと **13.3 s** かかる。同じブックでの `set_number` 2,000 回は 0.09 s、`set_number_with_format` 2,000 回は 0.14 s である。
- ADR §7.3 は「one pass over texts the census already holds」と書いており、1 回の走査を想定していると読める。
- 修正案:
  - (a) staged clone を差分（追記ログ）にする。clone は追記が必要なときだけにし、`HashMap` は apply 時に差分を反映する。
  - (b) 判定結果を `Workbook` にキャッシュし、数式や名前が変わったときだけ無効にする。数式の書き込みでは、新しい数式だけを見て true にできる。
  - 回帰用の perf テストを `tests/perf.rs` に加える。

### Minor

**m1. Code → built-in の写像にロケール依存の 14 と 22 が入っている**（`format.rs` `BUILTIN`）

- `set_num_format("mm-dd-yy")` は id 14、`"m/d/yy h:mm"` は id 22 に写像される（P5）。Excel はこれらをシステムの短い日付形式で表示するので、日本語版では `yyyy/m/d` になり、呼び手が書いた code どおりに表示されない。
- ADR §5.3 は「locale-independent built-in code」に限っている。`Format::date()` が使う `Builtin(14)` は残し、`Code` → id の写像からは 14 と 22 を外すべきである（5–8 と 41–44 が除外されているのは正しい）。

**m2. `NumFormat::General` は、ファイルが id 0 を再定義していても 0 を返す**（`num_fmt_id`）

- 52348.xlsx は 0–2 を再定義している。`Builtin` と同じく `BuiltinNumFormatRedefined { id: 0 }` にすべきである。

**m3. `Format::date()` は、14 が再定義されたブックで `Code("yyyy-mm-dd")` に落ちない**

- ADR §6 は落ちることを定めている。実装でこの fallback があるのは `EnsureDate`（`set_datetime`）だけで、`set_*_with_format(&Format::date())` は `BuiltinNumFormatRedefined` を返す。
- 実装と ADR のどちらかを合わせ、Deviations に記すこと。

**m4. `Format` の値検証に漏れがある**

- `formatCode=""` を受け付ける（P5）。
- theme index 200 を受け付ける（P5）。
- 100 文字のフォント名を受け付ける。Excel の上限は 31 文字。
- `family` が 200 でも通る。`ST_FontFamily` は 0–14。
- `indent` が 255 でも通る。Excel の上限は 250。
- いずれも Excel の修復ダイアログの原因になりうる（XL では未確認）。`validate` に加えること。

**m5. 読み取り API の `is_date` と `datetime` が style registry を活性化する**

- styles part のないブック（59021.xlsx）では `Err(NoStylesPart)` になる。`false` を返すべきである。
- activation は全 XML パートの `numFmtId` も走査するので、読むだけで重くなり、デコードできないパートでは失敗する。
- また、`styleSheet` 直下に AC があると、どのテーブルを包んでいるかに関係なく style 全体を拒否する。ADR §2 が拒否を求めるのは、包まれたテーブルの挿入と、`cellStyleXfs` が包まれているときの既定の読み取りだけである。この差は PR の Deviations にない。

**m6. `cellStyleXfs[0]` が（index-stable な）AC エントリのとき、`default` が `None` になる**

- このとき、未設定の group は黙って id "0" になり、alignment も protection も付かない。ADR は Normal を基準にすると定めている。`NotExpressible` で拒否すべきである。コーパスに該当例はない。

**m7. `numFmtId` 走査は文字参照で過小評価する**（`max_numfmt_id`）

- `numFmtId="&#49;70"` は `parse` に失敗して飛ばされる。ADR n6 は「must not under-estimate」としている。
- 値に `&` が含まれるときは、XML として decode して読むか、拒否する。

**m8. mutation の生存とテストの穴**

- 次の 9 件が phase2、lib、full のどれでも死なない。
  - R01: 上限の境界（`>` を `>=` に変える）。
  - R07: Fallback のない AC を stable と数える。
  - R11: UTF-16 パートを走査で飛ばす（ADR n6）。
  - R12: `cellStyleXfs` の fontId を dangling 検査から外す。
  - R13: `fills[0]` を中身に関係なく reuse する（ADR n4。64450 は `fills[1]` しか見ていない）。
  - R15: 既定の alignment を継承しない。
  - R18: count のない表が伸びたときに count を付ける（ADR m8）。
  - R20: `date1904="true"` を受け付けない。
  - R22: 上限の計算から `cellStyleXfs` を外す。
- R14（既定を常に `cellXfs[0]` にする、ADR M1 の要点）と R17（count のない sst に count を付ける）は、CI 外のコーパス sweep でしか死なない。
- それぞれ小さな合成テストを加えること。R14 には `cellStyleXfs[0]` と `cellXfs[0]` で fontId が違う合成ブックを使う。

**m9. PR 説明の「Deviations」が不完全**

- 次の 3 件が載っていない。
  - `Font` に `charset`、`vertAlign`、`condense`、`extend` がない（M3）。
  - `styleSheet` 直下の AC で全体を拒否する（m5）。
  - `Format::date()` の fallback がない（m3）。
- 「Core has no subtree-copy primitive on main」も古くなっている。#44（move-subtree）が main に入ったので、preserving edit をバイト単位のコピーにできるか、part 2 で再検討すること。

### 指摘しない（確認済み）

- **count と uniqueCount**:
  - activation 時に、現在の木（dirty なパートは serialize したもの）を走査して数える。活性化前の書き込みも正しく反映される。
  - target の旧 `t="s"` は差し引き、stripping された `t="s"` も数えている（M24、R02、R03 は kill）。
  - P4: 上書き・reuse・`xml:space` を混ぜた 2 セッションで、count は 2 → 3、uniqueCount は 3 のまま正しかった。
- **append-only と exact-form reuse**:
  - reuse の key は、ids を解決した後の xf の canonical bytes である。
  - table ごとに map を持ち、最小の index が勝つ。
  - stable でない table では reuse しない（R06 は kill）。
  - prefix は table の prefix に従う。
- **上限**: 伸びた table だけを検査する。「すでに上限を超えているが reuse する」場合を通すことを確認した（M18b は kill）。
- **rich text と phonetic の既存項目**: plain の canonical bytes とは一致しないので、reuse されない（テスト `rich_or_phonetic_si_is_never_reused`）。
- **style-only 書き込み**:
  - merged、table header、array の guard を迂回するのは妥当である（値が変わらない）。
  - `vm` セルの guard は value 書き込みについて残っている（R25 は kill）。
- **日付**:
  - 1900 系の偽の 2/29（serial 60）を扱っている。
  - 1904 系、9999-12-31、`from_serial(60)` は `None`。
  - Gregorian の世紀規則（R23 は kill）。
- **poisoning**: 新しい `ApplyStep::Registries` での fault injection をテストしている。

## 判定

**converged: no**（Major 5 件）。M1–M5 を直し、m1–m9 を反映または Deviations に記載した後、round 2 で再確認する。
