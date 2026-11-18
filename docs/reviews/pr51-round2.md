# PR #51 レビュー（Opus, round 2）

対象: `impl/pptx-phase2` @ `ed4d5e7`（修正コミット `6ab5119` と origin/main のマージ）。round 1（`docs/reviews/pr51-round1.md`）の指摘ごとに確認した。
リポジトリは変えていない。作業は scratchpad の `review-51r2-57r2/` で行い、`git clone` したコピーを使った。

- `r51/`: `ed4d5e7`。`tests/structure.rs` の末尾にレビュアーのプローブ `zz_a`〜`zz_e` を足し、round 1 の `examples/rsweep51.rs` を入れた。
- `mut51/`: レビュアーのテストを入れていないコピー。round 1 の mutant 25 個と、今回の修正向けの 4 個（R26〜R29）、oracle の検出力を見る 3 個を流した。

## 結論（先に）

**収束（Blocker 0、Major 0）。** M1 は正しく直っており、ADR-0012 にも post-acceptance note がある。m1〜m3 と n1〜n4 も対応された。残るのは Minor 1 件で、R21 のテストが別の経路を通っているため、mutant R21 がまだ生き残る（r2-m1）。ほかは Nit である。

- corpus sweep（617 デッキ）: 出力 3,522 本は、round 1 の出力と**すべてバイト同一**だった。manifest（ok 3,409、拒否 357、move の `-1` 115）も一致した。round 1 では、同じ出力を `check51.py` で検査して新しい欠陥 0 件だったので、その結果がそのまま当てはまる。panic と `REFUSAL_CHANGED` は 0 件。
- `cargo test --workspace --release`、`codegen --check`、`pptx-template --check`、clippy（`-D warnings`、`ooxml-pptx --all-targets`）: 通った。
- 作者の structure oracle: 233 cases ok を再現した。

## 解消状況

| round 1 | 状況 | 確認内容 |
|---|---|---|
| M1 セクションあり・スライド 0 枚 | **解消** | `slides.rs:978-984`: セクションがなければ何もせず、セクションがあってスライドが 0 枚なら `(0, 0)`（最初のセクションの先頭）に入れる。ADR-0012 の post-acceptance note（106-111 行）で §5 の読み方を直した。プローブ `zz_a`（全部消す → `add(Some(0))` → add → `duplicate(0, Some(0))` → move → delete → 保存して開き直す → 全部消す → add）と `zz_b`（空のセクションから `p14:sldIdLst` 要素を取り除いた入力）は、どちらも `check_structure` を通った。作者のテスト `sections_with_no_slides_take_the_next_slide_in_the_first_section` は、M1 を元に戻す mutant（R26）も「最後のセクションに入れる」mutant（R27）も検出した |
| m1 生存 mutant のテスト | **ほぼ解消**（r2-m1） | 再実行の結果は、25 個中 22 個が検出、生存 3 個。R01、R07、R10、R14、R17、R23 は、新しいテストで検出された。R09、R15、R24 は round 1 で等価またはほぼ等価とした 3 個である。**R21 は生存のまま**（下記 r2-m1） |
| m2 §4.5、§4.6 の先送り | **解消** | ADR-0012 の note（120-124 行）に、coordinator の判断、日付、N7 を follow-up で検証することが書かれた。README の表にも「not yet」の行がある |
| m3 oracle の独立検査 | **解消** | `structure.py` に `expected_cascade`（到達集合を、外からの参照で不動点まで削る）、`unreachable` の差分、`app.xml` の真値比較が入った。Rust 側の `CHANGED` リストには依存していない。検出力を mutant で確かめた。cascade から media を落とす mutant（O2）は、`animation.pptx delete 1` の差分として検出された。R01 は、fixtures に共有チャートがないので oracle では検出されないが、Rust のテストが検出する。残る弱点は n-c |
| n1 creationId の再利用 | 解消 | ADR の note に記載された |
| n2 strict URI | 解消 | ADR の note に記載された（fail closed） |
| n3 カスタムショーの別 rId | 解消 | presentation から slide part への全関係の `Id` で照合するようになった。テスト `custom_shows_naming_the_slide_by_another_rid_refuse` は、修正を元に戻す mutant（R28）と、最初の `Id` だけを見る mutant（R29）を検出した。カスタムショーがない場合の別 rId（プローブ `zz_c`）は `Ok` になり、2 本の関係がどちらも消え、構造検査を通った |
| n4 ADR-0001 の空行 | 解消 | main との差分はない |

## Findings

### r2-m1 [Minor・テスト] R21 のテストは、カスケード候補の検査ではなく、スライド自身への参照の検査を通っている

- 作者のテスト `a_package_relationship_to_the_slide_refuses_delete`（`tests/structure.rs`）は、`_rels/.rels` から**スライド自身**への関係を足す。この関係は、`delete_slide` の入口にある参照検査（`slides.rs` の `incoming_relationships(&part)` のループ）で `SlideIsReferenced` になる。R21 が変えるのは、`cascade()` の「外から参照される候補を落とす」検査（`slides.rs:710`、`inc.source` が `None` のとき）なので、このテストを通らない。再実行でも R21 は **SURVIVED** だった。コミットメッセージの「Tests for … R21 … (each mutant killed)」は正しくない。
- 検出できるプローブ（`zz_e`）: `poi/bar-chart.pptx` の `_rels/.rels` に、`ppt/embeddings/Microsoft_Excel_Worksheet1.xlsx` への関係（任意の型）を足し、`delete_slide(0)` を呼ぶ。
  - head: `Ok`。チャートは消え、埋め込みは残り、`check_structure` も通る。
  - R21: `Err(Core(RegistryRefused { … RelationshipTypeNotAllowed { rels: "_rels/.rels", id: "rId99", … } }))`。
- 提案: このプローブをテストとして足す。既存のテストは、そのまま「スライド自身への参照」のテストとして残してよい。
- 実害: なし。退行しても、core の `remove_parts` が拒否する（fail closed）。成功すべき delete が `Core(…)` になるだけである。

### Nit

- **n-a**: `delete_slide` の `let rid = self.slide_rel_id(index)?;` と `let _ = &rid;`（`slides.rs:559`、`567`）。n3 の修正後は `rid` を使っていない。`slide_rel_id` を検証として残すのであれば、`self.slide_rel_id(index)?;` だけにして、そのことをコメントに書く。そうでなければ削除する。
- **n-b**: ADR-0012 の Q1 の既定値（720 行目）は「empty sections stay empty」のままである。M1 の規則（スライドが 0 枚なら最初のセクションに入れる）は、この文の例外になる。round 1 で提案した manual check の項目（空のセクションだけのデッキにスライドを足したとき、PowerPoint がどのセクションに入れるか）もまだない。Q1 か §10 に 1 行足す。
- **n-c**: `structure_cases.rs:74-76` は、操作が `Err` になったケースを黙って飛ばす。そのため、「成功すべき操作が拒否になる」退行は oracle から見えない。カスケードの外部参照の除外を丸ごと止める mutant（O1）では、ケースが 233 から 227 に減り、oracle は「227 cases ok」を出した（Rust の `delete_keeps_shared_media` などは検出する）。ケース数の下限を assert するか、拒否を manifest に記録して oracle 側で数える。あわせて、`structure.py` の `CASCADE` 集合は定義されているが使われていない（`expected_cascade` は `KEEP` 以外をすべて辿る。未知の型は Rust 側で拒否されるので結果は同じ）。削除するか、`KEEP`/`CASCADE` 以外の型を見たときに assert するのに使う。

## mutant の再実行

`mut51.py`（`--test structure --test new_deck --lib`、release）、`ed4d5e7`。

| 区分 | mutant |
|---|---|
| 検出（22） | R01〜R08、R10〜R14、R16〜R20、R22、R23、R25 |
| 生存・等価またはほぼ等価（3） | R09（空の `p:extLst`）、R15、R24 |
| 生存・テストが足りない（1） | R21（r2-m1） |
| 今回の修正向け（4、すべて検出） | R26 M1 を元に戻す、R27 最後のセクションに入れる、R28 n3 を元に戻す、R29 最初の `Id` だけを見る |

oracle（`structure_cases` → `structure.py`）の検出力: O2（media をカスケードしない）は検出、R01 と O1 は検出しない（n-c。どちらも Rust のテストが検出する）。

## 判定

**converged: yes**

- Blocker: なし。
- Major: なし（M1 は解消）。
- Minor: r2-m1（R21 のテストが別の経路を通っている。プローブ `zz_e` をテストにする）。
- Nit: n-a（使っていない `rid`）、n-b（Q1 と manual check に M1 の規則を足す）、n-c（`structure_cases` が拒否を黙って飛ばす、使われていない `CASCADE`）。

r2-m1 と Nit は、マージ前に直すのが望ましい。ただし、収束の条件（Blocker と Major がない）は満たしている。
