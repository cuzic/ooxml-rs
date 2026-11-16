# ADR-0001 レビュー（Opus, round 2）

対象: `docs/adr/0001-lossless-round-trip-architecture.md` revision 2
前提: round 1 の B1〜B4 / M1〜M3 / m1〜m3 が反映済みかを確認し、改訂によって新しく生じた問題を探した。

## 総評

round 1 の指摘はほぼすべて実質的に反映されている。特に、append-only 不変条件、`inlineStr` 限定、formula セルは Err、パート状態の3値化、保証単位を (name, method, CRC32, payload) に絞った点、Alternatives 1b/5 の追加は、設計としても文書としても良くなった。MSRV 1.88 の修正も workspace `Cargo.toml` で確認した。

**ただしまだ収束していない。** 改訂で導入した Layer 2 のシリアライズ規則と Phase0 スコープの間に、コードを書き始めた初日に踏む矛盾が3件ある（R2-B1〜B3）。いずれも ADR の文言修正で解消できる規模で、アーキテクチャを再考する必要はない。

---

## R2-B1 [Blocker] `add_worksheet()` が Phase0 の範囲内と範囲外の両方に書かれている

### 根拠
- L252-255: 「Adding a new worksheet, or any other edit that requires a new OPC part, is out of scope for Phase0」
- L285-286 / L319-321: from-scratch の Phase0 API は「`add_worksheet()` + `write_string()`/`write_number()`」
- L277-284: from-scratch は「埋め込みテンプレートを `Package::open` し、編集と**同じコードパス**を使う」

### なぜ問題か
from-scratch が編集パスと同一であると宣言した以上、`add_worksheet()` は「既存パッケージへのシート追加」そのものになる。シート追加には `[Content_Types].xml` の Override、`workbook.xml.rels` の rId、`workbook.xml` の `<sheet>`、`docProps/app.xml` の `TitlesOfParts`/`HeadingPairs` の更新が必要で、これを ADR 自身が「real work, not yet designed」として範囲外にしている。実装者はどちらかの記述を破るしかない。

### 改善の方向性
次のどちらかを ADR に書く。
- (a) テンプレートに `sheet1` を1枚入れておき、Phase0 の `add_worksheet()` は「1回目はテンプレートの sheet1 を返す。2回目以降と、名前を指定した呼び出しは `Err`」とする。最小限の変更で済む。
- (b) シート追加（= Layer 3 の OPC レジストリ）を Phase0 に入れる。その場合は B1 で範囲外にした作業の設計が必要になる。

(a) を推奨する。シート名の変更も `workbook.xml` と `app.xml` を触るため、Phase0 では同様に Err とすること。

---

## R2-B2 [Blocker] シリアライズ規則が、空要素タグ（`<x/>`）に子を足すと壊れた XML を出力する

### 根拠
- L126-128: `span` は「要素全体（open tag + children + close tag）」の1区間だけ。
- L153-156: 「`!own_dirty && children_dirty` のとき、このノード自身の open/close タグの span を verbatim に保ち、子を再帰」
- L149-152: 「`own_dirty && !children_dirty` のとき、children の combined original span を verbatim」

### なぜ問題か
1. **データ構造が規則を支えていない。** 「open タグの span」「close タグの span」「子の combined span」は、要素全体の1区間からは導出できない。子が0個の場合や、先頭の子が text の場合に境界が決まらないからだ。open タグの終端と close タグの開始を、別々のオフセットとして持つ必要がある。
2. **空要素タグで壊れる。** Excel が書く xlsx には、書式だけ付いた値なしセル `<c r="B2" s="3"/>` が大量にある。ここに数値を書くと、`t` 属性は不要なので `own_dirty` は立たず、`<v>` 子が増えるので `children_dirty` だけが立つ。規則どおりに「open タグ verbatim + 子 + close タグ verbatim」を出すと、`<c r="B2" s="3"/><v>5</v>` となり、close タグは存在しない。結果は **well-formed でない XML** で、Excel は開けない。
3. **from-scratch パスは必ずこれを踏む。** 最小テンプレートの sheet1 は、ほぼ確実に `<sheetData/>` を含む。最初の `write_number()` が上記のケースそのものになる。

### 改善の方向性
- `span` を `{ start, open_end, close_start, end }` の4点にする。空要素タグでは `open_end == end` とし、`close_start` は「無い」と表現する。text/comment の子も自分の span を持たせる。
- 規則に1行足す: 「元が空要素タグで、子が1つ以上になった場合は `own_dirty` 扱いにして、open/close タグを再レンダリングする」。子がすべて消えた場合に `<x></x>` のまま出すか `<x/>` に戻すかも決めておく。どちらでも valid。
- T2 の fixture に `<c r=".." s=".."/>` と `<sheetData/>` を必ず含める（現 fixture のカバレッジ一覧 L356-358 にも追加する）。

---

## R2-B3 [Blocker] `children_dirty` が2つの異なる意味で使われている

### 根拠
- L131-134 の定義: 「子リストが変わった（insert/remove/reorder）。**個々の子が dirty かどうかとは独立**」
- L140-142 の規則: 「transitively, no descendant is dirty — **tracked by `children_dirty` at every ancestor on the path**」

### なぜ問題か
孫 `<v>` のテキストだけを変えた場合を考える（`<c>` の子リストは不変、`<v>` が `own_dirty`）。定義に従えば `<c>.children_dirty = false` で、規則の1行目「`!own_dirty && !children_dirty` → span verbatim」に該当し、**編集内容が出力されない**。規則の括弧書きに従えば `<c>.children_dirty = true` になるが、定義と矛盾する。さらに `own_dirty && !children_dirty` のケース（L149-152）は「子の combined span を verbatim」なので、親の属性と子の値を同時に変えると（`t="s"` → `t="n"` で `<v>` も変える、というごく普通の編集）、子の変更が捨てられる。

### 改善の方向性
フラグを用途で分ける。
- `own_dirty`: 自分の name/attrs が変わった、または新規作成された → open/close タグを再レンダリングする
- `subtree_dirty`: 自分または子孫のどこかが dirty → false なら span 全体を verbatim で出す。true なら再帰する

子リストの変更は「親の `subtree_dirty` を立てて再帰する」だけで処理できる。変更されていない子はそれぞれ `subtree_dirty = false` なので verbatim で出る。したがって3つ目のフラグは不要。祖先への伝播は `cell_mut` の書き込み時に経路上で行う、と書く。

---

## R2-M1 [Major] T1 は自明に通るので、パーサの忠実度を何も検証しない

### 根拠
L167-170: 「parse → 編集ゼロ → serialize → 入力と byte 一致」

### なぜ問題か
dirty が1つも無ければ、ルートの span を丸ごと出すだけになる。パーサが属性を落としても、`<extLst>` を読み飛ばしても、名前空間を取り違えても、T1 は通る。T1 が検証しているのは「ルート span の切り出し」と「XML 宣言・BOM・末尾改行がルート span の外にあっても保持されているか」だけだ（後者は ADR に書かれていないので明記すること）。

### 改善の方向性
- T1 はそのまま残し、prolog/epilog の保持テストと位置づける。
- **T1b を追加する**: 全ノードに `own_dirty`/`subtree_dirty` を強制して再レンダリングし、その結果を再パースしたツリーが元ツリーと等価であることを確認する。等価の判定は name・attrs の順序・子の順序・text で行う。こちらがパーサとレンダラの正しさを検証する本体になる。
- **T2 を全セルに対して回す**: fixture のすべてのセルについて「同じ値を書き戻す」「別の値を書く」を行い、毎回 diff の局所性を検査する。1セルだけでは、空要素タグや `t` の変化といった分岐をほとんど通らない。

---

## R2-M2 [Major] T2 の「diff はその `<c>` の span 内」は、新規セル・新規行のケースで定義できない

### 根拠
L171-175

### なぜ問題か
`cell_mut` が新しく作った `<c>` には元の span が無い。行自体が無ければ `<row>` も新規になる。どちらの場合も diff は「既存の兄弟ノードの間への挿入」であり、「要素の span 内」という判定条件が適用できない。

### 改善の方向性
T2 の判定を「diff が、編集したノードの元 span（新規なら挿入点）と `<dimension>` の span の和集合に収まる。さらに挿入点は、行順・列順として正しい位置にある」と定義し直す。

---

## R2-M3 [Major] 数式が参照する値を編集すると、キャッシュ値が古いまま黙って残る

### 根拠
round 1 B1 の表にあった「参照元の書き換え → `fullCalcOnLoad`」が、revision 2 の Layer 3 Phase0 policy（L232-255）から抜け落ちている。Phase0 が Err にするのは「`<f>` を持つセル自身」の編集だけ（L198-200）。

### なぜ問題か
`Sheet1!A1` を 10 → 20 に書き換えても、`Sheet2!B1 = Sheet1!A1*2` の `<v>20</v>` は残る。Sheet2 は Layer 1 で raw copy されるからだ。Excel は `calcId` が自分のバージョンと一致するファイルでは、開いたときに全再計算をしない場合がある。calamine・pandas・ooxml-rs 自身の `cell()` はキャッシュ値を読むので、**エラーなしに古い値を返す**。これは ADR が掲げる「黙って壊すより正直に失敗する」（L202-204）にちょうど反するケースだ。

### 改善の方向性
次のいずれかを Phase0 policy に明記する。
- (a) 「ワークブック内に `<f>` が1つでもある場合（`calcChain.xml` の有無で近似できる）は、値の編集を Err にする」
- (b) `workbook.xml` の `<calcPr>` に `fullCalcOnLoad="1"` を立てる。ただし `workbook.xml` が Layer 2 の編集対象になり、「生ツリーは sheet1.xml のみ」（L311-312）の範囲が広がる
- (c) 既知の制約として Consequences に「数式から参照されるセルの編集では、他セルのキャッシュ値が更新されない」と書く

Phase0 の狭さからすると (a) か (c) が妥当。黙って (c) の状態にするのだけは避けること。

---

## R2-M4 [Major] 「同じパスへ上書き保存」と raw copy の関係が未定義

### 根拠
L74-79, L78-82: `Raw`/`Parsed` パートは保存時に元エントリから `raw_copy_file` する。

### なぜ問題か
最も典型的な使い方は `Package::open("book.xlsx")` → 編集 → `save("book.xlsx")` だ。`Package` が元ファイルの `ZipArchive<File>` を保持したまま、同じパスを書き込み用に truncate して開くと、raw copy の読み出し元が先に消えて、**元ファイルも出力ファイルも壊れる**。

### 改善の方向性
次のどちらかを ADR に1行書く。
- `open` は元ファイル全体を `Vec<u8>` としてメモリに読み込む
- `save` は一時ファイルに書いてから rename する

両方行うのがよい。後者は保存途中のクラッシュでも元ファイルを残せる。

---

## R2-M5 [Major] 「Excel が開けるか」を確かめるオラクルが無い

### 根拠
L238-241 は「`inlineStr` は Excel が natively 受け付ける」、L188-189 は「`<dimension>` を直さないと Excel が文句を言う」と、Excel の挙動を前提にしている。一方でテストは T1/T2 の byte オラクルだけで、サンドボックスに Excel も LibreOffice も無い（L354-355）。

### なぜ問題か
T1/T2 は「意図した箇所しか変えていない」ことを保証するが、「変えた箇所が正しい」ことは保証しない。たとえば `<c t="inlineStr">` に `<v>` を残してしまう、`<is><t>` に必要な `xml:space="preserve"` を付け忘れる、といったミスは T2 を通ってしまう。

### 改善の方向性
- 最低限、出力を calamine で読み戻して値が一致することを確認する dev-dependency テストを入れる。independent な実装による読み戻しになる。
- 可能なら、実機 Excel で開けることを確認するステップを Phase0 の完了条件に入れる。手動で、修復ダイアログが出ないことを見る。実機を用意できないなら、その旨を Consequences に明記する。
- 編集で書く `<t>` には、値の先頭・末尾に空白があれば `xml:space="preserve"` を付ける、と Layer 2 の text の項に追記する。

---

## R2-m1 [Minor] `cell_mut` を呼んだだけでノードが作られ、dirty になる

L184-185 の「find-or-create; only this path can set dirty」だと、`let c = ws.cell_mut(5,5)?;` の後に値を書かずに return しただけで、空の `<c>` が挿入され、パートが `Replaced` になる。ノードの作成は値を書いた時点まで遅らせる、と書くのがよい（entry API と同じ発想）。

## R2-m2 [Minor] テンプレート由来のメタデータ

埋め込みテンプレートの `docProps/core.xml`（`dcterms:created`、`dc:creator`）や `app.xml` の `Application` がそのまま出力されると、すべての新規ファイルが「テンプレート作成日時・テンプレート作成者」を持つことになる。Phase0 では core.xml をテンプレートに含めないか、作成時刻だけ更新するか、方針を決めておくこと。

## R2-m3 [Minor] Alternative 5 の事実主張に出典が無い

「ooxmlsdk の generic DOM-level 編集は research 時点で未公開」（L429-432）は、不採用理由の中心になっている事実主張なのに、Sources にはバージョンもリンクも無い。調査した ooxmlsdk のバージョンと、根拠にした箇所（README の該当節、issue 等）を Sources に追記すること。

## R2-m4 [Minor] 推敲

- L140-142 の括弧書きは R2-B3 の修正で不要になる。
- Context L41-54 の「revision の経緯」は ADR 本体よりも Status か Changelog 節に置く方が、後から読む人にとって決定内容と経緯の区別がつきやすい。ただし好みの範囲。

---

## 収束判定

**未収束。** R2-B1〜B3 はいずれも、Phase0 の受け入れテスト（from-scratch で数値を1つ書く、空要素タグのセルに書く、属性と値を同時に変える）で確実に顕在化する。ただし必要なのは文言とデータ構造定義の修正だけで、3層構成・append-only 不変条件・Phase0 の絞り方はこのままでよい。R2-B1〜B3 と R2-M3（黙って古い値が残る件）を直せば、round 3 で収束できる見込み。
