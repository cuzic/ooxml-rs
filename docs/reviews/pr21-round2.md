# PR #21 レビュー（Opus, round 2）

対象: `ci/o3-libreoffice` @ `b9f902c`（`.github/workflows/o3-recalc.yml`）。
照合したもの:
- `impl/xlsx-phase1` @ `ab26233` の `spikes/xlsx/o3_recalc.py`、`crates/ooxml-xlsx/tests/phase1_o3.rs`、`tests/common/o3cases.rs`、`examples/o3_inputs.rs`、`reference/fixtures/xlsx/synthetic/o3_canary.xlsx`。ワークフローファイルは 2 つのブランチで同一である。
- GitHub API と `git ls-remote` による action の SHA。
- downloadarchive.documentfoundation.org の応答（2026-11-16）。

## 結論（先に）

**収束（Blocker も Major もない）。** round 1 の M1、M2、m1〜m5 は、すべて解消している。

- **canary は機能する。** LibreOffice を使わずに確かめた。偽の `soffice`（入力をそのまま書き戻す、つまり再計算しない LO）で `o3_recalc.py` を実行すると、`positive control failed … Sheet1!B1 n 3` で exit 1 になる。canary のファイルがない場合も、版が食い違う場合も exit 1 になる。Rust 側の試験も、canary の入力と出力（3 ケース × 2）の `Sheet1!B1 == "2"` と `canaries >= 2` を assert する。
- **Check が空振りで通る経路は、今の構成では見つからなかった。** 残るのは、canary が LO の「ファイルごとの」再計算の判断を代表しない場合という理論上の穴だけである（メモ 1）。

Minor が 1 件、nit とメモがある。

## 解消状況

| round 1 | 状況 | 根拠 |
|---|---|---|
| **M1** hash の検証 | **解消** | SHA-256 を `case` でワークフローに固定した。表にない版は `exit 1` になり、照合は `sha256sum -c -` で必須（`set -euo pipefail`）である。値 `3f6a85c5…1581` は、今日の時点でも archive の `.sha256` と一致した。content-length は 208,730,836 である。ただし、この sandbox の帯域（約 35 KB/s）ではファイルそのものから再計算できなかった（m6）。GPG は採用していない（任意の提案だった）。 |
| **M2** 空振りの Check | **解消** | positive control を入れた。`o3_canary.xlsx` は `B1 = A1+1` で、cache は 3（正しい値は 2）。Python（L135-137）と Rust（L91-102）の両方で、canary の値が 2 でなければ失敗する。偽の soffice による検証は上に書いた。canary の fixture は Z9 の編集で Phase1 を通るので、出力の側も検査される。edit が失敗した場合はケースが 0 になり、`canaries >= 2` で落ちる（fail-closed）。 |
| **m1** locale、TZ、版の照合、記録 | **解消** | `LANG`、`LC_ALL` は `C.UTF-8`、`TZ` は `UTC` で、job 全体の `env` に置いた。en_US ではないが固定されて記録されるので、再現性の目的は満たす。版はシェル（`soffice --version \| grep -F`）と Python（`O3_LIBREOFFICE_VERSION`）で二重に照合する。パスは `/opt/libreoffice${LO_VERSION%.*.*}` から導く。`GITHUB_SHA`、locale、TZ、`O3_CARGO_LOCK_SHA256` を header に書き、`Cargo.lock` そのものを artifact に入れる。 |
| **m2** artifact | **解消** | 成功したときの upload は `if:` を付けない（= `success()`）。名前は `o3-expected-<sha>` で、`if-no-files-found: error` にした。失敗したときは `o3-FAILED-<sha>`（`if: failure()`）として別に上げる。 |
| **m3** 依存の導入 | **解消** | `apt-get update` の後に `apt-get install -y --no-install-recommends …/DEBS/*.deb` を実行する。 |
| **m4** action の固定 | **解消** | API と `ls-remote` で確かめた。<br>・`actions/checkout@11d5960…` は `refs/tags/v4` と `v4.4.0` の両方と一致する（2026-07-16 の "backport fixes to releases-v4"）。<br>・`actions/upload-artifact@ea165f8…` は `v4` と `v4.6.2` の両方と一致する。<br>・`dtolnay/rust-toolchain@df2a88e…` は、今の `refs/heads/master` の HEAD（2026-11-15、PR #190 の merge）と一致する。 |
| **m5** input の検証、資格情報 | **解消** | `^[0-9]+(\.[0-9]+){3}$` を照合し、さらに `case` の表で限定する。`persist-credentials: false` を付けた。 |

## 指摘

### m6 [Minor] 固定した SHA-256 を、ファイルから再計算したことを確かめられない

- コメント（L51-52）には「re-computed from the downloaded file」とある。しかし、PR にも commit にも、その記録（実行したコマンドと出力）がない。
- reviewer の環境では 208 MB を取得しきれなかった。確認できたのは、MirrorBrain の `.sha256` と一致すること（同じ origin）だけである。round 1 の M1 の要点（同じ origin の hash では改ざんを検出できない）は、owner が別経路で再計算して初めて満たされる。
- 提案: PR の本文に、`sha256sum LibreOffice_25.2.7.2_Linux_x86-64_deb.tar.gz` をどの環境で実行したか（または、別のミラーから取った `.tar.gz` の hash）を 1 行記録する。可能なら、`.asc` を TDF の鍵で `gpg --verify` した結果も添える。ワークフローの変更は要らない。

### nit

- `# v4` のコメントは、実体に合わせて `# v4.4.0`、`# v4.6.2` と書くほうが正確である。`v4` は動く tag なので、更新するときの比較元が分かりにくい。
- Check の `cargo test … -- --ignored` は、ignored なテストが 0 件でも「0 passed」で通る。今は `o3_kept_values_match_libreoffice_recalculation` が `#[ignore]` で 1 件あるので問題ない。ただし、将来 ignore を外したりテストの名前を変えたりすると、黙って空振りになる。`-- --include-ignored --exact o3_kept_values_match_libreoffice_recalculation` にして、出力に `1 passed` があることを grep するか、`--ignored` のまま件数を確かめると固い。

## メモ（指摘ではない）

1. **canary は「別のファイル」である。** LO の `OOXMLRecalcMode` は profile の設定で、1 回の実行の中のすべての変換が同じ profile を使う。そのため、canary が再計算されていれば、ほかのファイルも同じ設定で読まれる。残る理論上の穴は、LO がファイルの属性によって再計算するかどうかを変える場合である。たとえば、生成元（`docProps/app.xml` の `Application`）や `calcPr` による分岐がこれにあたる。
   - canary には `app.xml` がない。合成の fixture と O2 の 200 冊も同じで、canary は大多数の入力と同じ形である。
   - 形が違うのは POI の 4 件（`Microsoft Excel` が 3 件、`LibreOffice/25.2.7.2` が 1 件）× 3 値だけである。
   - LO の oox import にそのような分岐があるかは、この環境では確かめられなかった。より固くしたいなら、POI の 1 件（例: `shared_formulas.xlsx`）に誤った cache を入れた 2 つ目の canary を足せばよい。
2. 偽の soffice による検証の範囲: 「LO が全体として再計算しない」は検出される（exit 1）。「canary だけ再計算する」偽の soffice（`<f>A1+1</f><v>3</v>` だけを書き換える）では通った。これはメモ 1 の理論上の穴をそのまま示しているだけで、実際の LO の挙動ではない。
3. Check が空振りで通る経路をほかにも探したが、どれも fail-closed だった。
   - TSV がない場合: `load().expect` で落ちる。
   - ケースのデータがない場合: `panic!("no expected data")`。
   - 入力と出力の fnv が食い違う場合: assert で落ちる。
   - 比較できたセルが 100 以下の場合: assert で落ちる。
   - LO が出力を書かなかった場合: Python の `cell_values` が `FileNotFoundError` で落ちる。
4. `permissions: contents: read`、入力を `env` 経由で渡すこと、`python3 -I` での起動は、round 1 と同じく適切である。

## 判定

**converged: yes**。

- Blocker: なし。
- Major: なし。
- Minor: m6（固定した SHA-256 を、ファイルから再計算した記録を PR に残す。ワークフローの変更は不要）。
- nit: action のコメントを正確な版にする。`--ignored` の件数を確かめる。
