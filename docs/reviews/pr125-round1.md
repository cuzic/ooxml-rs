# PR #125 レビュー（Opus, round 1）

対象: impl/xlsx-phase7-pre @ 0d90602a5ddeea5641b990a5245087b79d14e548

## 結論（先に）
converged: **no**。コーパスの数値は PR 本文どおり再現した。clause 1 は 760 + 582 件が一致し、relationship の付け替え 18 件も含む。clause 2 は 35 シートすべて `Protected` で、エントリ単位で無変更だった。Err ⇒ byte-identical はコーパスと手元のプローブで破れなかった。
一方、P7-pre の受け入れ項目のうち 3 つが未達で、Major とする。1 つ目は登録表と ADR ↔ データのテスト。データの欠けが原因で、今日成功する名前変更が拒否に変わる例を 1 つ作れた。2 つ目は §2.3 の書き換え関数が §3.2 に反する点、3 つ目は `plan_writes_after` が無い点。PR が Draft で未完を自分で書いている点とも合っている。

## 前ラウンドの解消状況
round 1 のため該当なし。

## 検証したこと（再現手順）
- **コーパス**: `/tmp/poi-xlsx`（366 ファイル）に calamine の 2 つを加えて使った。`tests/phase7_probe.rs` を base `99e9658` の worktree とこの head で `--release` 実行し、行ごとに突き合わせた。
  - 対象は 334 ブック・811 シート。
  - rename: OK→OK 一致 760、ERR→ERR 同じエラー 10、ERR→OK 6、OK→`Protected` 35。
  - delete: 一致 582、同じエラー 191、ERR→OK 3、OK→`Protected` 33、ERR→ERR でエラーが変わったもの 2。
  - base で成功した rename のうち `.rels` を書き換えたものは 18。
  - OK→ERR の遷移は `Protected` だけだった。
- `#[ignore]` の `protected_corpus_workbooks` と `newly_supported_corpus_locations_match_expected_values` は通過。新しい書き換え箇所 37 件の照合もこの test が行っている。
- **変異**（手書き。コミットはしない）:

  | 変異 | テスト |
  | --- | --- |
  | A: `EXTENSIONS` で CF の URI と行の対応を入れ替え | 落ちる |
  | B: L48 の Refuse 判定を削除 | 落ちる |
  | D: L19 の rename を Unaffected に | 落ちる |
  | F: L37 の delete を Keep に | 落ちる |
  | G: `location!` の scope を Own に | コンパイルで落ちる |
  | J: L71 の rename を Unaffected に | 落ちる |
  | C: `find` が `EXTENSIONS` の対応表を見ない | 残る（`matches` の URI 比較と同じ判定なので等価） |
  | **E: L69 の sqref 行を削除** | **残る** |
  | H: GUID の読み飛ばしを無効に | 残る |
  | I: `accepts_sheet_prefix` が scope を見ない | 残る（Own の行はすべて Unaffected なので現状は等価） |

- `EXTENSIONS` の GUID 11 個は、MS-XLSX §2.2.4.12（learn.microsoft.com、2026-11-18 更新）と一致した。照合したのは worksheet の 8 個、`cfRule` の 1 個、`sortState` の 1 個、`filterColumn` の 1 個。
- CI（`gh pr checks 125`）は全緑。

## Findings

### M1（Major）: 登録表のデータと ADR ↔ データのテストが ADR の受け入れに届いておらず、データの欠けで今日の名前変更が拒否に変わる
- 根拠:
  - **行が足りない**: `locations.rs` の `LOCATIONS` には次の行が無い。
    - L1, L2, L8, L10, L22, L31, L33, L36, L38, L40, L41, L43–L45, L51, L57–L61, L63, L64, L66, L67, L74
  - **列が足りない**: ADR §2.2 が定める列 `{ id, part, scope, path, attr, syntax, g1, copy, rename }` のうち、G1・Copy・テーブル名変更の列が無い。
  - **ADR ↔ データのテストが無い**: Issue #111 と §2.2 は「ADR の表と `LOCATIONS`/`EXTENSIONS` の id・scope・part・path が一致すること」を求めている。実際にあるテストは次の 2 つだけ。
    - `extension_uris_match_the_adr`: URI の集合しか比べず、URI と行の対応は見ない。
    - `registry_accepts_legacy_locations_for_rename`: id ごとの件数を手で書いた表と比べるだけ。ADR-0009 の `allowed()`、非参照の集合とは突き合わせていない。
  - **ignoredErrors の単体テストが無い**: ADR は「`ignoredErrors` を本来の URI では受け入れ、URI と行の対応を入れ替えると拒否する」単体テストを求めている。これが無いので、変異 E（L69 の削除）が残る。
  - **具体的な退行（プローブ）**: ワークシートを次の形にして `rename_sheet("Data","New")` と `delete_sheet("Data")` を実行した。
    - セルの数式が `Data!A1`。
    - `autoFilter/sortState/extLst/ext uri="{0FC57C36-…}"` の下に `ref="A2:A5"` を持つ子がある。
    - 結果:
      - base `99e9658`: **Ok**
      - この PR: `Err(UnrewritableReference { "{0FC57C36-…} <sortCondition> attribute ref" })`。出力はバイト単位で無変更。
    - 原因: `EXTENSIONS` はこの URI を `&[74]` に対応づけているが、`LOCATIONS` に L74 の行が無い。そのため `commit` の extension 走査で `find` が None になる。
    - §2.2 は「既知の URI で、その URI の登録表の場所に**無い**ときだけ拒否する」と書いている。L74 の sheet rename/delete は「–」。だからこれは ADR に反する拒否で、clause 1 と「意図した変更は 2 つだけ」にも反する。
    - コーパスでは L74 は 0 件なので、clause 1 の数値には出ない。
  - **記録されていない拒否**: 変更対象の part にある未知の URI のもとに参照の形をした内容があれば拒否する。これは workbook.xml の ext にも効き、`EXTENSIONS` は worksheet の URI しか持たない。§2.2 どおりの拒否だが、ADR-0009 の post-acceptance note に書かれていない。
- 修正案:
  - ADR の表の全行（L74 を含む）と、G1・Copy・テーブル名変更の列をデータにする。
  - ADR の表を読んで、id・part・scope・path・各規則を `LOCATIONS`/`EXTENSIONS` と突き合わせるテストを足す。`URI → 行` の対応もこのテストで見る。
  - ignoredErrors のテスト（本来の URI では受け入れ、入れ替えると拒否）を足す。
  - extension 走査による新しい拒否を ADR-0009 の note に書く。
  - 全行のデータ化を P7a に回すなら、Delivery 0 を変える ADR note と、その追跡 Issue が要る。

### M2（Major）: §2.3 の `rewrite_formula` が、シート接頭辞付きの削除された参照を裸の `#REF!` にし、§3.2 に反する
- 根拠:
  - 問題の箇所: `formula/structural.rs` の `rewrite_formula` の `AreaOutcome::Deleted` の腕。`text[last..token.start]` の後に `#REF!` を書くので、接頭辞ごと捨てる。
  - §3.2「Deleted references」は、接頭辞付きの参照は `Sheet1!#REF!` にして、書かれた接頭辞（引用符を含む）を残すと決めている。
  - 単体テスト `rewrites_only_references_selected_by_the_sheet_context` は、`Data!B2` → `#REF!` という誤った出力を期待値として固定している。
  - P7-pre では公開 API から届かないが、この関数は P7a がそのまま使う成果物である。
- 修正案: 接頭辞付きの参照では `text[token.start..token.start + area_offset]` を残して `#REF!` を付ける。テストの期待値を `Data!#REF!` に直し、引用符付き（`'My Data'!#REF!`）のケースも足す。

### M3（Major）: §2.1.1 の `plan_writes_after(edit, writes)` が無く、index map が `CellWritePlan` に結びついていない
- 根拠:
  - 入ったもの:
    - `apply_writes_with_insertion` が、呼び出し側でステージ済みの `workbook.xml` を受け取って `calcPr` を書く経路。
    - テスト `calc_flag_is_applied_to_the_callers_workbook`。
    - `Shift::before`/`after`（index map として使える純粋関数）。
  - 入っていないもの: post-edit の座標で検証・挿入を計画する `plan_writes_after` と、それを使って合流させる部分。具体的には、`sst` の追記、`refs_delta` を 1 つの `RegistryApply` に合流させること。
  - Delivery 0 と Issue #111 は、どちらも「the composed `CellWritePlan` variant: the index map, and …」を P7-pre の範囲としている。PR 本文も「`plan_writes_after` の結合は未完」と書いている。
- 修正案:
  - `plan_writes_after` を実装し、index map を壊す変異で落ちるテストを付ける。
  - または、P7b/P7c に回すことを ADR の Delivery に note として書き、owner の了承を取る。

### m1（Minor）: clause 4 の「838 件」はこのコーパスでは定義どおりに測れない
- 838 は ADR-0009 のコーパスの数値である。POI + calamine では 811 シートになる。
- 保護以外の拒否 10 件は、すべて base でも同じエラーで拒否されている。内訳は `BinaryReferencesPossible` 2、`IterativeCalculation` 3、`SharedWorkbook` 3、lex できない式 2。
- 「新しい拒否が `Protected` 以外に無い」という趣旨は満たしている（OK→ERR は `Protected` だけ）。ADR の clause 4 を「base 比で `Protected` 以外の拒否が増えない（811 シート）」と書き直せば足りる。

### m2（Minor）: clause 2 と clause 3 の重なりが PR 本文に書かれていない
- `workbookProtection-workbook_password-2013.xlsx` と `workbookProtection_workbook_structure_protected.xlsx` の delete は、`LastVisibleSheet` から `Protected` に変わる。clause 2 が優先するので正しいが、PR 本文の clause 3 の件数には現れない。
- 「出力が入力とバイト一致」はエントリ単位（無編集で保存したものとの比較）で確かめている。ZIP のバイト一致ではない（0/4）。これは既存の Err テストと同じ解釈なので許容でよい。どちらの解釈かを ADR に一文で書く。

### m3（Minor）: main と衝突している
- #122 が 2026-11-21 にマージされた後、`git merge-tree origin/main HEAD` が `fuzz/fuzz_targets/lens_ops.rs` で衝突する。main 側の `check_leaf_text` の行と、この PR の bulk-removal oracle の両方を残す必要がある。
- main の取り込みは Codex に任せる。

### m4（Minor）: `rewrite_index` が端で拒否し、ADR の「端に固定」と食い違う
- `rewrite_index` は `Shift::after` を通すので、`col max="16384"` や `brk/@max` のように端にある値を insert でずらすと `ShiftOffSheet` を返す。ADR §3.2 と L22 は、これらを端に固定するか切り詰めると決めている。
- `cols` の min/max 対に固定が効くことは、`C:XFD` の `shift_area` でしか確かめていない。
- P7a で使う前に、min/max 対を `Area::Cols` で扱うか、固定に対応した index 関数を作るか決め、テストを付ける。

### m5（Minor）: GUID の読み飛ばしにテストが無い
- 変異 H（読み飛ばしを無効にする）が残る。
- 現状では GUID が `r`/`ref`/`f`/`sqref` の参照として lex されることはないので、実害は無い。`{…}` の値が `ref` 属性にある場合を 1 件テストに足す。

### m6（Minor）: L30 の `controls` は `mc:AlternateContent` の中では一致せず、拒否されたまま
- Excel は通常 `controls` を `mc:Choice Requires="x14"` で包んで書く。登録表の worksheet の行は path の長さが完全一致でないと当たらないので、`linkedCell` は今までどおり拒否される（プローブ P4。base と同じ挙動）。
- ADR-0009 の note は「L30 を書き換える」と書いている。note を「`mc:AlternateContent` の外にあるときだけ」に直すか、L31 と合わせて扱いを決める。

### m7（Minor）: #108（P5d）との干渉
- この PR は sparkline の URI を `locations.rs` のローカル定数として持っている。P5d も x14 の sparkline ext を書く。
- P5d の側で、次の 2 つをすること。
  - URI を `x14.rs` の定数にまとめる。
  - P5d が書いた `sparklineGroup` を、L21 の行が受け入れる（rename/delete で書き換わる）テストを足す。
- 現時点では #108 のブランチは push されていないので、ファイルの衝突は確認できない。

### n1（nit）
- 新しい書き換え経路（`rewrite_registered_formulas`）は、値全体を `quick_xml::escape::escape` し直す。そのため CF/DV のテキストの `"` が `&quot;` になる（プローブ P3）。XML としては正しいが、従来の書き込み経路（`&` `<` `>` だけをエスケープする）と揃っていない。

## 判定
converged: no（Blocker 0 / Major 3 / Minor 7 / nit 1）
