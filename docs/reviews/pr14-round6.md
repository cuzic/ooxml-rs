# PR #14 レビュー（Opus, round 6）

対象: `core/remove-part` @ `96af1ff`（round 5 の修正 N17〜N20）。
照合したもの: `package.rs` の `plan_removal`、`is_rels_part`、`source_of_rels_part`、`relationships`。`opc.rs` の `normalize_part_name`、`resolve_target`。`tests/package.rs` の追加テスト。README。
プローブ、mutant、コーパスの走査は scratchpad（`review-14r6-26r2/`）の作業コピーで実行した。リポジトリは変えていない。

- 過去のプローブを再実行した: `probe_r3.rs`、`probe_r4.rs`、`probe_r4b.rs`、`probe_r5.rs`、`probe_r5c.rs`。
- 新しいプローブ `probe_r6.rs`:
  - U1: 復号して初めて `.rels` になる名前で、raw テキストの照合をすり抜ける。
  - U2: 読めない `.rels` で、file name に `&` を含む。
  - U3: `%2F` を含む source の base。
  - U4: 新しい拒否で、何も変わらないことを確かめる。
- mutant:
  - round 5 の集合に、N17・N18 の修正を戻すもの、または変えるもの 5 個を足した（`mutants6.py`、20 個）。
  - round 2〜4 の集合も再実行した（`mutants_prior.py`）。
- コーパス 4,176 件: 前回の一覧に、`reference/fixtures/**`、`scratchpad/corpus/files/`、`review-pr22/` の POI のサンプルを足した。

## 結論（先に）

**収束（Major なし）。**

- N17〜N20 はすべて解消した。
  - source と base は raw 名から、一度だけ復号して求めるようになった。`xl/a%2541.xml` も `x%2541/` も `StillReferenced` になる。
  - `%2F` を含む source の `.rels` も走査される。
- コーパスでは、`check_remove_part` が `Ok` の 23,291 part を実際に削除し、保存して開き直した。ダングリングは 0 件、panic は 0 件、Err の後に変化した例も 0 件だった。
- 残りは Minor 1 件と Nit 1 件である。
  - N21: N17 の修正で、`%5Frels` などの「復号して初めて `.rels` になる」名前を、パースせずに raw テキストの照合だけで判定するようになった。そのため、文字参照や `&amp;` を含む Target が照合をすり抜け、`Ok` のまま関係が残る。round 5 では `StillReferenced` だった。ただし不正な OPC 名に限られる。
  - N22: mutant AX が生き残る。

## 解消状況

| round 5 | 状況 | 根拠 |
|---|---|---|
| **N17** source と base の二重復号 | **解消** | `package.rs:490` で、source を `raw_rels.then(\|\| source_of_rels_part(&rels_name))`、つまり raw 名から求める。base も同じ。その後の `find_ci` と `same_part_name` で一度だけ復号される。T1（`probe_r5.rs`）の結果: `xl/a%2541.xml` は `StillReferenced`。`xl/aa.xml` が別にあっても `StillReferenced`。`x%2541/s.xml` も `StillReferenced`。`pct25`（`probe_r5c.rs`）も `StillReferenced` で、保存後の `.rels` と sheet は変わらない。回帰テストは `source_and_base_are_derived_from_the_raw_name`。 |
| **N18** `%2F` を含む source の `.rels` | **解消** | raw 名でも `.rels` と認識する（`package.rs:464`、`469`）。T1 の `xl/a%2Fb.xml` は `StillReferenced`（round 5 では `Ok` でダングリング）。テストは `rels_of_a_source_with_percent_2f_are_scanned`。 |
| **N19** mutant AH、AM、AP、AQ、AT | **解消** | 5 個とも `round5_gaps` で検出される（下表）。 |
| **N20** 文言 | **解消** | `UnresolvedMention` の doc（`package.rs:98-103`）が、読めない `.rels` の場合を `RegistryNotEditable` に振り分けている。`Display` は「a relationship in … has the part's file name but resolves to no part」になった。README は `` `\` `` 1 つで表示される。 |

## 再実行の結果

- **round 3 のプローブ**（R1〜R10）、**round 4 のプローブ**（S1〜S7、AD、REMOVE）、**round 5 の T2〜T4**: 結果は round 5 と同じだった。
  - 変わったのは S3 の `xl/%5Frels/sheet.xml.rels` と `.rel%73` の画像の削除だけで、`StillReferenced` から `RegistryNotEditable` になった。どちらも拒否なので安全側である（ただし N21 を参照）。
  - T4（open）: 50,000 entry で 32 ms、65,000 entry で 53 ms。
- **T3 と U4（Err ⇒ 変化なし）**: `RelationshipTypeNotAllowed`、`StillReferenced`、新しい `RegistryNotEditable`（復号して初めて `.rels` になる名前）のどれでも、バイトも part の状態も変わらなかった。CT と `.rels` を先に Parsed にした場合も同じだった。
- **コーパス**（`corpus6.out`）: 4,176 ファイルのうち 4,055 件が開けた。
  - `check_remove_part` が `Ok` の **23,291 part をすべて実際に削除した**。続けて保存し、開き直し、全 `.rels` と Override を確かめた。
  - 結果: ダングリング 0 件、開き直しの失敗 0 件、panic 0 件、`check` が `Ok` で `remove` が `Err` になる例 0 件、Err の後に変化した例 0 件（1 ファイルあたり最大 3 件の拒否で、実際に `remove_part` を呼んで確かめた）。
  - 拒否の内訳: `RegistryPart` 26,553、`HasOwnRelationships` 18,445、`StillReferenced` 9,358、`SourceNotScannable` 17、`RegistryNotEditable` 3。`UnresolvedMention` は 0 件で、過剰な拒否はなかった。
- `cargo test -p ooxml-core` はすべて通過した（package 34 件 + ignored 1 件。ignored の 1 件も `--release --ignored` で通過した）。`cargo clippy -p ooxml-core --all-targets -- -D warnings` もクリーンだった。

### mutant

`mutants6.out`（round 5 の集合を今のコードに合わせ、5 個を足した。20 個中 19 個を検出）:

| mutant | 結果 |
|---|---|
| AD、AI、AJ、AK、AL、AN、AO、AU、AV | 検出 |
| AH、AM、AP、AQ、AT（round 5 で生存） | **検出**（`round5_gaps`） |
| AR raw 名での認識を外す（N18 を戻す） | 検出（`rels_of_a_source_with_percent_2f_are_scanned`） |
| AR2 復号後の名前での認識を外す | 検出（`percent_encoded_rels_names_are_scanned`） |
| AS source を復号後の名前から求める | 検出（`source_and_base_are_derived_from_the_raw_name` ほか） |
| AY round 5 の求め方に戻す | 検出 |
| AW 復号して初めて `.rels` になる名前を拒否しない | 検出 |
| **AX** 同じ名前を、part に触れていなくても常に拒否する | **生存**（N22） |

`mutants_prior.out`（round 2〜4 の集合の再実行、39 個）:
- 29 個を検出した: A〜J、L、N、O、R、S、U、W、X、X2、Y、Z、AB、AD、AF〜AK。
- 生存したのは T と AC の 2 個だけである。
  - T は、到達しない `match` の腕を足すだけなので等価である。
  - AC は round 4 で等価と判定した（`r:embed` は collapse しないので、trim は安全側）。
- 残りの 7 個は対象外である。
  - M、P、Q、V、Z2、AA、AE の 7 個は、コードが変わってパターンが一致しない。同じ趣旨は、round 5 以降の mutant（AL、AT、AV など）が押さえている。
  - K は `xml/mod.rs` の mutant なので、この PR の範囲外として飛ばした。

## 新しい指摘

### N21 [Minor] 復号して初めて `.rels` になる名前を raw テキストで照合するので、文字参照や `&amp;` の Target がすり抜ける

**根拠**（`package.rs:477-483`、`490-494`。`probe_r6.rs::u1`）

- N17 の修正で、raw 名では `.rels` でなく、復号して初めて `.rels` になる part（`xl/%5Frels/…`、`….rel%73`、`xl%2F_rels/…`）は、パースされなくなった。
- 代わりに `mentions()` で判定する。これは、raw テキストを `normalize_part_name` したものに file name が含まれるかを見るだけである。そのため、XML としては同じ Target でも、次の書き方は照合をすり抜ける（どれも、`xl/s.xml` の `<pic r:embed="rId1"/>` が `rId1` を指したまま）。

  | `.rels` の名前 | Target | 結果 |
  |---|---|---|
  | `xl/%5Frels/s.xml.rels` | `media/i.png` | `RegistryNotEditable`（正しく拒否） |
  | `xl/%5Frels/s.xml.rels` | `media/&#105;.png` | **`Ok`**。関係も `r:embed` も残り、part だけが消える |
  | `xl/%5Frels/s.xml.rels` | `media/i&#x2E;png` | **`Ok`**（同上） |
  | `xl/_rels/s.xml.rel%73` | `media/&#105;.png` | **`Ok`**（同上） |
  | `xl%2F_rels/s.xml.rels` | `media/&#105;.png` | **`Ok`**（同上） |
  | `xl/%5Frels/s.xml.rels` | `media/a&amp;b.png`（part は `xl/media/a&b.png`） | **`Ok`**（同上） |
  | `xl/_rels/s.xml.rels`（正しい名前） | `media/&#105;.png` | `StillReferenced`（正しく拒否） |

- round 5 では、こうした `.rels` もパースして関係ごとに判定していたので、`StillReferenced` だった（N14 の修正）。そのため、N14 の一部が戻ったことになる。
- 同じライブラリの中で、扱いが食い違っている。
  - `HasOwnRelationships` は `find_ci` で `xl/%5Frels/s.xml.rels` を `xl/s.xml` の `.rels` とみなす（U1 の「remove sheet」の列）。
  - 一方、この関係は削除の判定では見られない。
  - なお、公開の `relationships("xl/s.xml")` は完全一致で探す（`package.rs:661-663`）ので、この関係を返さない。
- `.rels` の名前が不正な OPC 名（非予約文字の %エンコード、または `%2F`）である場合に限られる。コーパスには 0 件だった。そのため Minor とする。
  - 最後の `&amp;` の行は、part の名前（`a&b.png`）自体は正しい。
- U2: これとは別に、**読めない** `.rels` の `mentions()` も、`&amp;` で書かれた `a&b.png` を見落として `Ok` になる。読めない `.rels` は誰も解決できないので、推測の照合として許容範囲である。round 4 以来の挙動で、メモにとどめる。

**提案**
- 復号して初めて `.rels` になる part も、**パースできるなら**パースする。そのうえで、復号後の名前から求めた source と base で関係ごとに解決し、一致するか、安全網の条件（最後のセグメントが file name）に当たれば、`RegistryNotEditable` で拒否する。source が曖昧だという理由で編集はしないので、拒否するだけでよい。`mentions()` は、パースできない場合にだけ使う。
- それより簡単にするなら、`mentions()` の照合の前に、テキストの文字参照と定義済み実体（`&#…;`、`&amp;` など）を展開する（quick-xml の `unescape` で足りる）。
- テスト: `xl/%5Frels/s.xml.rels` で、Target が `media/&#105;.png` の場合に拒否されること。

### N22 [Nit・テスト] mutant AX が生き残る

- 「復号して初めて `.rels` になる part が削除対象に触れていなくても、常に拒否する」という変更（過剰な拒否）を、どのテストも検出しない。
- `xl/%5Frels/…` がある状態で、関係のない part を削除して `Ok` になることを確かめるテストを 1 本足すとよい。N21 の修正で判定が細かくなるなら、そのテストが過剰な拒否の回帰も押さえる。

### メモ（指摘ではない）

- **U3**: `%2F` を含む source（`xl/a%2Fb.xml`）の base は、raw 名から `xl/` と解釈される。
  - `xl/media/i.png` がない場合: `xl/a/media/i.png` の削除は `UnresolvedMention` で拒否される（安全側）。
  - `xl/media/i.png` が別にある場合: `Ok` になる。「復号してから解決する」読み方では、この関係は `xl/a/media/i.png` を指すことになる。
  - どちらの読み方も、`%2F` という不正な名前を前提にしている。raw 名で解決するのが、`relationships()` と一致する一貫した選択である。
- **round 5 のメモ**（安全網が round 4 より狭いこと、`TargetMode="external"`、U+FFFD への潰れ、偽の既存 part `xl/sub\..\calcChain.xml`）は、挙動が変わっていない。T2 と S4 で確かめた。

## 判定

**converged: yes**。

- Blocker: なし。
- Major: なし。
- Minor: N21 の 1 件。復号して初めて `.rels` になる名前（不正な OPC 名）が raw テキストの照合だけで判定されるので、文字参照や `&amp;` で書いた Target がすり抜け、`Ok` のまま関係と `r:embed` が残る。round 5 の N14 の扱いからの部分的な後退である。
- Nit: N22 の 1 件。mutant AX が生き残る。
- N17〜N20 は解消した。正しい OPC 名の範囲では、過去のプローブ、コーパスでの実際の削除 23,291 件、mutant のいずれでも、ダングリング、panic、Err の後の変化は見つからなかった。N21 は不正な名前に限られる。そのため収束と判定する。N21 の修正（パースできるならパースする、または照合の前に unescape する）とテスト 1 本は、マージの前に入れるのが望ましいが、必須ではない。
