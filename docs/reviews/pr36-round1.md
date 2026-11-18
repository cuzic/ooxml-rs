# PR #36 レビュー（Opus, round 1）

対象: `test/fuzz-core` @16e6999（base c4116e9。origin/main 1cf8132 とは衝突なしでマージ可能）。
検証: worktree を scratch にコピーして実行（Rust 1.99.1 / nightly 1.102.0 2026-11-13、cargo-fuzz 0.13.2、32 コア）。

## 結論（先に）

**再現テストは 8 件とも主張どおりに失敗し、原因の推定（F-1 の BOM 二重オフセット、F-2 の `take(compressed_size)` のコピー、F-6 の `with_capacity(file.size())`、F-7 の `any(eq_ignore_ascii_case)`）もコードと実行結果で裏付けられた。**
ハーネス・スモーク・ワークフローの骨格もよい。ただし **F-6 の重大度と「abort は見つからなかった」という結論は誤り**で、ZIP64 では
**459 バイトの入力で `Package::from_bytes` がプロセスを abort させる**（overcommit があっても防げない）。これは所見文書の主結論を変えるので Major とする。
ほかはワークフローの硬化（timeout・nightly / cargo-fuzz の固定・permissions）と、テストとフィルタの細部で、いずれも Minor。

- Major 1 件（M1）、Minor 9 件（m1–m9）
- 判定: **converged: no**（M1 の文書修正のみで次ラウンドは収束見込み）

## 確認したこと

| 項目 | 結果 |
|---|---|
| `cargo test -p ooxml-robustness --test fuzz_findings -- --ignored`（debug） | 8/8 失敗。F-1 の 2 件は `parse.rs:221` の `debug_assert_eq!(start, before)`（6 vs 3、63 vs 60）で落ちる |
| 同（release） | F-1 はテスト自身の assert（span が `"<ite"`、xlsx は `edited workbook reopens` の expect）で落ちる。F-7 は 0.055 s → 0.82 s（比 約 15） |
| 通常の `cargo test -p ooxml-robustness` | 8 ignored、smoke 5 件 pass（3.7 s / 32 コア、2 コアに制限して 5.1 s、1 スレッド 1 コアで 7.3 s） |
| `cargo clippy --workspace --all-targets --all-features -D warnings` | 警告なし |
| `cargo +nightly fuzz build` | 6 ターゲットとも build 可（16 s / 32 コア） |
| 各ターゲットを 15 秒（seed corpus 付き） | xml_t1b・lens_ops・xlsx_edit・package_roundtrip で新しい失敗なし。ただし `package_roundtrip` は `-max_total_time=15` で **70 秒**、`pptx_edit` は 120 秒で終わらず（下記 m1） |
| F-6/F-7 の修正を scratch で仮適用（`HashSet` 重複検査、`with_capacity(min(size, 1 MiB))`） | 両テストが pass に変わる（ガードとして機能）。F-7 の比は負荷下 12 回で 3.90–4.94 |
| F-6 の ZIP64 変種（scratch の probe） | 下記 M1 |

## 所見

### M1 [Major] F-6 は High。「crate 内で panic / abort は見つからなかった」は誤り

**場所**: `docs/testing/fuzz-findings-2026-11.md` 12–14 行（結論）、21 行（表）、104–113 行（F-6 の影響）

`file.size()` は ZIP64 では u64 で、上限がない。scratch で `[Content_Types].xml`（または `a.bin`）を ZIP64 エントリにし、
中央ディレクトリの zip64 extra の uncompressed size を 2^40 / 2^62 にした **459 バイトの zip** で:

```text
Package::from_bytes(...)            # [Content_Types].xml の場合は open だけで
memory allocation of 1099511627776 bytes failed
Aborted (core dumped)   exit=134    # 2^62 でも同じ（capacity overflow の panic ではなく abort）
```

overcommit の有無に関係なく、x86-64 の通常の Linux で abort する。`handle_alloc_error` は unwind しないので
`catch_unwind` でも防げず、サービスで受け付けると 1 リクエストでプロセスごと落ちる。ooxml-xlsx / -pptx / -docx の `from_bytes` も同じ経路。
現在の文書の「Linux の overcommit 下では…実害が出ないことが多い」「Medium」「crash は見つからなかった」はこの点で読者を誤らせる。

- ファザーが届かなかったのは seed に ZIP64 のエントリが 1 つもないため（74 ファイル中 0、下記 m8）。
- `known::f6_huge_declared_size`（> 256 MiB）はこの形も除外しているので、フィルタの範囲は正しい（`OOXML_FUZZ_ALL` なしでは skip、ありでは ASan `allocation-size-too-big` を確認）。

**修正案**:
1. F-6 を **High** に上げ、結論を「panic はないが、ZIP64 の申告サイズで open 時に abort する（F-6）」に直す。影響の段落に上の再現を書く。
2. 再現テストに ZIP64 の変種を足す。そのまま書くと `--ignored` 実行時にテストバイナリ全体が abort して他の結果も失われるので、
   子プロセスで実行する（`std::env::current_exe()` + `--exact <名前>` + 環境変数でガードし、親は終了ステータスが正常であることを assert）。
3. 修正の方向に「`size()` を信用しない: `min(size, compressed_size × 定数, 上限)` で確保し、`take(上限 + 1)` で読んで超えたら `Err`」を明記（I-2 と同じ上限）。

### m1 [Minor] fuzz ワークフローに時間上限がなく、`-max_total_time` は seed の初回実行を含まない

**場所**: `.github/workflows/fuzz-smoke.yml` 46–67 行

libFuzzer は初期コーパスの全件実行が終わってから `max_total_time` を数える。手元（32 コア機、単一プロセス）で
`package_roundtrip` は 15 秒指定で 70 秒、`pptx_edit`（seed 971 件、ASan 下で 1 件ずつ pptx を open/edit/save）は 15 秒指定で 120 秒の外側 timeout に達して終了しなかった（初期コーパスの実行が終わらない）。
2 コアのランナーでは各ジョブが「cargo-fuzz の install + nightly build + 初期コーパス + 60 秒」になり、どれも `timeout-minutes` がないので
既定の 360 分まで走りうる。週 1 回とはいえコストの上限がない。

**修正案**: 各ジョブに `timeout-minutes`（例: smoke 20、fuzz 30）。seed を `-runs` ではなく件数で絞る（例: `seed_corpus` に上限、または `-max_len` と小さい fixture のみ）か、
`-max_total_time` の説明を「初期コーパス実行の後の時間」と README / 入力の description に書く。

### m2 [Minor] nightly と cargo-fuzz が未固定、`fuzz/Cargo.lock` を ignore している

**場所**: `fuzz-smoke.yml` 55・59–60 行、`fuzz/.gitignore` 5 行

`dtolnay/rust-toolchain@nightly` は毎週違う nightly になり、`cargo install cargo-fuzz --locked` は最新版を毎回ソースから build する。
`fuzz/` は独立ワークスペースなのでルートの `Cargo.lock` が効かず、Cargo.lock も ignore しているため、zip / quick-xml などが
CI（ルート lock）と違う版で解決されうる。週次の失敗が「コードの回帰」か「ツールチェーン / 依存の更新」か切り分けられない。

**修正案**: `dtolnay/rust-toolchain@master` + `toolchain: nightly-2026-11-13`（所見の実行環境と同じ）、`cargo install cargo-fuzz@0.13.2 --locked`
（または `taiki-e/install-action`）、`fuzz/Cargo.lock` をコミット（ルートの lock からコピーして `cargo update -p libfuzzer-sys` だけ）。

### m3 [Minor] ワークフローの `permissions` と入力の扱い

**場所**: `fuzz-smoke.yml` 全体、25・45 行

`permissions:` がなく、GITHUB_TOKEN はリポジトリ既定（write の可能性）になる。このワークフローはトークンを使わないので
`permissions: contents: read` で足りる。`${{ inputs.seconds }}` / `${{ inputs.smoke_iters }}` を `run:` に直接展開している
（dispatch できるのは write 権限者だけなのでリスクは低いが、定石どおり `env:` 経由にして数値か検査する）。
アクションはタグ固定（`@v4`/`@v2`）で `ci.yml` と同じ流儀なので、SHA 固定はリポジトリ全体の方針として別途でよい。secrets は使っていない（問題なし）。

### m4 [Minor] F-7 は PR #14 で既に線形化されている

**場所**: 所見文書 115–123 行

`origin/core/remove-part`（PR #14）の `from_bytes_with` は `HashSet` + `normalize_part_name` で重複検査を線形にしている（review N11）。
文書にこれを書かないと二重に修正される。#14 がマージされた時点で `f7_*` の `#[ignore]` を外す、と書くのがよい。
なお #14 でも `find()` / `find_ci()` は線形探索のままなので、文書の「part ごとの `contains` / `read_part` で二乗になりうる」は #14 後も残る（別項として残す価値あり）。

### m5 [Minor] F-7 のテストは un-ignore 後にわずかに揺れうる

**場所**: `tests/fuzz_findings.rs` 193–204 行

修正後の比は 3.9–4.9（閾値 8）で余裕はあるが、小さい側が 12 ms 程度（debug・32 コア）なので、2 コアの CI で他テストと並行すると
1 回のスケジューリング遅延で比が跳ねうる。各 n を 3 回測って最小値を使えば十分に安定する。逆方向（二乗なのに pass）の心配はない（未修正で比 14–15）。

### m6 [Minor] 再現テストの「拒否なら OK」の早期 return が、無関係な理由でも pass させる

**場所**: `tests/fuzz_findings.rs` F-1 xlsx（91・93 行）、F-2（109–110 行）、F-6（174 行）

例えば F-1 xlsx は `Workbook::from_bytes` が別の理由で Err になっても、`set_number` が別の理由で拒否されても pass する。
`poi/51850.xlsx` が将来 Phase の変更で編集拒否になると、F-1 が直っていなくても静かに緑になる。
**修正案**: 同じ手順を BOM なしで行う対照（open でき、`set_number` が Ok、再オープンで 7.0）を同じテストで先に assert する。
F-2・F-6 も「パッチ前の zip は open / read できる」を先に assert する。F-6 の `LARGEST` はプロセス共有で並行テストの確保も数えるが、
誤って pass する方向ではない（64 MiB 超の単一確保をする他テストはない）ので現状のままでよい。

### m7 [Minor] `known.rs` のフィルタの一部が所見の形より広い

**場所**: `src/known.rs` 54–60 行（F-4）、`src/package.rs` 23–28 行（F-1）

- `f4_inner_xml_decl` は先頭 4 バイト以降に `<?xml␠` があれば入力全体を T1b と lens から外す。プロローグでの宣言の重複（`<?xml?><?xml?><a/>`）、
  コメントや CDATA の中の文字列も外れる。スモークの変異トークンに `<?xml version="1.0"?>` があるので、T1b スモーク入力の 1 割程度が丸ごと検査されない。
  ルート要素の開始タグより後に限る、または「失敗したら、宣言を除いた入力で再検査して同じなら既知」とする（入力ではなく失敗の形で仕分ける）とよい。
- `check_package` は**どれか 1 つ**の part が BOM 2 つで始まると、ゼロ編集の L1 検査ごと全体を skip する。F-1 はゼロ編集では顕在化しない（文書 70 行）ので、
  L1 の検査は残し、`force_rerender` の対象選択から外すだけでよい。
- F-2 / F-3 / F-5 / F-6 のフィルタは所見の形に絞れている（F-2 の `Err(_) => true` は `from_bytes` も同じ `by_index_raw` で Err になるので実害なし）。

### m8 [Minor] ファザーが届いていない面

seed（74 ファイル）を調べると ZIP64 0 件、data descriptor 付き 4 件、Strict 2 件、非 zip（CFB）2 件。次を勧める:

1. **ZIP64**: zip64 extra を持つ seed を `seed_corpus` で合成する（M1 はこれで見つかったはず）。I-2 の上限もここで検査できる。
2. **ooxml-docx**（main にマージ済み、#13）: `docx_edit` ターゲット（open → 本文の読み取り → ADR-0004 の編集 → 保存・再オープン、拒否なら無変更）。PR の base が #13 より前なので未対応なのは理解できるが、文書の「対象」に未対応と書く。
3. **data descriptor**（flag bit 3）: `raw_copy_file` が descriptor 付きエントリを L1 一致でコピーするかの専用 seed と、descriptor の値とヘッダの値が食い違う変異。
4. **CFB / 暗号化検出**: `CFB_MAGIC` は先頭一致だけなので、CFB の先頭数バイトを壊した入力や、zip の前に前置データ（SFX 形式）がある入力で `EncryptedPackage` / Err / L1 のどれになるか。
5. **Strict**: `StrictNotSupported` は package rels だけを見る。Strict の名前空間を part 側にだけ持つ入力（rels は Transitional）の扱い。
6. **Package の書き込み API**（`replace_part`、#14 の `remove_part`）に「Err ⇒ 無変更」の検査がない。

### m9 [Minor] 文書の細部

- F-2 の「148 バイトから 286 バイトに変わる」は zipcmp の見え方（どちらも末尾まで読む）で、実際には
  **出力の `a.bin` のデータ領域に入力の中央ディレクトリと EOCD（148 = データ 10 + CD 116 + EOCD 22）がコピーされ、ヘッダは 0x7FFF0000 のまま**。
  こう書く方が影響（壊れた入力が別の壊れ方で出力される）が伝わる。
- `zipcmp::assert_l1_except` は入力を raw で読めないと**何も検査せず return** する。F-2 のもう 1 つの形（データ開始が末尾より後）はここで素通りし、
  F-2 修正後も「Package は受け付けたが raw では読めない」入力の L1 は検査されない。Package が `to_bytes` まで Ok なら panic する方が oracle として強い。
- pptx の検査は `set_text`（kind 0）しか読み戻さない。保存・再オープン前後で全 text body の `text()` が一致することを足すと、kind 1–8 の編集にも oracle が付く。
- 文書 58 行の「debug で約 12 秒」はビルド込みの値に見える（テスト実行自体は 2 コアで約 5 秒）。どちらかを明記。

## 重大度の評価

| ID | PR の評価 | 本レビューの評価 | 理由 |
|---|---|---|---|
| F-1 | High | High（妥当） | 入力は非 well-formed だが、`set_number` が Ok で壊れたファイルを黙って保存する。ADR-0001 の中核違反 |
| F-2 | Medium | Medium（妥当） | 壊れた入力からの黙った別の破損。メモリ安全性の問題はない |
| F-3 | Medium | Medium〜Low | 信頼できない入力からは届かない API 契約の穴。順位は下げてよい |
| F-4 | Low | Low（妥当） | |
| F-5 | Low | Low（妥当） | |
| F-6 | Medium | **High** | M1: ZIP64 の 459 バイトで open 時に abort（DoS、catch 不可） |
| F-7 | Medium | Medium（妥当） | CPU DoS。#14 で修正済み（m4） |

## 判定

**converged: no** — M1（F-6 の重大度・結論の訂正、ZIP64 再現の追記）が必要。m1–m3 はワークフローを週次で回し始める前に直すことを勧める。その他の Minor は任意。

## 修正の推奨順（F-1〜F-7）

1. **F-6**（High に格上げ）: 申告サイズで確保しない＋展開上限（I-2 と同時）。open だけで abort に届く。
2. **F-1**（High）: 2 つ目の BOM を拒否し、`debug_assert_eq!(start, before)` を `Err` に格上げ。黙った破損保存。
3. **F-7**: PR #14 のマージで解消（`#[ignore]` を外すだけ）。コストが最小なので早く。
4. **F-2**: open 時に `data_start + compressed_size <= len` を検査して `Error::Package`。
5. **F-3**: `XMLNS_NS` の属性への `set_attr` / `remove_attr` を拒否。
6. **F-4**: 先頭以外の `Event::Decl` を `Error::Xml`。
7. **F-5**: 空の local name / prefix を `Error::Xml`（I-1 の Name 検査とまとめてよい）。
