# ADR-0020 レビュー（Opus, round 1）

対象: PR #145 `plan/ac-rank` @ `0c232039004454ae940bff23e05bc7f0420f74a4`
（`docs/adr/0020-alternatecontent-rank.md` revision 1 と `spikes/ac-rank/`）。

照合したもの:

- ADR-0002 §1.3、§4.1〜§4.3 と、そのレビュー記録（round 1 M1）。
- `crates/ooxml-core/src/lens.rs`: `candidates`、`place_skipping`、`plan_insert_at`、`get_or_insert`。
- `xtask/src/model.rs`（`Override::MceWrapped`）と `codegen/overrides.toml`。
- フォーマット側で AC を拒否している箇所:
  - docx: `images.rs`、`structure.rs`、`format.rs`
  - xlsx: `worksheet.rs`、`edit.rs`、`registry/`
  - pptx: `slides.rs`、`write/table_style.rs`
- ADR-0019 §3.2。
- open PR #144 と #140 の差分。

## 結論（先に）

区間規則そのものは正しい。次の 2 点を確かめた:

- AC ごとに、branch の選び方を全部試す総当たりの oracle と、コーパス上の全 pair で一致する。
- 出現数の上限を「branch の最大」で数える規則は、過不足なく正しい。

一方、Major が 4 件ある。

1. ADR は、同じ slot の AC を `before` callback に渡さず、`index_in_slot` にも数えないと決めている。しかし docx の既存の呼び出し側は、slot 内の index を **AC を含めて** 数えている。このため U1 が入ると、docx の該当操作は拒否されなくなり、黙って違う位置に挿入する。
2. §9 は docx `w:rPr`/`w:pPr` の事前チェックを外すとしている。しかしこのチェックは挿入だけでなく、Clear（削除）も守っている。外すと、branch の中にだけある書式を Clear しても `Ok` の no-op になる。
3. open PR #144 は、`ooxml-core` に別の規則の公開 API `insert_after_alternate_content` を足している。ADR-0020 はこれに触れていない。
4. 未定義要素だけを持つ branch を「制約なし」として扱う。これは ADR-0002 §4.3 の扱いとも、Q2 の fail-closed とも食い違う。

**converged: no**。

## 前ラウンドの解消状況

round 1 のため、なし。

## 検証したこと（挙動）

- **spike の再実行。** POI の docx/pptx は README の手順で取得し、xlsx は `/tmp/poi-xlsx`（366 件）を使った。`reference/fixtures` も加えた。`python3 -I spikes/ac-rank/ac_rank.py …` の出力は、コミット済みの `results/ac-rank.txt` と**完全に一致**した（`diff` で差分なし）。
- **総当たりの oracle**（スクラッチのプローブ。コミットしていない）:
  - 方法: AC を持つ親のインスタンスと型付きの子からなる各 pair について、各 AC の branch の選び方（`mc:Fallback` が無い AC には空の branch を足す）の**全組み合わせ**を展開する。そのうえで、spike の規則が決めた位置に新しい子を置き、§4.1 の greedy 割り当てと slot の上限で検査した。
  - 結果: 2,076 pair で規則と一致した。内訳は `ok` 1,625（wildcard 親の 920 を含む）、`limit` 431、`limit_ac` 20。組み合わせが 4,096 を超えた 3 件は飛ばした。規則が `ok` で oracle が不正、または逆、という食い違いは 0 件。
  - したがって、spike は ADR §3〜§5 の規則を正しく実装している（ただし m3 を参照）。
- **出現数の上限（「branch の最大」）は厳密。** 空の branch を選んだ reader は 0 個を見る。しかし直接の子を 1 個足すと、要素を持つ branch を選んだ reader には 2 個に見える。だから拒否が必要で、過剰な拒否ではない。合計で数えないのは、reader が 1 つの branch しか見ないからで、これで漏れもない。
- **`s == lo == hi`、隣り合う AC、wildcard、ECMA と順序が食い違う 8 件。**
  - 食い違う 8 件は、生成時点で `Content::Unsupported` になる。branch の中の要素も rank を持たないので、挿入は拒否される。規則がこの 8 件を悪化させることはない。
  - 同じ `Requires` を持つ AC が兄弟に並ぶ親が 6 件ある。どの場合も、規則と、branch を独立に選ぶ oracle の判定は一致した（m2 を参照）。
- **`mce_wrapped` は今は使われていない。** `codegen/overrides.toml` の op は `set_default`、`add_child`、`set_type` だけである。`mce_wrapped` は `xtask/src/model.rs` の単体テスト（`legacyDrawing` に適用）でしか使われていない。`x15ac:absPath` は slot 表にあり（`sml.rs:19614`）、`ChildOf<Workbook>` は持たない。再定義しても、今の生成物は変わらない。

## Findings

### M1（Major）: AC を `index_in_slot` に数えない（§4、Q4）と、docx の画像とハイパーリンクの挿入が黙って位置を誤る

- **根拠（呼び出し側の数え方）。** 次の 2 箇所は、slot 内の index `k` を同じ式で数えている:
  - `crates/ooxml-docx/src/images.rs:385`（`insert_image`、`run_insertion_point`）
  - `crates/ooxml-docx/src/structure.rs:624`（ハイパーリンクの追加）

  ```rust
  p_node.indexed_elements().filter(|(i, e)| *i <= a && !is::<ParagraphProperties>(e)).count()
  ```

  これは `w:pPr` 以外の**すべての要素**を数える。`w:p` の直下にある `mc:AlternateContent` も数に入る。数えた `k` は `insert_by::<R>(|i, _| Some(i) == slot_k)` に渡される（`images.rs:249/281`、`structure.rs:643/661`）。
- **ADR-0020 §4 との食い違い。** §4 は、同じ slot の AC を callback に渡さず、`k` も進めないと決めている。
- **再現例。** `w:p` の子が `[pPr, r0, AC{r}, r1, r2]` で、`RunPosition::After(r1)` を指定する。
  - 呼び出し側の `slot_k` は 3 になる（r0、AC、r1 を数える）。
  - lens 側の `k` は r0=0、r1=1、r2=2 なので、callback は一度も `true` を返さない。
  - 結果、新しい run は **r2 の後ろ**に追加される。
- **なぜ今は起きないか。** 今は、AC があれば core が `UnresolvedSibling` で拒否している。docx 側には `w:p` 直下の AC に対する事前チェックが無い。`body.rs` の story walk は AC の中に降りないが、AC の外にある run はそのまま index され、`check_structure_neighbour` にも AC の検査は無い。したがって、U1 が入るだけで（§9 の「Lift」行が無くても）この操作は `Ok` になり、本文の順序が黙って入れ替わる。
- **「r1 の直後」という意図は、そもそも表せない。** AC は r1 と r2 の間にある内容なので、正しい位置は AC の前である。しかし Q4 の決定では、callback で AC の前を指定できない。
- **index を数える他の callback。** `pptx/src/slides.rs:378/533/994`（`k == at`）と `xlsx/src/sheet_ops/mod.rs:563`（`x:sheet` を自分で数える）も、`k` の意味が変わると影響を受けうる。コーパスにはこれらの親の直下の AC が 0 件なので、Minor 相当である。
- **ADR の記述の誤り。** §4 の「既存の callback はすべて AC を飛ばすことを学ぶ必要がある」は逆である。docx の既存の数え方は、AC を**含めて**数えている。
- **修正案。** U1 では、`before` callback を受け取る `insert_by`/`insert_in_slot`/`check_insert_by` について、同じ slot に AC があれば今どおり `UnresolvedSibling` を返す。同じ slot の AC の隣に置いてよいのは、`insert`/`get_or_insert`（slot の末尾への追加）だけにする。あわせて次の 2 点も行う:
  - `k` で位置を決める呼び出し側を §9 に列挙し、それぞれに「同じ slot に AC がある場合は拒否し、出力は byte 単位で変わらない」ことを確かめるテストを U1 の受け入れ条件に入れる。
  - Q4 の推奨をこれに合わせて書き換える。

### M2（Major）: §9 docx「`w:rPr`/`w:pPr` に AC がある場合の書式設定を Lift」すると、Clear が黙って no-op になる

- **根拠。** `Planner::existing`（`crates/ooxml-docx/src/format.rs:357`）は、挿入だけでなく、`clear`（L386）、`on_off`/`val` の Set、`attrs` の削除すべての入口である。事前チェックを外すと、`existing` は**直接の子だけ**を探すことになる。
- **壊れる例。** `w:b` が `w:rPr` 内の AC の branch にしかない場合を考える。
  - `Prop::Clear` を指定すると、`existing` が `None` を返すので plan が空になり、`Ok` で何も変わらない。
  - しかし Word は branch の中の `w:b` を読むので、太字は残る。
  - 属性の削除だけの `attrs` も、L431 で何もせずに終わる。
- **拒否される場合もある。** Set のうち新しく挿入するものは、§5 の上限（`max_each = 1`、branch も数える）で `AlreadyPresent` になる。これは fail-closed で正しい。漏れるのは削除の方向だけである。
- **ADR にはすでに同じ型の修正がある。** pptx の `add_table_style` では、`styleId` の一意性検査を branch の中まで広げると書いている。docx の行にはそれが無い。
- **修正案。** §9 docx 3 行目に、次の条件を付ける: 「その種類（QName）が AC のどの branch にも無いときだけ Lift する。branch にある種類を Set/Clear/remove しようとしたら拒否する」。

  一般的な規則として §9 の冒頭にも書く: 「フォーマット側で『直接の既存の子』を探して判断している処理は、事前チェックを外すとき、branch の中も見る」。

  U3 の受け入れ条件には、branch の中にだけある `w:b` を Clear すると拒否され、出力が byte 単位で変わらないテストを入れる。

### M3（Major）: open PR #144 が、ooxml-core に別の AC 規則の公開 API を足している。ADR-0020 はこれを扱っていない

- **根拠。** `gh pr diff 144`（ADR-0019 P4a-1）は `lens.rs` に次を加えている:
  - `pub fn insert_after_alternate_content`
  - `place_skipping` の引数 `rank_alternate_content`

  #144 の規則は「各 branch の最初の要素の最小 rank を点とし、どの branch のどの要素も target slot より前にあること」である。上限は数えない。ADR-0020 の区間規則とは別物である。
- **ADR-0020 側の記述との食い違い。**
  - §8 は「新しい公開項目は不要」としている。
  - §9 は pptx の `p:extLst` を「core で lift される（ADR-0019 はすでに lens を通している）」としている。
  - U4 は「P4a-1 がまだ入れていなければ」としか書いていない。

  #144 が先にマージされると、core に AC の規則が 2 つ並び、公開 API（`#[non_exhaustive]` でもない method）が 1 つ増える。U1 でそれを消すことは、公開名の変更になる。
- **修正案。** ADR-0020 に、#144 との関係を決める段落を足す。次のどちらかを選ぶ:
  - (a) #144 は `insert_after_alternate_content` を足さず、U1 を待つ（または U1 を先に入れる）。
  - (b) #144 の method を暫定として受け入れ、U1 でそれを `insert` の別名にして、旧規則を消す。

  Q7（U2〜U4 の順序）もこれに合わせて直す。私の推奨は (a) で、U1 を #144 より先に入れる。

### M4（Major）: 未定義要素だけを持つ branch を「制約なし」とする扱いが、ADR-0002 §4.3 と Q2 に反する

- **根拠（§3 の扱い）。** §3 の手順 1 は、branch の中の未定義要素を読み飛ばし、listed な要素を 1 つも持たない branch の区間を *none* とする。そのうえで、*none* の branch を和集合から除く。
- **空の branch と非空の branch の違い。** 空の branch（`<mc:Fallback/>`）なら、これで正しい。しかし、空でなく未定義要素だけを持つ branch は事情が違う。
- **ADR-0002 §4.3 との違い。** ADR-0002 §4.3 は、未定義の直接の子を「直前の listed な兄弟に付属する」と扱う。直接の子が `[A(5), v, X(20)]` のとき、slot 10 の挿入は `[A, v, new, X]` になり、`v` は `A` から切り離されない。
  一方、ADR-0020 の規則では、`[A(5), AC{Choice{v}, Fallback{X(20)}}]` の AC の区間は `[20,20]` になる。slot 10 の新しい子は AC の**前**に置かれ、Choice を選ぶ reader には `[A, new, v]` と見える。直接の子のときには決してしない「`A` と `v` の間への挿入」をしている。
- **Requires を理解する reader には、`v` の位置が意味を持つ。** その reader は `v` のスキーマを知っている。
- **Q2 との矛盾。** Q2 は、全 branch が未定義要素だけの AC を「ライブラリが位置を知りえない内容を隠している」として拒否する。同じ理由は、1 つの branch だけが未定義要素のみの場合にも当てはまる。
- **先頭が未定義要素の branch も同じ。** listed な要素の前にある未定義要素は直前の兄弟に付属するので、その branch の `lo` は `cur` である。
- **「厳密な union で、保守的な近似ではない」という記述は過大である。** 厳密なのは、branch の要素がすべて listed のときだけである。
- **コーパスへの影響。** プローブで数えると、「空でなく未定義要素だけの branch」と「先頭が未定義要素の branch」は、どちらも 0 件だった（3 フォーマットとも、生成された slot 表を持つ親の範囲で）。直してもコーパス上の lift は 1 件も失われない。
- **修正案。** 次の 2 つのどちらかにする:
  - (a) 空でなく未定義要素から始まる branch の区間の `lo` を `cur` にする（直前の兄弟に付属させる）。そうすると、`[A, AC]` の間への挿入は straddle として拒否される。
  - (b) 空でなく listed な要素を持たない branch がある AC を、Q2 と同じく拒否する。

  §3 の表に「空でなく、未定義要素だけの branch」の行を足し、spike にもその counter を足す。

### m1（Minor）: `insert_at`/`check_insert_at` の局所検査で、AC の区間をどう読むかが決まっていない

- **根拠。** §3 の区間は greedy の `cur` から決まる。しかし `plan_insert_at` は O(1) の局所検査で、`cur` を計算しない。§4 の「AC の隣はその区間で読む」をそのまま実装すると、`cur = 0` から区間を作りがちになる。
  - 2 つの slot に現れる名前（`w:tbl` の range marker）では、それで `hi` が実際より小さくなり、前側の検査が甘くなる。これは review F1 と同じ型の穴である。
  - 今の生成物では、そうした名前は `AmbiguousSlot` で先に弾かれるので、実際には起きない。
- **未定義・不正な AC の扱いも書かれていない。** 局所検査で、隣ではない位置にある未定義（unranked）や不正な AC を拒否するのかどうかが書かれていない。`insert_by` は親全体を拒否するので、そのままでは両者の判定が食い違う。
- **修正案。** §4 に次を書く:
  - 隣の AC の区間は、branch の各要素の**すべての候補 slot** の min と max から作る（F1 と同じ読み方）。
  - 未定義・不正な AC は、位置に関係なく `insert_at` でも拒否する（または、局所検査であることを明記する）。

### m2（Minor）: 「厳密」という主張は、branch を独立に選ぶ場合に限られる

- **根拠。** 同じ `Requires` を持つ AC が兄弟に並ぶと、reader は両方で同じ branch を選ぶ。§3 の union は独立な選択を前提にしているので、相関がある場合は**安全側の過剰な拒否**になる（例: 1 つ目の AC が Choice に X(5) を持ち Fallback が空、2 つ目の AC が Choice が空で Fallback に Y(3) を持つ場合、親全体が Disorder になる）。コーパスにはこの形の親が 6 件ある。どれも判定は変わらなかった。
- **修正案。** §3 の「保守的な近似ではない」を「1 つの AC については厳密。`Requires` の相関は無視し、安全側に倒す」と直す。`mc:Fallback` の無い AC は「暗黙の空の branch」を持つと読むことも明記する（xlsx に 248 件ある。結果は今の規則と同じ）。

### m3（Minor）: spike の docstring と実装が、ADR の決定と一部食い違う

- **食い違い。**
  - docstring と `assign()` は、未定義の AC を「unlisted として扱う」。ADR の Q2 は拒否する。
  - §3.1 の不正な AC（`mc:Choice` が無い、branch でない子がある）は数えるだけで、pair の判定には反映していない。
  - `Requires` の prefix は「パートのどこかで宣言されているか」を見ている。ADR の基準は「スコープ内で束縛されているか」である。
- **影響。** どれもコーパスでは 0 件なので、数値には影響しない。
- **修正案。** docstring を ADR に合わせる。`simulate` で未定義・不正な AC を `refused` として数える。

### m4（Minor）: 「`check_insertable` が core の検査を呼ぶ」には、新しい公開 API が要る。shared strings の拒否は registry にしか残らない

- **新しい公開 API が要る理由。** `check_insertable` は、lens の無い**仮の子リスト**に対して実行される（`worksheet.rs:297` では `kept` という iterator）。core にはこれを受ける API が無い。§9 の xlsx 1 行目どおりにするには、core に公開関数を足す必要があり、§8 の「新しい公開項目は不要」と矛盾する。
- **shared strings の拒否が残る場所。** `check_insertable` は `edit.rs:698` で `SharedStringTable` にも使われている。§9 は shared strings の拒否を Stays としているが、委譲後にそれを支えるのは `registry/mod.rs:61` の「AC を持つ表には追加しない」だけになる。
- **修正案。** §8 に、core に足す公開関数（または `check_insertable` を残す判断）を書く。§9 の xlsx 3 行目には、拒否を支える箇所として `registry/mod.rs` を記す。

### m5（Minor）: `mce_wrapped` の再定義は安全だが、適用してよい範囲を書くべき

- **安全な理由。** 今の生成物は変わらない（上の「検証したこと」を参照）。
- **危ない適用先。** ADR-0002 §1.3 は、ooxmlsdk の `legacyDrawing`/`controls` への使い方を例に挙げている。これらは直接の子としても現れる。新しい定義でこれらに適用すると、`ChildOf<Worksheet> for LegacyDrawing` が消え、`add_note` の挿入経路と公開 impl が消える。
- **修正案。** §8 と Q5 に、次の 3 点を書く:
  - 適用してよいのは「Office が直接の子としては決して書かない子」（`x15ac:absPath`）だけである。
  - 適用すると公開 impl が消えるので、通常の生成物差分のレビューを通す。
  - `xtask/src/model.rs` の既存テスト（`legacyDrawing` に適用している）を U1 で新しい定義に書き換える。

## 所有者への質問について

| Q | 判断 |
|---|---|
| Q1（広げるか拒否するか） | 推奨どおりでよい（区間を広げる）。 |
| Q2（listed な要素の無い AC） | 拒否でよい。ただし、空でなく未定義要素だけの branch と、未定義要素から始まる branch にも広げる（M4）。 |
| Q3（エラー型） | 推奨どおりでよい（`UnresolvedSibling` を再利用）。 |
| Q4（callback に AC を見せるか） | **変更する。** 「見せない」だけでは不十分で、`before` callback を受け取る挿入は、同じ slot に AC があれば拒否する。AC の隣への追加を許すのは `insert`/`get_or_insert` だけにする（M1）。 |
| Q5（`mce_wrapped`） | yes でよい。ただし適用範囲を限る（m5）。 |
| Q6（スタイル表） | 推奨どおりでよい。 |
| Q7（U2〜U4 の順序） | **変更する。** U1 を #144 より先に入れ、#144 は `insert_after_alternate_content` を足さない（M3）。U1 の後は U4、次に U2 の順がよい。 |

## 判定

converged: no（Blocker 0 / Major 4 / Minor 5 / nit 0）
