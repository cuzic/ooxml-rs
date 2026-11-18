# PR #56 レビュー（Opus, round 2）

対象: `impl/pptx-phase1-resolver` @ `cbb95ca`（round-1 指摘への対応コミット）。
基準: ADR-0008 rev 3 と post-acceptance note（2026-11-16）、owner 決定 Q1–Q5、ADR-0002 の owner 決定 Q2（`EnumValue`）、[round 1](pr56-round1.md)。
プローブ: `git clone` したコピーで実行した（`/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr56-r2/`）。worktree は変更していない（このファイルを除く）。

## 結論（先に）

- **round 1 の Major 2 件は解消した。** Blocker と Major はもうない。
  - M1: `is_text_box`、Q4 helper、`xsd_default`、`slide_rect` と `TransformedGroup` が入った。残りの後回しは ADR-0008 の post-acceptance note に一つずつ書かれている。§1.7 のノート/レイアウト/マスター文脈とまとめて notes PR へ回すのは筋が通っており、妥当と判断する。
  - M2: round 1 で挙げた列挙属性は、すべて生成型の `EnumValue` になった。未知の値は `Unknown(raw)` で残る。
- **リゾルバと読み取り API の値は、引き続き誤りが見つからない。**
  - 独立 Python ミニリゾルバ: 12,483 ラン × 6 属性で不一致 0。
  - spike の 76 カウンタ: 差 0。
  - python-pptx: 差は ADR が予告した 6 件だけ（あとは既知差 1 件の鍵が古いという報告）。links 172 件で失敗 0。
  - 読み取り API のカウンタ: round 1 と完全に同一。読んでもパッケージは汚れない。
- **`click_index` の修正（clickEffect を数える）は正しい。** PowerPoint の構造（mainSeq 直下の `p:par` 1 つが 1 クリック）から独立に計算した参照と、コーパスの全 299 効果で一致した。ただし「PowerPoint は `clickPar` を書かない」という説明は事実と違う（nit）。
- round 1 の mutant 18 個はすべて死んだ。新しく 26 個を作ったところ、13 個が生き残った。どれも新しく入ったコードの試験の穴である（r2-m2）。
- Minor が 3 件ある。
  - r2-m1: M2 の残り。`Outline::Preset` と `Color::theme_slot` が生文字列のまま。
  - r2-m2: 新コードの試験の穴。
  - r2-m3: `slide_rect` が、グループの不正な変換属性を黙って恒等変換として扱う。
- どれも小さく、マージ後の follow-up でも閉じられる。

**判定: converged = yes**（Blocker 0、Major 0、Minor 3、Nit 数件）。

---

## 検証したこと

| 項目 | 方法 | 結果 |
|---|---|---|
| ビルドと試験 | `cargo test -p ooxml-pptx`、`clippy --all-targets -D warnings`、`cargo xtask codegen --check` | すべて通過。codegen は「7 generated files up to date」 |
| spike モデル | `OOXML_PPTX_CORPUS=…` で `corpus_matches_the_spike_model` を実行（633 ファイル） | **76 counters compared, 0 differ** |
| 独立差分 | round 1 のミニリゾルバ `mini.py` を全デッキで再実行し、Rust 側 `rv_dump`（EnumValue に合わせて `cap` を `as_str()` に変えただけ）と比べた | **12,483 ラン × `sz`/`b`/`cap`/fill/`latin`/`ea` で不一致 0** |
| python-pptx（O1/O3） | `rv_geo` の出力をコーパス全体で `rc.py` にかけた | 1,272 placeholders のうち差は 6 件で、round 1 と同じ ADR 予告分。あとは既知差の鍵が古いという報告 1 件（m6 で文書化済み）。**links 172、失敗 0** |
| 読み取り API（O6） | `rv_feat` の数を round 1 の出力と比べた | **完全に同一**（sections、comments、transition の分岐、timing、links、`color_rule`）。`DIRTY_AFTER_READ` は 0 |
| `click_index` | `rv_click`（Rust）と `clickref.py` を比べた。`clickref.py` は、mainSeq 直下の `p:par` のうち開始条件が `delay="indefinite"` で `evt="onBegin"` がないものを 1 クリックとして数える、構造からの独立な参照 | **299 効果すべてで一致**（Some(0) 118、Some(1) 40、Some(2) 42、None 38 ほか） |
| 合成プローブ | `rv_syn`（round 1 の P1/P2/P3/P7/P9）と `rv_syn2`（新規 N1–N8） | 下の各項目に記載 |
| mutant | round 1 の Y1–Y18 と新規 Z1–Z27（`--lib --test resolve --test deckread --test oracle`） | Y は 18/18 が死んだ。Z は 13/26 が死に、13 個が生き残った（r2-m2） |

---

## 解消状況

### M1（ADR / owner 決定の API）: 解消

- **`ShapeRef::is_text_box()`（Q1）.** `p:cNvSpPr@txBox` を読み、`parse_bool` で `"1"` と `"true"` を受ける。AC のエントリでは代表を見る（N4: AC で包んだ txBox → `true`）。グループや表は `false`。試験は Z13 で死ぬ。
- **Q4 helper.** `RunProps::script_font_candidate(&slot)` と `script_for_lang`。
  - 戻り値は `Option<(script, typeface)>` である。`Lookup` は書き換えないので、**`Set` になる経路はない**。
  - N2 の結果:
    - `lang="ja-JP" altLang="en-US"` の `ea` → `("Jpan", "ＭＳ Ｐゴシック")`
    - `lang="en-US" altLang="ko-KR"` → `("Hang", "맑은 고딕")`
  - ADR の対応表（`ja`→`Jpan`、`ko`→`Hang`、`zh-CN`→`Hans`、`zh-TW`/`zh-HK`→`Hant`）を満たす。`zh-MO`/`zh-Hant`→`Hant`、`zh`/`zh-SG`→`Hans` も妥当。
  - doc とのずれは Nit に書いた。
- **`xsd_default`.** 生成済みの `ElementSpec::ATTRS` の default を引く。N3 の結果:
  - rPr: `cap`→`"none"`、`dirty`→`"true"`。`u`/`b`/`kumimoji`/`baseline` は `None`。
  - bodyPr: `upright`→`"false"`。`anchor`/`wrap`/`vert`/`lIns` は `None`。
  - pPr: すべて `None`。
  - これはベンダリングした Transitional XSD のとおりである（ADR §1.3 の「leak しうる属性」の列挙と一致する）。`PropertyId` ではなく属性名で引く形への変更は、post-acceptance note に記録されている。
- **`slide_rect` と `Error::TransformedGroup`.** N1 の結果:
  - 2 段の入れ子グループ（外側 ×2、内側 ×0.5）で `(10,10,10,10)` → `(1210,1210,10,10)`。手計算と一致する。
  - グループ自身の rect も正しい。
  - `flipV="true"` → `Err(TransformedGroup { group_id: Some(84) })`。
  - `rot="21600000"`（360°）は回転なしとして通る。
  - 不正な値の扱いは r2-m3 に書いた。
- **後回し**（`Deck::layouts`/`masters`、ノート/レイアウト/マスターの解決文脈、ノート・レイアウト・マスター上のリンク、S8、O2b の JVM 実行）について。
  - ADR-0008 の post-acceptance note に一つずつ挙がっている。後回し先のブランチ `impl/pptx-phase1-notes` も実在する。
  - どれも §1.7 の文脈（ノート/レイアウト/マスターを「読む側」の連鎖）が前提なので、notes PR とまとめるのは設計上も自然である。
  - コーパスへの影響は小さい（レイアウト/マスター上のリンクは 5 件）。
  - **受け入れ可能**と判断する。round 1 は「owner の了承を取る」と書いた。その了承は、ADR の note を含むこの PR のマージをもって記録される、と理解してよい。

### M2（列挙値の型）: 解消（残りは r2-m1）

- 次の属性が `EnumValue<'a, 生成型>` になった。
  - `algn`、`u`、`strike`、`cap`、`anchor`、`wrap`、`vert`
  - `buAutoNum@type`
  - transition `spd`
  - `presetClass`、`nodeType`、`bldP@build`
  - p188 `status`
- N5 の結果:
  - `u="zz"` → `Unknown("zz")`
  - `cap="small"` → `Known(Small)`
  - transition `spd="slow"` → `Known(Slow)`
- 未知の値は raw のまま残る。

### m1（試験の穴）: 解消

- round 1 で生き残った Y5、Y6、Y11、Y14、Y16、Y17、Y18 は、すべて CI の試験で死ぬ。Y11、Y15、Y17 はコードが変わったので、パターンを合わせ直して再実行した。
- S2（レベル 0–8、`lvl="9"`、欠けたレベル）、S3（タイ、idx なし、family、orphan、`4294967295`、idx-0 優先）、S4、`themeOverride`、`fontRef idx="major"` の試験が入った。
- 合成デッキの作り方の変更（`build_phase1.py` を使わず、メモリ上で XML を書き換える）は、post-acceptance note に記録された。
- **`click_index` の修正の検証.**
  - 旧実装は `nodeType="clickPar"` の `cTn` を数えていた。新実装は、mainSeq の中で `presetClass` を持つ `nodeType="clickEffect"` が次のクリックを始めるとし、`withEffect`/`afterEffect` は現在のクリックに入れる。最初のクリックより前の効果と `interactiveSeq`（トリガー）の効果は `None` にする。
  - PowerPoint の意味論について。
    - mainSeq の `childTnLst` の直下にある `p:par` 1 つが 1 回の「クリック」（ビルドの 1 段）にあたる。
    - その開始条件は `<p:cond delay="indefinite"/>`（次のクリックを待つ）である。
    - 先頭が「直前の動作の後/と同時」の場合は `evt="onBegin"` が付き、クリックなしで始まる。
    - その段の最初の効果の `nodeType` は、前者なら `clickEffect`、後者なら `afterEffect`/`withEffect` になる（コーパスの集計: クリック段 150 件はすべて先頭が `clickEffect`。自動開始の 21 件は先頭が `afterEffect` 9 件、`withEffect` 12 件）。
  - したがって「`clickEffect` を数える」は構造による定義と一致する。コーパス全 299 効果で、構造から独立に計算した参照と差 0 だった。
  - 旧実装の欠陥（ほとんどのデッキで `None`）は本物で、この修正は正しい。
  - 説明の誤りは Nit に書いた。

### m2（グループのジオメトリ）: 解消

- P1 の結果: `p:grpSp` → `Set(Xfrm{10,20,30,40})`。グループ自身の `rot`/`flip*` も `WinnerAttr` で返る。
- Z7（`grpSpPr` → `spPr`）は死ぬ。

### m3（`WinnerAttr` と `AttrError` の raw）: 解消

- 結果:
  - P7: `rot="abc"` → `rot: WinnerAttr { Err(raw "abc"), defaulted: false }`。`flipH`/`flipV` は `defaulted: true`。
  - P9: `pitchFamily="zz"` → `Err(raw "zz")`、`charset` → `Ok(1), defaulted: true`。
  - 試験では、spacing `val="x1"` と `startAt="q"` の raw が保たれている。
- `Resolved::defaulted` は、図形自身の autofit scale だけに使うよう意味が絞られ、doc にも書かれた。
- Z11 と Z12 は死ぬ。Z24（座標の raw を落とす）は生き残る（r2-m2）。

### m4（MCE の扱い）: 解消

- `timing()` と `transition_of()` は、同じ `chosen()` と `understood()` を使う。
  - P3 の結果: `Requires="zz"`（未知の URI）の Choice には exit、Fallback には entr を置くと、`[entr]` が返った。
  - N8 の結果: `Requires=""` の Choice は理解しないものとして扱われ、Fallback の `wipe` が返った。
- `shape_action` は AC のエントリの代表を見る（P2: `Jump(Next)`、`links()` は 1 件）。
- レイアウトの AC 内のプレースホルダーも見えるようになった。ただしこれを守る試験はない（Z6 が生存、r2-m2）。
- 参考: AC で包まれた `p:sp` のテキスト内のランリンクは、`text_body()` が AC では `None` を返すので列挙されない。Phase 0 と同じ扱いで、コーパスにも 0 件（Choice 内の `p:sp` は 1 件だけで、テキストはない）。対応は不要とする。

### m5（アンカー分類器）: 解消

- `refs::classify_anchor` が 1 つあり、ロック側（`scan_comments`）と読み取り側（`Comment::anchor`）の両方から使われる。
- id のない `pc:sldLayoutMkLst` について、読み取りは `Other` を返し、ロックは全図形をロックする。試験もある。Z18 は死ぬ。

### m6: 解消

- `s7` は theme を書き換えてから assert する形になり、条件分岐がなくなった。
- `resolve_check.py` には、コーパスで実行したときに既知差の鍵がどう扱われるかが書かれた。

### Nit（round 1）: 解消

- `ambiguous` は、等しい候補の数で決まるようになった。Z20 と Z27 は死ぬ。
- master 由来の値は、スライドのプレースホルダー自身の `MatchKind` を返すようになった。ただし試験はない（Z19 が生存）。
- theme フォント参照は 6 種の完全一致で判定するようになった。試験はない（Z21 が生存）。
- 空の `Requires` は理解しないものとして扱われる。Z3 は死ぬ。
- `kind_attributes` は名前空間を保つようになり、xmlns 宣言は除かれる。試験はない（Z22 が生存）。
- 不要なバインディングは除かれた。

---

## Findings

### r2-m1（Minor）. M2 の残り: `Outline::Preset` と `Color::theme_slot` が生文字列のまま

**根拠.**

- `resolve.rs:792` の `Outline::Preset(&'a str)` は `a:prstGeom@prst`（`ST_ShapeType`）である。生成型 `ooxml_schema::dml::ShapeTypeValues` はすでにある。
- `resolve.rs:832` の `Color::theme_slot: Option<&'a str>` は、カラーマップを通した後の slot（`dk1`…`folHlink`、`ST_ColorSchemeIndex`）である。生成型 `ColorSchemeIndexValues` がある。
- round 1 の M2 の理由（ADR-0002 Q2、後で直すと破壊的変更になる）は、そのままこの 2 つにも当てはまる。
- 一方 `Color::val` は、`kind` によって型が変わる（16 進、`ST_SchemeColorVal`、`ST_PresetColorVal`、`ST_SystemColorVal`）。raw のままでよい。`kind_attributes` も種類ごとに違う属性の集まりなので、raw でよい。

**推奨.**

- `Outline::Preset(EnumValue<'a, ShapeTypeValues>)` と `theme_slot: Option<EnumValue<'a, ColorSchemeIndexValues>>` にする。
- この PR で直すのがいちばん安い（公開 API がまだリリースされていないため）。

### r2-m2（Minor）. round-1 対応で入ったコードに、試験で守られていない規則がある

**根拠.** 新しく作った 26 個の mutant（`probes/mutants_r2.py`）のうち、次のものが CI で生き残った。

| mutant | 守られていない規則 |
|---|---|
| **Z1** `interactiveSeq` を mainSeq と同じに数える | トリガー効果の `click_index` が `None` になること |
| **Z2** 最初のクリックより前の効果を `Some(0)` にする | 自動開始の効果が `None` になること。コーパスには 38 件ある。試験 `click_index_counts_click_groups` は `filter_map(click_index)` してから比べるので、`None` と `Some(0)` を区別できない |
| **Z6** レイアウトの AC 内のプレースホルダーを無視する | m4 の 3 点目 |
| **Z9** `slide_rect` が `flipH`/`flipV` を見ない | `TransformedGroup` の flip の場合（試験は `rot` だけ） |
| **Z10** `slide_rect` が直近のグループしか合成しない | 入れ子のグループ（N1 で正しく動くことは確認した） |
| **Z19** master 由来の値の `matched` を `MasterFamily` に戻す | round-1 nit 2 |
| **Z21** theme フォント参照を「`+` で始まる長さ 6」に戻す | round-1 nit 5 |
| **Z22** `kind_attributes` の名前空間を落とす | round-1 nit 6 |
| **Z24** 座標の `AttrError` の raw を `""` にする | m3（`rot` と spacing の raw は試験されているが、座標の raw はされていない） |
| Z17 解決値の enum を常に `Unknown` にする | `Known` に写ることを、リゾルバの値として試していない |
| Z25 `rot="21600000"` を変換ありとして扱う / Z26 `zh-HK` → `Hans` | 小さな境界条件 |
| Z14 `altLang` と `lang` の優先順を入れ替える | 等価に近い（両方が Latin 以外で、かつ別の文字体系のときしか差が出ない）。対応しなくてよい |

**推奨.**

- 最低限 Z1、Z2、Z6、Z9、Z10、Z19 を殺す試験を足す。どれも 1 つの合成スライドで済む。`rv_syn2.rs` の N1 と N7 がそのまま使える。
  - N7 の期待値: `afterEffect`→`None`、`clickEffect`→`0`、`withEffect`→`0`、`clickEffect`→`1`、`interactiveSeq` の `clickEffect`→`None`。
- `click_index_counts_click_groups` は、`None` を含めた列そのものを比べる形にする。

### r2-m3（Minor）. `slide_rect` が、外側グループの不正な変換属性を黙って恒等変換として扱う

**根拠.**

- `resolve.rs:710–719` の動作は次のとおりである。
  - `rot` が数値として読めなければ 0 にする。
  - `flipH`/`flipV` が真偽値として読めなければ false にする。
  - `a:off`/`a:ext` が欠けているか読めなければ、そのグループを `continue` で飛ばす。
- N1 の結果: `<a:xfrm rot="abc" flipH="2">` のグループの中の図形について、`Ok(Some(Rect{5,5,1,1}))` が返った。
- 実際の変換が分からないのに、軸に平行な矩形を確定値として返している。ADR §1.3 と ADR-0002 §1.2（不正な値は `AttrError`、never lossy）と同じ方針を取るなら、ここは答えを出さないのが筋である。
- `chExt` が 0 のときに倍率を 1 にする扱いも、同じ種類の推測である。
- コーパスには該当がない。

**推奨.** 外側グループの `rot`/`flip*`/`off`/`ext`/`chOff`/`chExt` が不正なとき（`chExt` の 0 も含む）は、`Ok(None)` にするか、`AttrError` を運ぶ `Error` にする。doc の「`None` when …」にもその旨を足す。

### Nit

- **`clickPar` についての説明が事実と違う.**
  - ADR-0008 の post-acceptance note、`deckread.rs:780–782` の doc、PR 本文は、「PowerPoint writes no `clickPar`」「so it was always None」と書いている。
  - 実際には、コーパスの 8 スライド（`tdf169088`、`alterman_security`、`customGeo` ×6。いずれも Application は「Microsoft Office PowerPoint」）に、mainSeq 直下の `nodeType="clickPar"` が計 30 個ある。古い版の PowerPoint は書く。旧実装は、これらのデッキでは値を返していた。
  - 「PowerPoint does not always write `clickPar` (most files have none)」に直す。修正そのものは正しい。
- **`script_font_candidate` の doc とコードがずれている.**
  - doc は「`altLang` first for `ea`/`cs`」と書くが、関数はどの slot かを知らない。そのため `latin` slot が `ScriptDependent`（`bug64693` の `+mn-ea` 型）でも `altLang` を先に使う（N2: S71 の latin → `Hang`）。
  - doc を「for any slot」に直すか、slot ごとに選ぶ。
- **ADR §5 のスケッチが古いまま.** `click_index` のコメント「ordinal of the enclosing clickPar」（ADR l.722）は、post-acceptance note で上書きされている。それでも、スケッチ側に「see note」と一言あると読み手が迷わない。
- **PR 本文が古いまま.** 「Oracles」節の「Mutants: 18, all killed」は round 1 前の記述で、round 1 では 7 個が生き残っていた。後半の「Mutants after the fixes」と食い違うので、前者を消す。

---

## 判定

- **converged: yes**（Blocker 0、Major 0、Minor 3: r2-m1〜r2-m3、Nit 4）。
- round 1 の M1、M2、m1〜m6、Nit は、すべて意図どおりに解消された。
- 値の正しさは、3 つの独立した判定（ミニリゾルバ、python-pptx、spike カウンタ）と、新しく作った `click_index` の構造参照で、差 0 のままだった。
- r2-m1（2 つの型を `EnumValue` にする）は公開 API の形の問題なので、マージ前に直すことを勧める。数行で済む。
- r2-m2 と r2-m3 は、マージ後の follow-up でもよい。
