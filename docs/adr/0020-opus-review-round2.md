# ADR-0020 レビュー（Opus, round 2）

対象: PR #145 `plan/ac-rank` @ `95b745f8306e5f0233aa042b1b1552335b753ce1`
（`docs/adr/0020-alternatecontent-rank.md` revision 2 と `spikes/ac-rank/`）。

照合したもの:

- round 1（`0020-opus-review-round1.md`）。
- revision 2 の差分（`1f6eed9..95b745f`）。
- `d066ff1` 上の次のコード:
  - format crate の位置指定の呼び出し: `insert_by` / `insert_in_slot` / `insert_at` / `move_in_by` / `insert_detached_by` と、その `check_*` / `_with_ns` 版。
  - `crates/ooxml-core/src/xml/node.rs` で子を追加・置換するすべての経路。
  - `lens.rs` の detach。
  - `package/add.rs` の `append_root_child`。
- PR #144 の現在の head（`62aab77`、未改修。§8.1 の順序どおり）。

## 結論（先に）

round 1 の M2、M3、M4 と m2、m3、m5 は解消された。M4 については、修正後の規則を確かめた。先頭に未登録の要素がある branch は、`lo = cur` として扱われる。このとき「may follow iff `lo ≥ s`」は健全である。`s = cur` の挿入は、付属する未登録要素より前に置かれても順序を破らない。

M1 は大筋で解消されたが、穴が 1 つ残る。§4.2 は「AC が slot `s` に入っている」場合だけを拒否する。一方、docx の呼び出し側は、どの slot にも入らない AC まで数える。対象は、全 branch が空の AC と、別の slot にある AC（`AC{w:pPr}`）である。このため、`insert_image` とハイパーリンクの挿入は、まだ黙って位置を誤りうる（M1）。

新しく足された公開関数 `check_append_among` は、なくても §9 の lift を 1 つも失わない（m3）。

spike は再実行して、コミット済みの結果と完全に一致した。**converged: no**（Major 1）。

## 前ラウンドの解消状況

- **M1（callback で位置を決める挿入）: 一部解消。** 位置指定の挿入は、同じ slot に AC があれば拒否し、末尾への追加だけを許す。この決定と §4.4 の一覧、呼び出しごとの byte-identical テストは入った。残る問題は 2 つ:
  - slot に入らない AC を数える呼び出し側が残っている（新しい M1）。
  - §4.4 の一覧に漏れがある（m1）。
- **M2（branch 内の書式の Clear）: 解消。** §9 に一般則が入り、docx はどの branch にも無い種類だけを lift する。Clear、既存の値の Set、属性の削除は拒否のまま残り、U3 のテストも決まっている。
- **M3（PR #144 との重複）: 解消。** §8.1 で、core の仕組みは U1 だけとし、#144 は公開 API を足さない。順序は U1 → #144 改修 → U4 → U2 → U3。#144 は今も `insert_after_alternate_content` を含むが、これは改修前なので順序どおりである。
- **M4（未登録の要素だけの branch）: 解消。**
  - 空でなく、登録された要素を 1 つも持たない branch は、AC を unranked にして拒否する。
  - 先頭に未登録の要素がある branch は `lo = cur` とし、前に兄弟が無ければ −1 とする。
  - oracle の mutation（e）もある。
- **m1（`insert_at` の局所検査）: 解消。** 候補 slot をすべて使って区間を読み、親のどこかに unranked / malformed / 同じ slot の AC があれば拒否する。新しい AC ビットについては m2 を参照。
- **m2（相関する AC）: 解消。** §3.2 に記述と表がある。
- **m3（spike と ADR の食い違い）: 解消。** unranked / malformed の拒否、スコープ内の `Requires` の検査、新しい counter が入った。
- **m4（`check_insertable` の委譲）: 形を変えて解消。** 公開関数を足すことになったので、その要否を新しい m3 で扱う。
- **m5（`mce_wrapped` の適用範囲）: 解消。** 適用範囲を限定し、xtask のテストを書き換えることも記された。

## 検証したこと（挙動）

- **spike の再実行。** xlsx は `/tmp/poi-xlsx`、docx / pptx は README の手順で取得した POI のファイル、加えて `reference/fixtures` を使った。出力は `results/ac-rank.txt` と**完全に一致**した。
  - 合計: 「末尾への追加で置ける」挿入が 256 / 411 / 41。
  - そのうち同じ slot に AC があるもの（`pair_ok_same_slot_ac`）が 21 / 411 / 6。
  - よって、位置指定でも置けるものは 235 / 0 / 35 で、ADR の表と一致する。
- **§4.4 が網羅しているか。** `d066ff1` で、format crate の非テストコードを grep した（86 箇所）。ADR に無いものは m1 に挙げた。それ以外はすべて一覧にある。
- **xlsx の「試し実行」を `check_insert` に切り替えてよいか。** 切り替える対象（`hyperlink.rs:199`、`range.rs:107/270/317/393/443`、`layout.rs:209/574/577/644`）について、実際に書き込む側を確かめた。どれも `get_or_insert` で書き込んでいる（`hyperlink.rs:230`、`range.rs:167/329/349/399/449`、`layout.rs:217/587/661`）。したがって、切り替えると試し実行と書き込みの判定が一致する。

## Findings

### M1（Major）: §4.2 の拒否は「slot `s` に入っている AC」だけが対象。docx は slot に入らない AC まで数えるので、位置のずれがまだ起きる

- **根拠（数え方）。** `images.rs:385` と `structure.rs:624` は、slot の中での位置 `k` を `indexed_elements().filter(|(i, e)| *i <= a && !is::<ParagraphProperties>(e)).count()` で数える。これは `w:pPr` 以外の**すべての要素**を数えるので、どの slot にも入らない AC も数に入る。
- **根拠（§4.2 の規則）。** §4.2 が拒否するのは「`lo ≤ s ≤ hi` の AC がある」ときだけである。§2 / §3 の定義では、次の 2 つはこれに当たらない:
  - **全 branch が空の AC**。区間が空で、「何も制約しない」と扱われる。
  - **別の slot にある AC**。`w:p` では slot 0（`w:pPr`）を包む `AC{w:pPr}` がこれに当たる。
- **例。** `w:p = [pPr, r0, AC{Choice Requires="w14"/}, r1, r2]` で `RunPosition::After(r1)` を指定する。
  - 呼び出し側の `slot_k` は 3 になる。lens の `index_in_slot` は r0=0、r1=1、r2=2 なので、`before` は一度も true にならない。
  - 結果、新しい run は r2 の**後ろ**に追加され、`Ok` が返る。
  - `[AC{w:pPr}, r0, r1]` で `After(r0)` を指定した場合も同じで、新しい run は r1 の後ろに入る。
  - 今は、親に AC があるだけで拒否されている。U1 が入ると、この 2 つの形だけが通るようになる。
- **ADR の説明が成り立たない。** §4.2 の理由「数え方を監査しなくても、既存の呼び出し側はすべて正しいままになる」は、この 2 つの形では成り立たない。round 1 の M1 と同じ種類の誤りで、`Ok` のまま本文の順序が入れ替わる。コーパスでは 0 件だが、全 branch が空の AC は MCE として正しい形である。
- **修正案。** 次の両方を入れる:
  - (a) §4.2 では、区間が空の AC を「どの slot にも入っている」と読む（安全側に倒す）。
  - (b) docx の `insert_image` とハイパーリンクの挿入は、§9 ですでに「拒否のまま」になっている。そこで、`w:p` に AC の子が 1 つでもあれば拒否する事前チェックを format 側に残す。そもそも、その数え方は未登録の要素も数えるので、lens の `index_in_slot` とは元から一致していない。
  - U1 または U3 の受け入れ条件に、上の 2 つの形で byte-identical に拒否されるテストを足す。

### m1（Minor）: §4.4 に載っていない呼び出しがある

- **漏れている呼び出し（`d066ff1`）:**
  - xlsx の `check_insert_by(|_, _| false)` による試し実行 5 箇所: `layout.rs:649`（`SheetFormatProperties`）、`layout.rs:793`（汎用 `E`）、`layout.rs:823`（`PageSetupProperties`）、`layout.rs:826`（`SheetProperties`）、`validation.rs:233`（`DataValidations`）。どれも書き込みは `get_or_insert` である（`layout.rs:719/850/854`、`validation.rs:240`）。
  - pptx `write/table.rs:829`（`check_move_in_by(…, |_, _| false)`）。書き込み側の `:860` は一覧にある。
- **影響。** 載っていない試し実行は §4.2 の側に残る。同じ slot に AC があると、書き込みでは通るものを試し実行で拒否する。結果は安全側の拒否だが、lift が 1 つ失われ、「試し実行と書き込みの判定が一致する」という性質も崩れる。
- **修正案。** §4.4 に上の 6 箇所を足し、xlsx の 5 箇所は `check_insert` / `check_get_or_insert` に切り替える。§4.4 の表には「grep で全数を確認した」と書き、どの式で grep したかも残す。

### m2（Minor）: 「AC の子を持ちうる」ビットは、`RawNode` の変更関数の中で維持すべき

- **今は古い false にはならない。** ADR は、このビットを「parser と挿入の primitive が立てる」としている。`d066ff1` で子を追加する経路を全部見た。
  - 対象: `node.rs` の `insert_child`、`splice_children`、`set_children`。
  - 呼び出し元: lens の挿入（`lens.rs:845/937/1168`）、detach（`lens.rs:441`、MC は拒否）、`same_part.rs:394`（AC は拒否）、`move_subtree.rs:248`（AC の移動は拒否）、`package/add.rs:405`（パッケージ側の root への追加）、`leaf_text.rs`（テキストだけ）。

  したがって、今のところ parser 以外に AC の子を足す経路は無く、古い false（Major 相当）にはならない。
- **将来は崩れうる。** 決まりが「呼び出し側が立てる」なので、新しい経路が足されれば簡単に崩れる。
- **修正案。** §4.3 を次のように直す:
  - ビットは `RawNode` の `insert_child` / `splice_children` / `set_children` の**中で**更新する（AC の要素を足したら立てる）。
  - 「ビットが立っていない ⇒ AC の子は無い」を `debug_assert` と単体テストで確かめる。
  - ADR-0010 で `RawNode` を詰めて配置している（`CompactSpan` の上位ビットに flag を置いている）ので、ビットをどこに置くか、`RawNode` のサイズが変わらないことも書く。
  - 一度立ったビットが残るのは、削除のあとで走査が 1 回余分に起きるだけなので、問題ない。

### m3（Minor）: 公開関数 `check_append_among` は要らない。残すなら、型付きの `Lens` メソッドにする

- **要らない理由。** `check_insertable` の呼び出し元は 3 つで、どれも新しい公開関数なしで済む:
  - `skip` を使うのは `worksheet.rs:297` の `c` だけで、`c` の AC は `locate` がすでに拒否している（`worksheet.rs:157`）。
  - `edit.rs:698` の `si` は、registry が拒否している（`registry/strings.rs`、`Table::has_ac`。§9 にもそう書かれている）。
  - `worksheet.rs:280`（`sheetData`）は `skip` を使わないので、lens の `check_get_or_insert::<SheetData>` で足りる。

  したがって、`check_insertable` が今どおり AC を一律に拒否しても、§9 の lift は 1 つも失われない。新しい公開項目を足さずに済む。
- **残す場合の形。** ADR-0002 §4.2 の基本操作は、`C: ChildOf<E>` で子の型を検査する型付きの lens メソッドである。`E` を型引数に取りながら、子は `QName` で受け、`doc` と `parent_path` を自由関数に渡す形は、この一覧から外れている。次の形を推奨する:
  - 読み取り専用の `Lens<E>`（`Lens::at(root, path)` がすでにある）に、`check_insert_skipping::<C: ChildOf<E>>(skip)` を足す。スコープは lens が持っているので、`Requires` の検査もそのままできる。
  - path の先が `E` でなければ `Err` にすることを明記する。
  - 生成物の名前は変わらないので、生成された公開名を変えないという規則には触れない。

### n1（nit）: spike の `ac_stats` が区間の統計を `cur = -1` で計算している

`ac_rank.py:427` は、`disagree` / `spans` の統計を実際の greedy の `cur` ではなく `cur = -1` で計算している。2 つの slot に現れる名前があると数がずれうるが、コーパスでは影響しない。`assign` と同じ前置き（prefix）から `cur` を取るのが望ましい。

## 所有者への質問（残っているもの）

| Q | 推奨 |
|---|---|
| Q1（区間を広げるか拒否するか） | 推奨どおり、広げる。 |
| Q2（unranked の AC、先頭が未登録要素の branch は `lo = cur`） | 推奨どおり、拒否する。 |
| Q3（エラー型） | 推奨どおり、`UnresolvedSibling` を再利用する。 |
| Q5（`mce_wrapped` を限定して再定義） | 推奨どおり、yes。 |
| Q6（スタイル表への追加） | 推奨どおり、この ADR からは外す。 |
| （追加の提案）`check_append_among` | 公開しない。`check_insertable` は AC を一律に拒否したままにし、`sheetData` は lens の check で扱う（m3）。 |

Q4 と Q7 は coordinator が決定済みで、どちらも妥当である。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 3 / nit 1）
