# PR #56 レビュー（Opus, round 1）

対象: `impl/pptx-phase1-resolver` @ `1006394`（ADR-0008 rev 3 の §1 継承リゾルバと §3 読み取り API、#48 round-2 の後始末）。
基準: ADR-0008 rev 3（Accepted）、owner 決定 Q1–Q5（提案どおりの既定値）、ADR-0002 の owner 決定 Q2（`EnumValue`）。
プローブ: `git clone` したコピーで実行した（`/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr56/`）。worktree は変更していない。

## 結論（先に）

- **リゾルバの答えは正しい。** 独立した判定方法を 3 つ使い、どれでも不一致は 0 件だった。
  - spike のカウンタ: 76 個すべて一致した。
  - レビュアーが書いた独立の Python ミニリゾルバ: 619 デッキ、12,483 ラン × 6 属性で一致した。
  - python-pptx: ジオメトリの差は、ADR が予告した 6 件だけ。リンクは 172 件で 0 failures だった。
  - 読み取り API のカウンタも `features-633.txt` と一致した。デッキを読んでもパッケージは汚れない（619 件中 0 件）。
- **ただし、まだマージしないほうがよい。Major が 2 件ある。**
  - **M1:** ADR と owner 決定が求める公開 API の一部がない。それなのに、PR 本文の「対象外」には書かれていない。
    - Q1 の `is_text_box()`
    - Q4 のスクリプト別フォント helper
    - `xsd_default()`、`slide_rect()`
    - `Deck::layouts`/`masters`
    - ノート上のリンク
  - **M2:** 列挙型の値を生文字列で返している。これは ADR-0002 の owner 決定 Q2（`EnumValue`）と ADR-0008 §1.3 に反する。生成済みの enum 型はすでにある。
- Minor は 6 件ある（試験の穴、グループのジオメトリ、group-winner 属性の扱い、MCE の扱い、コメント分類器の重複）。
- §5 スケッチからのずれのうち、**解決メソッドを `DeckSlide` に置くことは妥当**である（理由は下）。**列挙値を生文字列で返すことは、owner の承認がない限り不可**とする（M2）。
- #48 round-2 の後始末（r2-m1、r2-m2）は、いずれも効いていることを確認した。

**判定: converged = no**（Blocker 0、Major 2、Minor 6、Nit 数件）。

---

## 検証したこと

| 項目 | 方法 | 結果 |
|---|---|---|
| ビルドと試験 | `cargo test -p ooxml-pptx`、`cargo xtask codegen --check`、`clippy -D warnings` | すべて通過。codegen は「7 generated files up to date」 |
| spike モデル（O2 相当） | `OOXML_PPTX_CORPUS=…` で `corpus_matches_the_spike_model` を実行した。633 ファイル = POI 94 + SDK 8 + python-pptx 11 + LO 3 + LO 残り 417 + ShapeCrawler 100 | **633 files, 76 counters compared, 0 differ** |
| 独立差分（レビュアー作） | `probes/mini.py`: ADR §1.2–§1.4 だけを読んで書いた Python ミニリゾルバ。`chain.py` は見ていない。Rust 側は `examples/rv_dump.rs`（プローブ用）。読める 619 デッキの全ラン（50 デッキの抜き取りではなく全数）で `sz`/`b`/`cap`/fill/`latin`/`ea` を比べた | **12,483 ラン × 6 属性、不一致 0**。Unresolved の内訳: `ea` ScriptDependent 11,543、`latin` ScriptDependent 4（ADR の `bug64693` 4 件と一致）、ThemeOverride 7 ラン × 3 属性（`60810.pptx` slide 18） |
| python-pptx の判定（O1/O3） | `resolve_check.py` を、python-pptx が開けないファイルを飛ばす形にしてコーパス全体に適用した（`probes/rc.py`、`rv_geo.rs`）。fixture だけでの実行も別に行った | fixture: 112 placeholders、7 links、0 failures。コーパス: **1,272 placeholders のうち差は 6 件**（orphan を master から解決した 5 件と、`tdf143129` の idx-0 タイ 1 件。ADR §7 O1 の予告どおり）。**links 172、0 failures** |
| 読み取り API（O6） | `examples/rv_feat.rs` の数を `features-633.txt` と比べた | 下の表のとおり一致した |
| ロスレス | 619 デッキで `deck()` と全 API を呼び、その前後で `to_bytes()` を比べた | 変化 0 件 |
| mutant（レビュアー作 18 個） | 下記 | 11 個を検出、7 個が生存（m1） |
| #48 の後始末 | X14（`rposition`）、X6（`LinkSplitByBreak` を無効化）、混在書式の拒否を無効化。それぞれで `cargo test` と `check.py` を実行した | X14 は `rule_4_keeps_the_first_run_with_its_format` で検出。X6 は `check.py` の 12 failures、混在書式は 6 failures で検出。無変更では `check.py` 113 cases、0 failures |

読み取り API の突き合わせ（`rv_feat` と `features-633.txt`）:

| 項目 | ooxml-pptx | spike |
|---|---|---|
| sections: ファイル / セクション / 空セクション | 21 / 59 / 35 | 21 / 59 / 35 |
| comments: legacy / modern / reply | 11 / 1 / 2 | 11 / 1 / 2 |
| timing / 効果（entr 220、exit 30、mediacall 19、path 17、emph 13） / `bldP` 123 / `bldGraphic` 3 | 一致 | 一致 |
| transition: direct | 240 | 240 |
| transition: Choice | p14 80、p15 1、p159 1（`p159:morph`） | Requires p14 81、p15 1、p159 1 |
| リンク: ラン click / ラン hover / 図形 | 83 / 1 / 94 | 83 / 1 / 94 |
| `color_rule = RunColor` | 12 | ADR の 12 件（`ahyp:hlinkClr`） |

p14 の 1 件の差について: `animation.pptx` slide 2 では、`mc:Choice` が `xmlns:p14` を**プレリリース版の URI**（`…/powerpoint/2007/7/12/main`）に束縛している。そのため URI で比べると「理解しない」名前空間になり、`mc:Fallback`（`p:checker`）に落ちる。これは ADR §3.4 の「prefix の文字列ではなく URI で比べる」どおりの正しい動作である。

ADR の各規則（すべて上の差分 0 件と、コードを読んで確認した）:

- 属性の存在で判定し、XSD の既定値で連鎖を止めない（`find_attr` は raw の `attr` しか見ない）。
- type/idx の照合、`4294967295`（6 件とも `Type`）、layout → master は layout の family で決める（r2-m2）。
- 連鎖の順序: プレースホルダーは POI 順で、`defaultTextStyle` は step 8 だけ。非プレースホルダーは `defaultTextStyle` → `otherStyle`。
- `lvl1pPr`–`lvl9pPr` は該当レベルだけを見る。不正な `lvl` は step 3 で `InvalidLevel` になる。
- `a:defPPr` は見ない。段落の `a:defRPr` は見る。
- `p:style/a:fontRef` は自分の図形のものだけを見る（`idx="none"` は書体を与えない）。
- `+mj-lt`/`+mn-ea`、空の `ea`/`cs` → `ScriptDependent`。`themeOverride` → `Unresolved`。
- `clrMapOvr` は slide → layout → master の順。`DmlPercent` の 11 overrides で `DefaultMismatch` は 39 から 36 になった。
- 継承した autofit の scale は `Unresolved`。プレースホルダーがグループ内にあれば `GroupCoordinates`。
- リンク色の規則は `hlinkClick` だけ。`Requires` は URI で比べ、p159 も理解する。

---

## Findings

### M1. ADR と owner 決定が求める API の一部がなく、PR 本文の「対象外」にも書かれていない

**根拠.**

- PR 本文は、対象外を「§2（ノート編集）と §1.7 のノート/レイアウト/マスター文脈」とだけ書いている。
- `crates/ooxml-pptx/src` を grep すると、次のものがない（`is_text_box`、`txBox`、`xsd_default`、`slide_rect`、`TransformedGroup`、`Jpan`、どれも 0 件）。
  - **Q1（owner 決定）**: 既定値の文面は「… POI for text boxes and shapes alike, with `Source` **and `is_text_box()` exposed**」（ADR-0008 Open questions、§1.3、§5 `ShapeRef::is_text_box`）。M1 の手動確認で読み方が分かれたとき、呼び出し側が種類ごとに扱いを変えるための口である。これがないので、owner が採った既定値の一部が実装されていない。
  - **Q4（owner 決定）**: 「a separate, documented helper maps `lang`/`altLang` to a script and picks a candidate, never reported as `Set`」（§1.4）。この helper がない。日本語のデッキでは `ea` 11,543 ランがすべて `ScriptDependent` になるので、この helper がないと東アジアの書体は実質的に取り出せない。
  - §1.3 / §5 の `xsd_default(PropertyId)`: `Unset` のときに仕様の既定値を得る、と ADR が約束した口である。
  - §1.5 / §5 の `slide_rect()` と `Err(TransformedGroup)`。
  - §5 の `Deck::layouts()`/`masters()`、`ParagraphRef::end_props`。`run_props(…, RunItem::End)` で代わりになるので、これは軽い。
  - §3.1「on slides **and notes**」と、レイアウト/マスター上のリンク（コーパスに 5 件）。前者は §1.7/§2 の後回しに含まれるとも読めるが、そうは書かれていない。
- 一方で、PR 本文は「following the Accepted ADR (rev 3) and the owner-confirmed Q1–Q5 defaults」と書いている。

**推奨.**

- 次のものは小さいので、この PR で実装する。
  - `is_text_box()`: raw の `p:cNvSpPr@txBox`、ADR §6 では allowlist 外の raw 読みとされている。
  - Q4 の helper: `lang` → script の表と、`ScriptDependent.script_fonts` からの選択。
  - `xsd_default`: 生成済みの `AttrSpec` の default を引く。
- `slide_rect`、`layouts`/`masters`、ノートやレイアウト/マスター上のリンクを後回しにするなら、PR 本文の「Out of scope / Follow-ups」に一つずつ並べ、owner の了承を取る。

### M2. 列挙型の値（`algn`、`u`、`strike`、`cap`、`anchor`、`wrap`、`vert`、`buAutoNum@type`、`spd` …）を生文字列で返している

**根拠.**

- 該当箇所: `resolve.rs:891`（`underline: Lookup<'a, &'a str>`）、`:1055`（`align`）、`:1152`（`anchor`）など。`deckread.rs` の `Transition::speed: &str` も同じである。
- ADR-0002 の「Resolved questions 2（owner 決定 2026-11-16）」は、「Typed enum reads return `EnumValue<'a, E> { Known(E), Unknown(&'a str) }` … the type forces callers to decide what to do with unknown values」としている。
- ADR-0008 §1.3 も、「parse the present value with the attribute's generated simple type」としている。
- 必要な生成型はすでにある。`dml.rs` に `TextAlignmentTypeValues`、`TextUnderlineValues`、`TextStrikeValues`、`TextCapsValues`、`TextAnchoringTypeValues`、`TextVerticalValues`、`TextWrappingValues`、`TextAutoNumberSchemeValues` があり、`pml.rs` に `TransitionSpeed` 系がある。
- `ooxml_core::EnumValue::from_raw` は 1 行で使える。Phase 0 の `PlaceholderRef::kind()` もすでに `EnumValue` を返しているので、同じ crate の中で型付けの方針が割れてしまう。
- 公開 API の形が変わるので、後から直すと破壊的変更になる。

**推奨.** `Lookup<'a, EnumValue<'a, …Values>>` にする（`find_attr` に `|s| Some(EnumValue::from_raw(s))` を渡すだけでよい）。生文字列のまま行くなら、ADR-0002 Q2 の例外として owner の明示的な承認を得て、ADR-0008 に post-acceptance note を残す。

**§5 スケッチからのもう 1 つのずれ（解決メソッドを `DeckSlide` に置き、`shape`/`body`/`para` を引数で受ける）は妥当である。**

- 連鎖の文脈（layout、master、theme、colour map）が要る以上、`ShapeRef`/`RunRef` に置くには、それらがデッキへの参照を持つ必要がある。そうすると Phase 0 の `Slide` の API が変わってしまう。
- そのため、今の形は筋が通っている。ADR に post-acceptance note で記録するのがよい。

### m1. 試験の穴: 主要な規則の一部が、CI で走る試験に守られていない

**根拠（レビュアー作の mutant、`probes/mutants.py`、`probes/mut2.py`。`--lib --test resolve --test deckread --test oracle` で実行）.**

| mutant | 結果 |
|---|---|
| Y1 段落 `defRPr` を落とす / Y2 プレースホルダーで `fontRef` を無視 / Y3 master を slide の family で選ぶ / Y4 継承した autofit scale を Set にする / Y7 `Requires` を prefix の文字列で比べる / Y8 hover にも色の規則を返す / Y9 空の theme 書体を解決済みにする / Y10 layout の `clrMapOvr` を無視 / Y12 グループ内プレースホルダーにジオメトリを継承 / Y13 表のセルで連鎖を続ける / Y15 `startAt` の `defaulted` を常に false | 検出 |
| **Y5/Y6** `themeOverride` を無視（フォント / 色） | **生存**（`themeOverride` の試験がない） |
| **Y11** step 3 の「idx 0 を優先」を外す | **生存**（opt-in のコーパス試験でも 0 differ） |
| **Y14** `lvl=9` を受け入れる（`[usize::from(9)]` で panic する） | **生存**（試験は `lvl="12"` だけ） |
| **Y16** `fontRef idx="major"` の `ea` を `+mn-ea` にする | **生存** |
| **Y17** `clickPar` の数え方を壊す（`click_index`） | **生存** |
| **Y18** `lstStyle` のレベルを 1 つずらす | **CI では生存**。opt-in のコーパス試験だけが検出（6 differ） |

- ADR §7 O2 は、synthetic の S2（レベル 0–8 と欠けたレベル）、S3（タイ / idx なし / family 違い / orphan / `4294967295`）、S4（dt/ftr/sldNum）、S8（notes）を求めている。
- また O2 は、それらを「committed script `tests/fixtures/build_phase1.py` で作る `reference/fixtures/pptx/synthetic/*.pptx`」とし、期待値を mechanical と semantic に分けるよう求めている。
- 実装は `poi/sample.pptx` をメモリ上で書き換える方式で、レベル 0 のタイトル 1 つしか試していない。この方式の変更自体は合理的だが、PR 本文には書かれていない。

**推奨.**

- 次の試験を足す。
  - `lvl` 1 と 8 の値。`lvl="9"` → `InvalidLevel`。
  - `themeOverride` 関係を付けた slide（色とフォントが `Unresolved(ThemeOverride)` になること）。
  - `fontRef idx="major"` で `ea`/`cs` が `+mj-*` を指すこと。
  - layout に同じ type が 2 つあり、片方だけ idx 0 のときの `Type` の選択。
  - `click_index`。
- fixture の方式の変更は、PR 本文に一行書く。

### m2. グループ図形のジオメトリが `Unset` になる（実際は `p:grpSpPr/a:xfrm` がある）

**根拠.**

- `resolve.rs:581` の `own()` は、`p:graphicFrame` なら `p:xfrm`、それ以外は `p:spPr` を見る。そのため `p:grpSp` では何も見つからない。
- プローブ P1（`rv_syn.rs`）: `<p:grpSpPr><a:xfrm><a:off x="10" y="20"/>…` を持つグループに対して `geometry()` は `Unset` を返した。
- `Unset` は「連鎖のどの要素も設定していない」という意味なので、答えとして誤りになる。`a:CT_GroupShapeProperties/p:grpSpPr` と `a:CT_GroupTransform2D/a:xfrm` は ADR §6 の allowlist に入っている。

**推奨.** `p:grpSp` なら `p:grpSpPr` を見る。`chOff`/`chExt` を返すかどうかは `slide_rect`（M1）と一緒に決めればよい。

### m3. group-winner 属性（r2-m1）の `defaulted` と、不正な値の扱い

**根拠.**

- `resolve.rs:623`: `defaulted` は rot、flipH、flipV がすべてないときにだけ true になる。`rot` だけがあって `flipH` がない場合は false になるが、`flip_h` の値は実際には既定値である。
- `:968`: `pitchFamily`/`charset` も同じ作りになっている。
- 不正な値は黙って既定値に置き換わる。§1.3 は「first element that sets the property decides, including when its value is invalid (returned as `AttrError`)」、ADR-0002 §1.2 は「never lossy」としている。該当するのは次の箇所である。
  - `rot`/`flipH`/`flipV`（`:617`）
  - `pitchFamily`/`charset`（`:966`）
  - `startAt`（`:1108`）
- プローブでの確認:
  - P7: `<a:xfrm rot="abc">` → `Ok(Xfrm{ rot: 0 … }), defaulted: false`
  - P9: `pitchFamily="zz"` → `Ok(Font{ pitch_family: 0 }), defaulted: false`
- `AttrError` の raw も失われている。
  - `:1095`: spacing は `attr_error("val", "")`。
  - geometry の数値エラーは `attr_error("xfrm", "off/ext")`。

**推奨.**

- `defaulted` を属性ごとに持つ。たとえば `Xfrm { rot: Result<i32, AttrError>, … }` にし、既定値を使ったかどうかを示すビットを付ける。
- 不正な値は `AttrError` にして raw を残す。

### m4. `mc:AlternateContent` の扱いが §3.4 の規則と一致していない箇所が 2 つある

**根拠.**

- `deckread.rs:808–812` の `timing()` について。
  - doc コメントは「An `mc:AlternateContent` around `p:timing` follows the transition branch rule」と書いている（ADR §3.5 も同じ）。
  - しかし実装は、`Requires` を見ずに、最初に `p:timing` を持つ分岐を返す。
  - プローブ P3: `Requires="zz"`（`urn:unknown`）の Choice に exit、Fallback に entr を置くと、`exit` が返った。コーパスには 0 件。
- `deckread.rs:242` の `shape_action()` について。
  - `c_nv_pr(shape.raw())` は、AC のエントリでは `mc:AlternateContent` の子を探すため、何も見つからない。
  - プローブ P2: AC で包んだ図形の `hlinkClick` について、`shape_action` は `None`、`links()` は 0 件を返した。
  - リゾルバ側は `representative()` を使っているので、扱いが食い違っている。
  - 同じく、layout の AC 内にあるプレースホルダーは `placeholders()` から見えない。

**推奨.**

- `timing()` は `transition_of` と同じ選択関数（宣言されている名前空間 URI で判定する）を使う。
- `shape_action` は `ac_representative` を通す。

### m5. コメントのアンカー分類器が `refs.rs` と別に実装され、すでに結果が食い違う

**根拠.**

- ADR §3.3 は「The anchor classifier is the one Phase 0 uses for locks (`refs.rs`), so reading and locking cannot disagree」としている。しかし `deckread.rs:430` の `classify_anchor` は独自に実装されている。
- 例として、`pc:` 名前空間で `pc:sldMkLst` 以外のアンカーが `ac:@id` を持たない場合を考える。
  - 読み取り側は `Anchor::Slide` と判定する。
  - ロック側（`refs.rs` `scan_comments`）は「safe ではない」と判定し、id もないので全図形をロックする。
- 現行のコーパスには該当するものがないが、ADR が避けたかった種類の食い違いである。

**推奨.** `refs.rs` に分類関数（safe / ids / text）を 1 つ置き、読み取りとロックの両方から使う。

### m6. ADR §7 O2 の synthetic デッキと手動確認の材料について、PR 本文と試験の対応が追いにくい

**根拠.**

- `s7_script_dependent_fonts` は、theme に `<a:ea typeface=""/>` があるかどうかで assert を切り替えている。そのため、fixture が変わると何も試さなくなる（今は空の分岐を通ることを Y9 で確認した）。
- O2b の POI 差分表は `poi_known_differences.md` としてコミットされている。一方 `geometry_known_differences.json` の 1 件は roadmap fixture のパスで記録されているので、コーパスで実行すると「no longer differs」と出る（CI では問題ない）。

**推奨.**

- `s7` は、theme を `ea=""` に書き換えてから試す形にし、条件分岐をなくす。
- 既知の差の表には、コーパスで実行した場合の扱いを一言書いておく。

### Nit

- `match_layout` step 3/4 の `ambiguous`: idx 0 を優先して 1 つに決まった場合でも `by_type.len() > 1` で true になる（`resolve.rs:323`）。ADR の「equal candidates」より広く true にしている。
- master 由来の値の `Source::Placeholder.matched` は常に `MasterFamily` になる（`:488`、`:570`、`:594`）。そのため orphan や `IdxFamily` が値の側からは見えない。ADR の「Every resolved value from a layout or master placeholder reports the `MatchKind`」と少しずれている。
- `let _ = ids;`（`deckread.rs:333`）と `let _ = para;`（`resolve.rs:463`）は不要な残りである。
- 性能:
  - `placeholder_match` は、ランごとに layout と master の全プレースホルダーを走査し直している。
  - `Deck::part` は `part_names()` を線形に探している。
  - コーパス全体で 9 秒（debug）なので、今は問題ない。
- `theme_font` は、長さ 6 で `+` から始まる任意の文字列を minor/cs として扱う（`+xx-yy` など）。
- `Transition::kind_attributes` は属性の名前空間を落としている（`p14:dir` と `dir` を区別できない）。
- `Requires=""` は「すべて理解した」と扱われる（`all()` が空集合で true）。

---

## 判定

- **converged: no**（Blocker 0、Major 2: M1、M2。Minor 6: m1–m6）。
- リゾルバと読み取り API の**値**には誤りが見つからなかった（独立差分 0、python-pptx 差 0、spike の 76 カウンタはすべて一致）。M1 と M2 は API の範囲と形の問題で、どちらも小さな追加で閉じられる。
  - M1 は、`is_text_box`、Q4 の helper、`xsd_default` を実装し、残りを後回しとして明記すれば閉じる。
  - M2 は、`EnumValue` に置き換えれば閉じる。owner が生文字列を承認する場合も閉じる。
- round 2 では M1、M2 と m1 の試験追加を確認し、レビュアー作の mutant Y5、Y6、Y11、Y14、Y16、Y17、Y18 を再実行する。
