# PR #162 レビュー（Opus, round 1）

対象: impl/xlsx-ej-j0b2 @ 920a529a20545f4be9b721975242499f736c13c4

## 結論（先に）
converged: **yes**（Blocker 0 / Major 0 / Minor 10 / nit 3）。P の変更（`cols` の正準 run、`f` の属性、
`cm`/`vm` のブロック内容、関係による styles/sst/calcChain/metadata の特定）は ADR-0024 Test oracle 1 の
「Modeled」と除外表どおりで、oracle 1 が捕まえるべき差を P が隠す経路は見つからなかった。raw 値は
`Val::Raw` ではなく別フィールド `raw_value` だが、モデルに入っているので差は見える（同等）。P の panic
（`cm`/`vm`/`s`/sst の範囲外、styles が無いのに `s≠0`）はすべて「every index in range」述語の代わりで、
oracle の中では妥当。POI oracle は 25 fixture を実際に回しており（debug・16 cases で 3,658 ok / 1,055 refused、
24 s）、拒否の内訳も想定どおり（`MissingReference`、`MergedCell`、`NoSuchStyle`、`DuplicateCellWrite` など）。
A7 の判定（Vec of runs のまま）は数値の余裕が 2 桁以上あり、測り方の偏り（m8）を補正しても変わらない。
指摘は生成器・自己テストの穴と PR 本文の不足で、どれも follow-up でよい。

## 前ラウンドの解消状況
（round 1 のため、なし）

## 確認したこと
- `cargo test -p ooxml-xlsx --all-features --lib journal`: 71 passed / 3 ignored（debug、27 s）。
- 手で入れた変異（コミットしていない）:
  `f` の `ca`/`aca`/`other` を捨てる → `k_formula_attributes_are_values` と
  `meta_fixture_formula_without_ca_differs_from_the_base` が落ちる（PR の表どおり）。
  `col/@style` を解決せず生の index で比べる → **全テスト green**（m1）。
- POI 生成器の拒否の内訳を一時的な eprintln で数えた（上の結論）。`59746_NoRowNums` と `InlineString` は
  ほぼ全部 `MissingReference`（どちらも `r` の無いシート）。
- `origin/main`（C5 #160、P1 #161）を試しにマージ: 衝突なし、journal のテストはこの PR の分は green。
  ただし J0a の `oracle1::the_generator_hits_every_op_family_and_some_refusals` がマージ後の状態で
  10 回中 2 回、この PR の head 単体でも 8 回中 1 回落ちた（`shared` が 1〜2、閾値は 3）。`main` 単体では 14 回とも green。
  この test は seed を固定しない乱数で、この PR は oracle1.rs・Journal 本体に触れず、oracle1 が使う
  `base_cells` の結果も oracle1 の fixture では変わらない（raw 値が無い）ので、回数の差は標本の偶然と見る（下の「範囲外」）。

## Findings

### m1（Minor）: `col/@style` の解決がテストされていない（変異が生き残る）
- 根拠: `crates/ooxml-xlsx/src/journal/project.rs:1113-1115` は `styled(1)` と `styled(2)` が**違う**ことだけを見る。
  生の index で比べても違うので、`cols_model` の `styles.resolve` を外した変異（`project.rs:592-593`）は
  全テスト green。`cellXfs` は styles の Tree から外している（`project.rs:545`）ので、両側が同じ index に
  別内容の session `xf` を置いた場合、未解決の比較は差を隠す。
- 修正案: `custom_styles(true)` の内容が等しい 2 つの index（セルのテストで使っている 1 と 4）で
  `eq(&styled(1), &styled(4))` を (m) に足す。行の `s` にも同じ eq を 1 本。

### m2（Minor）: `cols_model` が不正な `col` を黙って正規化する
- 根拠: `project.rs:583-603`。`min > max` の `col` は範囲が空になって消え、`max > 16384` は切り詰め、
  重なる `col` は後勝ちで 1 本にまとまり、`min` の昇順違いも消える。どれも Excel が修復を求める形で、
  片側だけがそれを書いても P は等しいと言う。両側が同じ setter を通る J0 では差になりにくいが、
  B4 の「歴史に依らない形」を J1 の oracle 11 でも使うので、ここで落としておくと後で見えなくなる。
- 修正案: 正規化の前に、`min == 0`・`min > max`・`max > MAX_COLS`・重なり・順序違いを `odd` に入れる
  （または panic して「`cols` は妥当」述語の代わりにする）。

### m3（Minor）: POI 生成器の revert が revert にならない場合がある
- 根拠: `crates/ooxml-xlsx/src/journal/tests/poi.rs:352-366`。
  (a) inline string の Base セルは `CellCall::String` で戻すので、保存形が Base と違い、
  write-back の経路（`check_written_back`）に届かない（PR が挙げた「inline-string revert の特例が無い」）。
  (b) raw 値（`t="d"` など、`BaseCell.value = Empty`）と、値の無い `c` の revert は `RemoveFormula` になり、
  前の op が書いた値を消さない。
  POI では (a) の対象がほぼ無い（`InlineString.xlsx` は `r` が無くて全部拒否）ので実害は小さいが、
  oracle 1 の「values: reverts to the Base value」が POI では弱い。
- 修正案: `BaseCell` に保存形（`inline: bool`）を足し、revert で `StringInline` を使う。(b) は P1
  （`clear_value`、#161 で main 入り）を、J0b-1 が生成器に加えるとき（§8 の取り決め）に revert にも使う。
  それまでは doc コメントに「raw と空セルの revert は近似」と書く。

### m4（Minor）: `Where::Dim` が境界ではなく範囲内の一様乱数
- 根拠: `poi.rs:335-344`。`r % (mr + 2)`、`c % (mc + 2)` なので、行 `mr+1`・列 `mc+1`（Base の外側の 1 行・1 列）に
  当たる確率は大きい fixture ほど小さい。範囲も `dimension` 要素ではなくセルの最大値から取っている
  （`poi.rs:263-268`）。ADR の生成器は「boundaries（最後の行、…）」に寄せることを求めており、
  内側の一様な点は `Hot`/`Earlier` とほぼ重なる。J1 で `dimension` 述語が入ると、ここが主な入口になる。
- 修正案: `Dim` の半分を「最終行 / 最終行 + 1 / 最終列 / 最終列 + 1」の境界に固定し、残りを今の一様乱数にする。
  範囲は `dimension/@ref` があればそれを使う。

### m5（Minor）: fixture ごとの実行量を確かめていない
- 根拠: `poi.rs:545` は合計で `ok > 1000 && refused > 100` だけを見る。ある fixture で全 op が拒否される回帰
  （たとえば新しい guard の誤検出）は合計の閾値を割らずに通る。`59746_NoRowNums`（47 ok）と
  `InlineString`（41 ok）はすでにほぼ全部拒否で、これは想定どおりだが、他の 23 個も同じ状態になり得る。
- 修正案: `r` の無いシートしか持たない fixture を除き、fixture ごとに `ok >= cases()` 程度の下限を assert する
  （除外した名前は理由付きでリストにする）。

### m6（Minor）: branch が main より古い。C5 の `freeze_dimension` が測定に入っていない
- 根拠: `crates/ooxml-xlsx/src/journal/tests/measure.rs:211-214`。タスクは「C5 が入っていれば
  `freeze_dimension = true`」とし、C5 は #160 で main に入った。shadow と同じ設定で測っていない。
  A7 の (a) は 28 µs 対 約 2.5 s で余裕が 4 桁あるので、判定は変わらない。
- 修正案: `origin/main` をマージし（衝突なし）、`wb.freeze_dimension = true;` を足して `insert_rows_cost` を
  取り直し、本文の数値を差し替える。

### m7（Minor）: PR 本文に必要な項目が足りない
- 根拠: 本文にあるのは要約と 3 つの数値だけ。タスクの取り決めでは、マシン・`rustc -V`・profile をすべての数値に、
  debug の既定実行時間（今回の手元では 24 s）、変異の表（どのテストで落ちたか）、J0a の `append_cost` の表
  （10k と 100k 行）、`AxisVec` の delete・`id_at` の数値、コードに入れた判断（B4、B8、B11 など）と
  「Open questions」が要る。Open questions には実装者自身の逸脱（`Val::Raw` の代わりの `raw_value`、
  `BaseCell.group`、`pub(crate) mod poi`、`Dim` の置き方、inline revert、C5 未設定）を書くべき。
- 修正案: 上の項目を本文に足す。逸脱は本レビューの判定（m3, m4, m6, n3、`raw_value` は同等）を添えて載せる。

### m8（Minor）: `index_of` の数値は平均で、最悪ケースではない
- 根拠: `measure.rs:271-277`。`index_of` は run の線形走査で、見つかった時点で返る（`ids.rs:177-197`）。
  測っているのはランダムな位置の生存 id（ほとんど Base id）なので、平均は約 R/2 run の走査になる。
  最後の run の id や死んだ id（`None`、全 run を走査）はその約 2 倍。replay が死んだ id を引くこともある。
  A7 (b) の上限 50 µs に対して 4.2 µs × 2 程度なので判定は変わらないが、上限は 1 回あたりの値。
- 修正案: 最後の run の id と死んだ id の `index_of` も測って、最大値として本文に載せる。

### m9（Minor）: 「styles が無い」と「AC の style table」の P に自己テストが無い
- 根拠: `project.rs:271-295`。`StyleTable::Missing`（`s="0"` → `<no styles>`、それ以外は panic）と
  `StyleTable::Opaque`（生の `s`、styles の Tree を丸ごと保持）は、POI の `style-alternate-content.xlsx` の
  GetPut で「panic しない」ことしか確かめていない。Opaque で `s` が違うセルが違うと言うこと、
  Missing で `s="0"` が通り `s="1"` が panic することのテストが無い。
- 修正案: 自己テストを 2 本足す（Opaque: `s="1"` と `s="2"` は ne。Missing: `s="0"` は eq、`s="1"` は
  `#[should_panic]`）。

### m10（Minor）: metadata 除外の代わりの述語に、配置先が無い
- 根拠: ADR の除外表は metadata part について「Base の part は残る。Base に無ければ `set_dynamic_formula` が
  log にあるときだけ増える」を両側に課す。P は関係で part を丸ごと外すようになった（`project.rs:483-486`）ので、
  参照する `c` が無くなった Base の metadata part を片側が落としても見えない。Delivery 0 が J1 に回す
  save 側の述語の列挙（`dimension`、キャッシュ、`sst/@count`、calcChain）にこれは入っていない。
- 修正案: 安い検査なので `Pair::finish` に「Base に sheetMetadata 関係があれば両方の出力にもある」を足すか、
  少なくとも Open questions に「J1 で入れる」と書く。

### n1（nit）: 正規化が強すぎる小さな箇所
- `project.rs:777`: `t="b"` の `v` が `"1"`/`"true"` 以外なら何でも `false`（`"2"` と `"0"` が等しい）。
- `project.rs:198-203`: 数値でない `fontId` などを黙って 0 にする。
- `project.rs:590`、`695`、`790`: 属性 map のキーが local 名なので、名前空間付きの属性と無しの属性が同じキーになる。
- 修正案: パースできない値は raw のまま持つ（`raw_value` と同じ扱い）。キーは `{ns}local` にする。

### n2（nit）: `E5` のテストが差の場所を確かめていない
- `poi.rs:600`: `is_some()` だけなので、`ca` 以外の理由で P が変わっても通る。
- 修正案: `first_diff` が `cell/4,4` で始まり、Base 側に `ca: true` を含むことを assert する。

### n3（nit）: `pub(crate) mod poi`
- `tests/mod.rs:9`: 公開するのは J0b-1 が `fixtures()` を使う場合だけ必要。使わないなら `mod poi;` に戻す。
  使うならその旨を doc コメントに書く。

## 範囲外（この PR の指摘ではない）
- J0a の `oracle1::the_generator_hits_every_op_family_and_some_refusals` は seed を固定しない `TestRunner::new` で
  200 ケースを引き、`shared >= 3` を要求する。手元で計 32 回中 3 回（`shared` が 1〜2）落ちた。
  seed を固定する（`TestRunner::deterministic()` など）か、閾値を下げる follow-up を J0b-1 側に。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 10 / nit 3）
