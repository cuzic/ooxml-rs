# ADR-0001 レビュー（Opus, round 3）

対象: `docs/adr/0001-lossless-round-trip-architecture.md` revision 3

## 総評

round 2 の Blocker 3件は正しく解消されている。
- `ElementSpan` の4点構造と「空要素タグが子を得る/失うと必ず `own_dirty`」規則によって、`<c r="B2" s="3"/>` と `<sheetData/>` のケースが well-formed になった。
- `own_dirty` / `subtree_dirty` の2フラグ化と、書き込み時に祖先へ伝播させる規則によって、孫ノードの編集が欠落しなくなった。
- `add_worksheet()` は「1回目はテンプレートの sheet1 を返し、以降は Err」とされ、範囲の矛盾が解消された。

T1/T1b/T2 の役割分担、in-memory open + 一時ファイル rename、`cell_mut` の entry 化、calamine による読み戻しも妥当。アーキテクチャとしては収束した。

残る指摘は Major 1件（安全性の主張が事実より強い）と Minor 4件。いずれも1〜2文の修正で済む。

---

## R3-M1 [Major] 「`calcChain.xml` が存在しなければ数式は無い」という判定は偽陰性を持つのに、「完全に塞ぐ」と主張している

### 根拠
L348-354:「if the package contains a `calcChain.xml` part at all, every value-editing call returns `Err` … it closes the silent-staleness gap completely」

### なぜ問題か
`calcChain.xml` は OOXML 上 optional なパートだ。数式を含むのに calcChain を書かない producer は普通にある。rust_xlsxwriter は calcChain を出力しない。openpyxl は保存時に calcChain を削除する。Excel 以外のツールや、ooxml-rs 自身の将来の出力も同様になり得る。

具体的な失敗シナリオ:
1. rust_xlsxwriter で作られたファイルがある。`Sheet2!B1 = Sheet1!A1*2` で、キャッシュ値 `<v>20</v>` を持ち、calcChain は無い。
2. ooxml-rs で開き、`Sheet1!A1` を 10 → 99 に編集する。calcChain が無いので Err にならない。
3. 保存すると、Sheet2 は raw copy されてキャッシュ値 20 が残る。calamine や pandas は黙って 20 を返す。

つまり R2-M3 で塞いだはずの「黙って古い値」が、calcChain を書かない producer のファイルでは開いたままになる。「completely」は誤り。

### 改善の方向性
Layer 1 を壊さない、安価で健全な判定に置き換える。
- **全ワークシートパートを（ツリーを作らずに）解凍して、quick-xml のイベントスキャンで `<f` 開始要素の有無だけを調べる。** 状態は `Raw` のまま変えず、保存時は raw copy のままにする。解凍は検査のためだけで、出力には影響しない。M2 の「parse != dirty」原則と同じ扱いになる。
- 判定結果は open 時に1回だけ計算してキャッシュする。
- calcChain の有無は高速パスとして残してよい（あれば即 Err）。ただし「無いから安全」とは判定しないこと。
- 文言は「closes the gap completely」から「すべてのワークシートパートに `<f>` 要素が1つも無い場合のみ値編集を許可する」に変える。Consequences L617-624 の「formula-free workbooks」という表現と、ちょうど一致する定義になる。

（補足: 定義名 `definedNames` やデータ検証の式はキャッシュ値を持たないので、この判定の対象外でよい。外部リンク `externalLinks/` のキャッシュは、他ブックへの参照なのでこの問題とは無関係。）

---

## R3-m1 [Minor] `core.xml` の更新が「Layer 2 のツリーは sheet1.xml のみ」と整合していない

L399-404 では `docProps/core.xml` を常に dirty として扱い、時刻を書き換えるとしている。しかし L439-440 では、生ツリーの対象は `sheet1.xml` のみとしている。どちらで実現するのかを1語足すこと。推奨は「`core.xml` は Layer 2 を通さず、`set_part` で丸ごと生成し直す（`Replaced`）」。テンプレート由来の内容なので保存すべき未知要素は無く、この方が単純。

## R3-m2 [Minor] L176-177 の事実誤認

「an empty templated sheet is nothing *but* self-closing `<c>` elements」とあるが、空のテンプレートシートには `<c>` は1つも無く、`<sheetData/>` があるだけだ。規則自体（空要素タグが子を得たら `own_dirty`）は `<sheetData/>` にも `<row>` にもそのまま適用されるので、設計への影響は無い。文言だけ「empty templated sheet is a self-closing `<sheetData/>`」に直すこと。

## R3-m3 [Minor] Phase0 の API と成功条件の語の不一致

- L443-445:「edit every string/number/boolean cell … via `inlineStr`」とあるが、`inlineStr` は文字列にしか使わない。また Phase0 API は `set_string`/`set_number`（`write_string`/`write_number`）だけで、boolean を書く API が無い。「string/number/boolean セルを、それぞれ string または number で上書きする（string は `inlineStr`）」のように、何をテストするかを正確に書くこと。
- boolean セル（`t="b"`）を number で上書きする場合は `t` 属性を削除する必要がある。`own_dirty` になるので規則上の問題は無いが、T2 の全セル走査で当たる分岐なので、テストケース名として明示しておくとよい。

## R3-m4 [Minor] 修正経緯の書き方

L364-369 では、R2-B1 について「it doesn't [contradict]」と書いている。しかし revision 2 の時点では「1回目はテンプレートの sheet を返す」という規則は書かれておらず、矛盾は実在した。revision 3 で規則を足したことで解消した、と書く方が正確で、後から読む人が誤解しない。

---

## 収束判定

**実質収束。** アーキテクチャ（3層構成、append-only 不変条件、`ElementSpan` + 2フラグのシリアライズ規則、T1/T1b/T2 + calamine の各オラクル、Phase0 の Err 境界）について、これ以上の構造的な指摘は無い。R3-M1 は安全性の主張に関わるので、数式検出を「全ワークシートパートの `<f>` スキャン」に置き換えたうえで確定すること。Minor 4件は文言修正のみで、round 4 のレビューは不要。
