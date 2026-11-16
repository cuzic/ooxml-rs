# PR #14 レビュー（Opus, round 5）

対象: `core/remove-part` @ `54309a7`（round 4 の N11〜N16 の修正）。
照合したもの: round 1〜4 のレビュー、`package.rs`、`opc.rs`、`tests/package.rs`、README、ADR-0001。
プローブと mutant は scratchpad（`review-14r5-26/`）の作業コピーで実行した。リポジトリは変えていない。

- round 3、4 のプローブ（`probe_r3.rs`、`probe_r4.rs`、`probe_r4b.rs`）を再実行した。
- 新しいプローブ `probe_r5.rs`（T1〜T4、`corpus5`）と `probe_r5c.rs` を書いた。
- `mutants5.py` で、round 4 で生き残った AD、AH、AI、AJ、AK と、新しい mutant 11 個（AL〜AV）を試した。
- コーパスを走査した。`reference/fixtures/**`、`corpus/files/`、`review-pr22/`（POI の xlsx サンプルを含む）と、round 4 の一覧を合わせ、内容の hash で重複を除いて 4,103 ファイルになった。

## 結論（先に）

**未収束（Major 1 件）。** round 4 の N11〜N16 は、指摘した形ではどれも解消している。

- N11: open は線形になった。20,000 entry で 26 ms、50,000 で 66 ms、65,000 で 89 ms（release）。
- N12: S1（同じ `.rels` に一致する関係が同居）と S2（文字参照）は、どちらも `UnresolvedMention` で拒否される。
- N13: コーパスで `UnresolvedMention` は **0 件**になった（round 4 は実在 part 976 件、ディレクトリ entry 1,830 件）。`Ok` は 22,764 件で、round 4 で安全網を外したときの数と一致する。
- N14: S3 の `%5Frels`、`.rel%73`、`%2Exml` は、どれも `StillReferenced` で整合する。
- N15: AD、AI、AJ、AK は検出される。ただし AH は生き残る（N19）。
- N16: open の doc、README の open の行、ADR-0001 の Layer 1 の注記が入った。

残る Major は 1 件である。

- **N17**: N14 の修正で、`.rels` の source とその base を、**%デコード済みの名前から**求めるようになった。その名前をもう一度 `find_ci` や `same_part_name` に通すので、二重にデコードされる。その結果、`%25` の後に 16 進 2 桁が続く名前（例: `xl/a%2541.xml`）を source に持つ、**OPC として正しい**パッケージで、`Ok` のまま `r:embed` がダングリングする。round 4 では raw 名を使っていたので起きなかった。この PR の新しい退行である。

ほかに Minor が 3 件ある。

- N18: `%2F` を含む source 名で、`.rels` が走査から外れる。
- N19: mutant AH、AM、AP、AQ、AT が生き残る。AH はコミットメッセージで「検出される」と書かれている。
- N20: 文言が実際の挙動とずれている箇所がある。

## 解消状況

| round 4 | 状況 | 根拠 |
|---|---|---|
| **N11** open の O(n²) | **解消** | `package.rs:231-237` で、各名前を一度だけ正規化して `HashSet` に入れる。T4: n=20,000 で 26 ms、50,000 で 66 ms、65,000 で 89 ms。round 4 の S5 は n=20,000 で 21.7 ms（round 4 では 70.5 s）。`open_duplicate_check_is_linear_small` は常に走る。50k の版は `#[ignore]` で、`--ignored --release` で通過した。 |
| **N12** 安全網の範囲 | **解消** | パース済みの `Target` で、関係ごとに判定する（`package.rs:511-526`）。`indices` が空でない場合にも走る。S1 の 2 形と、S2 の `xl\calc&#67;hain.xml`、`%2E/calc&#x43;hain.xml` は `UnresolvedMention` で拒否され、何も変わらない（T3 でバイトと state を確認）。T2 の `..\xl\calcChain.xml`、`sub\..\calcChain.xml` も拒否される。 |
| **N13** 過剰な拒否 | **解消** | コーパス 3,985 件（開けたもの）の全 part について、`UnresolvedMention` は 0 件だった。glossary の同名 part、External、Type URI、ディレクトリ entry は、どれも拒否されなくなった（S6、`safety_net_does_not_over_refuse`）。 |
| **N14** `%5Frels` | **解消（ただし N17 の退行を生んだ）** | `is_rels_part` と `source_of_rels_part` を正規化済みの名前に適用する（`package.rs:461`、`465`）。S3 の 3 形はすべて `StillReferenced` になる。 |
| **N15** mutant | **一部解消** | AD、AI、AJ は `padded_attributes` で、AK は `safety_net_is_per_relationship` で検出される。**AH は生き残る**（N19）。 |
| **N16** 文書 | **解消** | `Package::open` の doc（`package.rs:193-197`）、README の L27、ADR-0001 の Layer 1 の注記。ADR の「0 of 4,102」は今回の走査（`%` を含む entry 名を持つファイルは 0 件）と合っている。 |

## 新しい指摘

### N17 [Major] source の名前が二重にデコードされ、正しい OPC パッケージで `Ok` のまま `r:id` がダングリングする

**根拠**（`package.rs:465`、`563-565`。`probe_r5c.rs::pct25`、`probe_r5.rs::t1`）

- `source_of_rels_part(&normalize_part_name(&rels_name))` は、%デコードと小文字化を済ませた名前から source を作る。
- `xl/_rels/a%2541.xml.rels` の source は `xl/a%41.xml` になる。
- ところが、その source を探す `self.find_ci(src)` は、もう一度 `normalize_part_name` を通す。そのため `xl/aa.xml` と比べることになり、実在する `xl/a%2541.xml`（正規化すると `xl/a%41.xml`）が見つからない。
- 見つからない source は、`if let Some(si) = …` で**黙って飛ばされる**。そのため、`refers_to_rel_id` による `StillReferenced` の検査が走らない。
- 結果（同じ構成で、source の名前だけを変えて比べた）:

  | source | `remove_part("xl/media/i.png", [image])` |
  |---|---|
  | `xl/plain.xml` | `Err(StillReferenced { source: "xl/plain.xml", id: "rId1" })` |
  | `xl/a%2541.xml` | **`Ok`**。`.rels` から `rId1` が消え、sheet の `<pic r:embed="rId1"/>` はそのまま残る。保存後に開き直すと、`relationships("xl/a%2541.xml")` は 0 件になる。 |

- `%25` は `%` 自身の %エンコードである。非予約文字でも `/` でもないので、`a%2541.xml`（文字としては `a%41.xml`）は OPC として正しい part 名である。round 4 は raw 名から source を求めていたので、この退行は N14 の修正で入った。
- 同じ原因で、base も二重にデコードされる。
  - `x%2541/_rels/s.xml.rels` の `media/i.png` は、`x%41/media/i.png` に解決される。比べるときに `xa/media/i.png` になるので、一致しない。
  - 今は安全網が `UnresolvedMention` で拒否する（安全な側に外れる過剰な拒否）。
  - ただし、正規化すると `xa/media/i.png` になる part が別にあると、安全網の `find_ci(&resolved).is_some()` が成り立って拒否されない。この場合は、関係が残ったまま part が消える（コードから導いた。プローブは未実施）。
- 実在するファイルで踏む見込みは低い（コーパスに `%` を含む entry 名は 0 件）。ただ、「`Ok` ならダングリングしない」という remove_part の中心の保証を、正しい入力で黙って破る。そのため Major とする。

**提案**:
- `_rels` と `.rels` の**認識**は正規化済みの名前で行ってよい。ただし、source と base は raw 名から切り出す。
- 最小の修正は 3 つの手順で済む。
  1. `source_of_rels_part(&rels_name)`（raw）が `Some` ならそれを使う。
  2. raw では認識されず、正規化して初めて `.rels` になるもの（`%5Frels`、`.rel%73`。どちらも非予約文字の %エンコードなので不正な名前）は、何かを指していれば `RegistryNotEditable` で拒否する。
  3. フィルタは `is_rels_part(raw) || is_rels_part(norm)` にする。
- 別の方法として、`find_ci` の代わりに「正規化済みどうしを比べる」探索を使う手もある。ただし、base と raw の Target を混ぜて解決すると、やはり二重デコードになる。そのため raw 名を使う方が筋がよい。
- テスト: source の名前が `xl/a%2541.xml` の場合に `StillReferenced` になること。folder が `x%2541/` の場合に、関係が一致して消えること。

### N18 [Minor] `%2F` を含む source 名の `.rels` が、走査から外れる

- `xl/_rels/a%2Fb.xml.rels` を正規化すると `xl/_rels/a/b.xml.rels` になる。`_rels` が親フォルダでなくなるので、`is_rels_part` が偽になる。
- T1: 画像の削除は `Ok(relationships: [])` になる。`.rels` の `rId1 → media/i.png` は**消えずに残り**、存在しない part を指す。
- 一方、`relationships("xl/a%2Fb.xml")` はこの関係を返す。round 4（raw 名で認識）では走査されていた。
- `%2F` は OPC で禁止されているので、不正な名前である。そのため Minor とする。N17 の修正（raw でも認識する）で、同時に直る。

### N19 [Minor] テストの穴: mutant AH、AM、AP、AQ、AT が生き残る

`mutants5.out` の結果:

| mutant | 結果 | 備考 |
|---|---|---|
| AD `without_ids` で trim しない | 検出 | `padded_attributes` |
| **AH** `mentions` で %デコードしない | **生存** | コミットメッセージは「tests kill … AH」と書いている。しかし `mentions()` は今、**読めない** `.rels` の場合にしか使われない。round 4 の AH のテスト形（`xl\calc%43hain.xml`）は、パースできる側の新しい判定に吸収された（そちらに当たる AU は検出される）。読めない `.rels` が `calc%43hain.xml` を含む場合に `RegistryNotEditable` になることを確かめるテストがない。 |
| AI、AJ | 検出 | `padded_attributes` |
| AK | 検出 | `safety_net_is_per_relationship` |
| AL open の重複を ASCII の大小文字だけで比べる | 検出 | `percent_equivalent_names` |
| **AM** `is_ascii_hexdigit` の検査を外す | **生存** | round 4 の S4（`%+1` と `%01`）を固定するテストがない。 |
| AN `\` を区切りとみなさない | 検出 | |
| AO 「既存 part に解決するなら拒否しない」を外す | 検出 | `safety_net_does_not_over_refuse` |
| **AP** 安全網で、空の `file_name` を除外しない | **生存** | ディレクトリ entry のテスト（`xl/`）には、`/` で終わる Target がない。 |
| **AQ** `mentions()` で、空の `file_name` を除外しない | **生存** | ディレクトリ entry と、読めない `.rels` を組み合わせたテストがない。 |
| AR、AS（raw 名に戻す） | 検出 | `percent_encoded_rels_names_are_scanned` |
| **AT** RegistryPart の判定を raw 名にする | **生存** | `xl/%5Frels/x.rels` そのものを削除しようとしたときに `RegistryPart` になることを、確かめていない。 |
| AU 安全網のセグメントを %デコードしない | 検出 | |
| AV 安全網を外す | 検出 | |

提案: 次の 5 つのテストを足す。AH の主張は、テストを足すまでコミットメッセージから外す。
- 読めない `.rels` が %エンコードした名前を含む場合
- `a%+1.xml` と `a%01.xml` が両方開ける場合
- `Target="sub/"` を持つ `.rels` がある状態で、ディレクトリ entry を削除する場合
- 壊れた `.rels` がある状態で、ディレクトリ entry を削除する場合
- `%5Frels` の `.rels` そのものを削除する場合

### N20 [Minor・文書] 文言が実際の挙動とずれている

- `RemoveRefusal::UnresolvedMention` の doc（`package.rs:98-102`）は、「or an unreadable relationships part mentions the file name」と書いている。しかし、その場合のコードは `RegistryNotEditable` を返す（`package.rs:484`、`492`、`498`）。メソッドの doc（`package.rs:405-406`）の方が正しい。
- `Display`（`package.rs:128-130`）は「mentions the part's file name but no relationship there resolves to it」と書いている。しかし N12 の修正後は、同じ `.rels` に一致する関係があっても出る（S1）。「a relationship there has the part's file name but resolves to no part」などに直す。
- README の remove_part の行は「`` `\\` counted as a separator ``」と書いている。コードスパンの中なので、バックスラッシュが 2 つ表示される。`` `\` `` にする。

### メモ（指摘ではない）

- **安全網は round 4 より狭い**（T2）。
  - `file:calcChain.xml`、`calcChain.xml%23x`、`calcChain.xml%2F`、`calcChain.xml.`、`calcChain.xml%20`、`x:calcChain.xml` は、拒否されなくなった。どれも、そのまま `Ok` で残る。
  - round 4 では、`file:…` と `%23x` は生テキストの判定で拒否されていた。
  - どれも OPC では対象 part の名前ではない。README の新しい説明（「最後のセグメントが file name」）とも合っている。そのため指摘としない。
- **偽の既存 part**: `sub\..\calcChain.xml` は、通常は拒否される。ところが、entry 名が `xl/sub\..\calcChain.xml` の part が実在すると、`find_ci(&resolved)` が成り立つので `Ok` になる。バックスラッシュを含む entry 名を自分で作らないかぎり起きないので、敵対的な入力に限られる。
- **`TargetMode="external"`**（小文字）は Internal とみなされ、関係が消える（R6）。enumeration の値としては不正である。挙動は round 4 と変わらない。
- **U+FFFD への潰れ**: `a%FF.xml` と `a%FE.xml` は、今も重複として open で拒否される（S4）。ADR の「nothing valid is rejected」は、不正な UTF-8 の %列を IRI として正しくないものとみなせば成り立つ。気になるなら、デコードの結果を `Vec<u8>` のまま比べる。
- **Err の後に状態が変わる経路**: T3（`UnresolvedMention`、`RelationshipTypeNotAllowed`、`StillReferenced`。CT を Parsed にした状態で）では、バイトも state も変わらなかった。コーパスでも、1 ファイルあたり最大 3 件の拒否で `remove_part` を実際に呼び、0 件だった。
- **コーパスでの実際の削除**: `check_remove_part` が `Ok` だった 22,764 part のすべてを、実際に削除した。続けて保存し、開き直し、全 `.rels` と Override を確かめた。
  - 削除した名前に解決する Internal の関係が残った例は 0 件だった。
  - 開き直しの失敗は 0 件、panic は 0 件だった。
  - `check` が `Ok` で `remove` が `Err` になる例も 0 件だった。
- `cargo test -p ooxml-core` はすべて通過した（package の 31 件 + ignored 1 件。ignored の 1 件も `--release --ignored` で通過した）。`clippy --all-targets -D warnings` もクリーンだった。

## 判定

**converged: no**。

- Blocker: なし。
- Major: N17 の 1 件。正規化済みの名前から source と base を求めるので、二重にデコードされる。`xl/a%2541.xml` のような OPC として正しい名前で、`Ok` のまま `r:embed` がダングリングする。N14 の修正による退行である。
- Minor:
  - N18: `%2F` を含む source 名の `.rels` が走査されない。
  - N19: mutant AH、AM、AP、AQ、AT が生き残る。AH はコミットで「検出される」と書かれている。
  - N20: `UnresolvedMention` の doc と `Display`、README の `\\`。
- N11〜N16 は、それぞれの趣旨としては解消した。コーパスでの過剰な拒否は 0 件、ダングリングも 0 件である。N17 は、source と base を raw 名から求める小さな修正で直り、N18 も同時に片付く。N17 を直して回帰テストを足せば、残りが Minor だけでも収束と判定してよい。
