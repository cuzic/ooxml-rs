# PR #14 レビュー（Opus, round 4）

対象: `core/remove-part` @ `7df7de5`（`1daab3d` round 3 の修正 + origin/main（PR #16）の merge）。
照合したもの: round 1〜3 のレビュー、`package.rs`、`opc.rs`、`tests/package.rs`、README、ADR-0001。
プローブは scratchpad（`review-14r4-21r2/`）の作業コピーで実行した。リポジトリは変えていない。

- round 3 のプローブ `probe_r3.rs`（R1〜R10）を再実行した。
- 新しいプローブ `probe_r4.rs`、`probe_r4b.rs` を書いた。
- `mutants4.py` で、Y、Z、Z2 と、新しく作った mutant 11 個（AA〜AK）を試した。
- コーパスを走査した。`reference/fixtures/**` と scratchpad の全コーパス（`corpus/files/` を含む）を対象とし、内容の hash で重複を除いて 4,102 ファイルになった。

## 結論（先に）

**未収束（Major 1 件）。** round 3 の N7〜N10 は、どれも指摘した形では解消している。

- R1 と R2（ソース part や自分の `.rels` の %同値）は、正しく `StillReferenced` や `HasOwnRelationships` で拒否されるようになった。
- R10（Target や Id の前後の空白）も `StillReferenced` で拒否される。
- R5 の `" calcChain.xml"` は関係を消すようになった。`xl\calcChain.xml`、`%2E/…`、`file:…` は `UnresolvedMention` で拒否される。
- R3 は open の時点で拒否され、R4 ではキャッシュも消える。
- mutant Y、Z、Z2 はすべて検出された。
- コーパスの結果は次のとおり。
  - calcChain を持つ 274 件は、すべて削除、保存、開き直しまで通った。panic は 0 件だった。
  - %同値な entry の組のせいで新たに open を拒否されるファイルは **0 件**だった。entry 名に `%` を含むファイル自体が 0 件である。

残る Major は 1 件である。

- **N11**: open の重複検査は、もともと O(n²) だった。そこに、毎回 2 つの名前を正規化する処理（アロケーションを伴う）が加わった。そのため、**`Package::open` が entry 数に対しておよそ 85 倍遅くなった**。entry 数 20,000（約 2 MB の zip）では、0.81 s が 70.5 s になる。信頼できない入力を開くすべての呼び出し元に効く退行であり、修正は 1 行で済む（`HashSet` を使う）。

ほかに Minor が 5 件ある。

- N12: 安全網の範囲。
- N13: 安全網による過剰な拒否。コーパスで、実在する part 965 件とディレクトリ entry 1,830 件が新たに拒否される。
- N14: `%5Frels` の扱いが場所によって食い違う。
- N15: 生き残った mutant が 6 個ある。
- N16: open での拒否が文書に書かれていない。

## 解消状況

| round 3 | 状況 | 根拠 |
|---|---|---|
| **N7 (a)** ソース part の探索 | **解消** | `find_ci` が `same_part_name` で比べるようになった（`package.rs:427-429`）。R1 は 2 方向とも `StillReferenced{source:"xl/sheet%E7%94%BB.xml"/"xl/sheet画.xml", id:"rId1"}` になり、R2 は `HasOwnRelationships` になる。テストは `remove_part_finds_sources_under_part_name_equivalence`。mutant AF（`find_ci` を ASCII だけに戻す）は検出される。 |
| **N7 (b)(c)** 空白 | **解消** | `get` を `xml_trim` するようにした（`TargetMode`、`Target`、`Id`、`Type`）。ソースの属性も trim して比べる。R10 の 3 形はすべて `StillReferenced` になり、R5 の前後の空白は `Ok(1)` で関係が消える。テストは `remove_part_trims_target_and_id`。キャッシュの側は、`without_ids` が両側を trim する。` rId2 ` を package の関係として probe したところ、キャッシュから正しく消えた。 |
| **N7 安全網** | **入った（範囲は N12、副作用は N13）** | `RemoveRefusal::UnresolvedMention` を追加した。R5 の `%2E/`、`..%2F…`、`xl\…`、`file:…`、`calcChain.xml%23x` は、どれも拒否されて何も変わらない。mutant AA は検出される。 |
| **N8** %同値な entry と CT キャッシュ | **解消** | open が `same_part_name` で重複を拒否する（R3）。`remove_override` も同じ同値性で比べる（R4 で、キャッシュの Override が消える）。mutant Y、AE、AG は `percent_equivalent_names` で検出される。open での拒否は N11 と N16 を参照。 |
| **N9** mutant Y、Z | **解消** | Y は `percent_equivalent_names` で、Z は `nested_dot_rels_relative_target` で、Z2 は 12 個のテストで検出される。 |
| **N10** 文言 | **解消** | README の行を書き直した。ADR-0003 の 1 引数の残りは PR #20 の側（`origin/impl/xlsx-phase1` の L396-400 と L586-587）で 2 引数に直っている。 |

## 新しい指摘

### N11 [Major] open の重複検査が「O(n²) × 正規化（アロケーション）」になり、`Package::open` が大きく遅くなった

**根拠**（`package.rs:200-206`、release ビルド、`probe_r4.rs::s5_open_cost`）
- 今の実装は、entry ごとに、それまでのすべての entry と比べる。比べるたびに、`same_part_name` が `normalize_part_name` を 2 回呼び、`Vec` と `String` を確保して %デコードと小文字化をする。
- 同じ入力で、main の `eq_ignore_ascii_case` と比べた結果:

  | entry 数 | main（ASCII の大小文字だけ） | PR #14 |
  |---|---|---|
  | 2,000 | 12.7 ms | 793 ms |
  | 8,000 | 139 ms | 11.0 s |
  | 20,000 | 814 ms | **70.5 s** |

- entry 数 20,000 の空の stored entry は、zip にして約 2 MB である。zip64 を使わずに作れる 65,535 entry では、外挿すると 10 分を超える。
- `Package::open` と `from_bytes` は、どの crate（xlsx、docx、pptx）でも信頼できない入力に対する最初の呼び出しである。remove_part を使わない呼び出し元まで含めて、全員に効く退行である。

**提案**: 各 entry の名前を一度だけ正規化し、`HashSet<String>`（または `HashMap<String, usize>`）に入れて検査する。O(n) になる。main の ASCII の大小文字の検査も、これで同時に置き換えられる。`find_ci` は remove_part のときにしか呼ばれず、9,003 part で `check_remove_part` が 77 ms だったので、急がなくてよい。ただし、同じ正規化済みの索引を共有すれば揃う。entry 数 20,000 で open が 1 s 未満で終わることを確かめるテスト（`#[ignore]` でもよい）を足す。

### N12 [Minor] 安全網は「その `.rels` で何も一致しなかった場合」の「生テキスト」にしか効かない

`package.rs:483-487`、`459-470`。

- **一致した関係と、解決できない表記が同じ `.rels` にある場合（S1）**: `indices` が空でないので、安全網は走らない。`Target="calcChain.xml"`（rId2）と `Target="xl\calcChain.xml"`（rId3、workbook が `r:id="rId3"` で参照している）を持つ `.rels` では、`Ok` で rId2 だけが消え、rId3 は残る。`%2E/calcChain.xml` でも同じだった。
- **ファイル名を文字参照で書いた場合（S2）**: `Target="xl\calc&#67;hain.xml"` と `"%2E/calc&#x43;hain.xml"` は、どちらも `Ok(0)` で関係が残る。生テキストを正規化しても `calcchain.xml` が現れないからである。一方、`calcChain&#46;xml` はパーサーが解決するので、正しく消える。
- どちらも、安全網の趣旨（「この実装が解決しない表記が、実は対象を指しているかもしれない」）がそのまま当てはまる経路である。実際の writer がこうした表記を出す見込みは低いので、Minor とする。
- 提案: 生テキストではなく、パース済みのツリーで判定する。一致しなかったすべての `Relationship` について、Target（unescape 済み、trim 済み）を %デコードして小文字化し、その最後のセグメントが `file_name` と等しい場合に拒否する。`indices` が空でない場合にも同じ判定を走らせる。パースできない `.rels` は、今の生テキストの `mentions()` のままでよい。この形にすると、N13 の過剰な拒否の大半も同時に消える。

### N13 [Minor] 安全網による過剰な拒否: 汎用 API として、拒否が大きく増えた

コーパスの全 part（75,796 件）に対して、出てくるすべての関係の型を許可して `check_remove_part` を呼び、安全網を無効にした場合と比べた。

| | 安全網あり（PR） | 安全網なし |
|---|---|---|
| `Ok` | 19,969 | 22,764 |
| `UnresolvedMention` | 2,806（うちディレクトリ entry が 1,830） | 0 |

原因は 4 つに分かれる。
1. **ディレクトリ entry**（`xl/`、`_rels/` など）: `file_name` が空文字列になるので、`contains("")` が常に真になる。
2. **同じファイル名の別の part**: docx の `word/glossary/` と `word/` の `styles.xml`、`settings.xml`、`fontTable.xml` などが該当する。サンプル 400 件はすべてこの類だった。S6 の `xl/glossary/media/image1.png` と `xl/media/image1.png` も同じである。
3. **External の関係**: `http://example.com/calcChain.xml`（`TargetMode="External"`）だけを持つ別の `.rels` があると拒否される（S6、R6）。`remove_part_keeps_external_relationships_with_the_same_target` が通るのは、その `.rels` に一致する関係も同居しているからにすぎない。
4. **Type URI の部分文字列**: `xl/x/image` のような拡張子のない名前は、`…/relationships/image` に一致する（S6）。

- 拒否は安全な側に外れるし、Phase1 の calcChain（274/274 が `Ok`）には影響しない。そのため Minor とする。
- ただ、README は「`.rels` that mentions the part's file name in a spelling that does not resolve」と書いている。実際の挙動（解決できる別の part でも拒否する）より狭い説明になっている。
- 提案: N12 の判定（パース済み、セグメント単位、External を除く）に置き換える。空の `file_name` は対象外にする。少なくとも README と doc の説明を実際の挙動に合わせる。

### N14 [Minor] `%5Frels` などの扱いが、場所によって食い違う

- `HasOwnRelationships` の検出は `find_ci(rels_part_name(..))` を使うので、%同値で比べる。一方、走査の対象を選ぶ `is_rels_part` と `source_of_rels_part` は、生の名前の `_rels` と `.rels` を見る。
- probe（S3）の結果:
  - `xl/%5Frels/sheet.xml.rels` と `xl/_rels/sheet.xml.rel%73` を、sheet 自身の `.rels` として扱って sheet の削除を拒否する。
  - ところが、そこにある `rId1 → media/i.png` は走査しない。そのため、画像の削除は `Ok([])` を返し、sheet の `r:embed="rId1"` が残る。
  - `sheet%2Exml.rels` の場合は整合している（`StillReferenced`）。
- OPC は非予約文字の %エンコードを禁じているので、どれも不正な名前である。また `relationships()` もこれを見ない。そのため実害は小さい。ただし、「同値性はどこでも」という主張とは食い違う。
- 提案: `is_rels_part` と `source_of_rels_part` を `normalize_part_name` の結果に対して適用する。あるいは、`%` を含む `_rels` や `.rels` を見つけたら `RegistryNotEditable` にする。

### N15 [Minor] テストの穴: 新しい mutant のうち 6 個が生き残る

`mutants4.out` の結果:

| mutant | 結果 | 備考 |
|---|---|---|
| AA 安全網を外す | 検出 | |
| AB 関係の属性を trim しない | 検出 | |
| **AC** ソース走査で trim しない | 生存 | 過剰に一致する側の変更なので、ほぼ等価。`r:embed` は `xsd:string`（collapse されない）なので、trim は安全側の判断である。 |
| **AD** `without_ids` で trim しない | 生存 | 実害がある。キャッシュの `Id` は trim されずに保持される（`opc.rs:165`）。そのため、package の `.rels` に `Id=" rId2 "` があると、キャッシュに残る。今のコードは正しい（probe で確認済み）が、テストがない。 |
| AE、AF、AG | 検出 | |
| **AH** `mentions` で %デコードしない | 生存 | `xl\calc%43hain.xml` のような形のテストがない。 |
| **AI** `TargetMode` を trim しない | 生存 | `TargetMode=" External"` を Internal とみなして関係を消す方向の退行が、検出されない。 |
| **AJ** `Type` を trim しない | 生存 | `Type=" …calcChain"` のテストがない。 |
| **AK** `file_name` を %デコードしない | 生存 | %エンコードした名前の part を、生テキストの安全網にかけるテストがない。 |

提案: テストを足す。package の `.rels` で ` rId2 ` を使う場合（キャッシュを確かめる）、`TargetMode=" External"`、`Type` の前後の空白、%エンコードした unresolved な表記。AC は等価として除外してよい。

### N16 [Minor・文書] open で decoded-duplicate を拒否することが、open の文書に書かれていない

- open で拒否すること自体は、ADR-0001 と矛盾しない。ADR-0001 は、暗号化されたパッケージや Strict を「黙って誤動作させず、open で `Err` にする」方針である。また、ASCII の大小文字の重複を拒否することは、main ですでに行っていた（`origin/main` の `package.rs:123`）。OPC で同じ part 名になる 2 つの entry は不正なパッケージなので、拒否はその延長として妥当である。
- しかし、ADR-0001 の Layer 1 は「パッケージを開けば、未知の part も含めて保持する」と約束している。その例外として、この拒否は次のどこにも書かれていない。
  - `Package::open` と `from_bytes` の doc（`package.rs:190-193` の Errors は Encrypted と Strict だけを挙げる）。
  - README の `Package::open` の行（L27）。
  - ADR-0001 の Layer 1。
- README で書かれているのは、`remove_part` の行の括弧書きの中だけである。
- 提案: open の doc と README の open の行に、`Error::Package("duplicate part name …")`（ASCII の大小文字を問わず、%デコード後に同じ名前）を明記する。ADR-0001 の Layer 1 か、Phase0 の「open で Err にするもの」の箇条に 1 行足す。
- コーパスでは、新たに拒否されるファイルは 0 件だった（4,102 件中、`%` を含む entry 名を持つファイル自体が 0 件）。実在するファイルを拒否することはない、と言ってよい。

### メモ（指摘ではない）

- **デコーダの癖**（S4）:
  - `u8::from_str_radix` は先頭の `+` を受け付ける。そのため、`%+1` が 0x01 にデコードされる。
  - 不正な UTF-8 は `from_utf8_lossy` で U+FFFD に潰れる。
  - その結果、`a%+1.xml` と `a%01.xml`、`a%FF.xml` と `a%FE.xml` が重複として open で拒否される。
  - どれも OPC では不正な名前なので、拒否の側に外れるだけである。気になるなら、`is_ascii_hexdigit` で 2 桁を確かめる。
  - `a%2Fb.xml` と `a/b.xml`、`a%20b.xml` と `a b.xml` も重複になる。どちらの組も、片方は不正な名前である。
- **空の Id**（S7）: `Id=""` の場合は、ソースの空の属性値（`a=""`）と一致して `StillReferenced` になる。安全な側に外れる。
- **`RemovedPart.relationships` の Id は trim 済みの値になる**（` rId2 ` → `"rId2"`）。呼び出し元が `.rels` の生の値と突き合わせるなら、doc に一言あるとよい。
- **remove の計算量**: 9,003 part、3,000 個の `.rels` に対する `check_remove_part` は 77 ms だった。問題ない。
- **panic と、Err の後の状態の変化**: R9 の奇妙な名前、S1〜S7、コーパスの 75,796 回の `check_remove_part` のすべてで、panic は 0 件だった。plan は `&self` のままで、新しい拒否（`UnresolvedMention`）も plan の中で返る。Err の後に状態が変わる経路はない。
- **`Ok` のまま ID がダングリングする経路**: R1〜R10 と S1〜S7 では、N14（不正な `%5Frels`）を除いて見つからなかった。S1 と S2 で残るのは、関係そのもの（ID は存在し続ける）であって、ダングリングした `r:id` ではない。
- `cargo test -p ooxml-core` は 60 件が通過し、`clippy --all-targets -D warnings` もクリーンだった。

## 判定

**converged: no**。

- Blocker: なし。
- Major: N11 の 1 件。open の重複検査の正規化によって、`Package::open` が entry 数に対して約 85 倍遅くなった（20k entry で 70 s）。`HashSet` で O(n) にする 1 行の修正で済む。
- Minor:
  - N12: 安全網が、一致した関係のある `.rels` に効かず、文字参照で回避できる。
  - N13: 過剰な拒否（実在する part 965 件、ディレクトリ entry 1,830 件）。
  - N14: `%5Frels` の扱いの食い違い。
  - N15: mutant AD、AH、AI、AJ、AK が生き残る（AC は等価）。
  - N16: open での拒否の文書化。
- round 3 の N7〜N10 は解消した。N11 を直せば、残りが Minor だけでも収束と判定してよい。N12 と N13 は、パース済みの Target をセグメント単位で比べる形に直すと、1 つの修正で両方片付く。
