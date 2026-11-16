# PR #21 レビュー（Opus, round 1）

対象: `ci/o3-libreoffice` @ `67c8abd`。追加は 1 ファイル `.github/workflows/o3-recalc.yml`（58 行）である。`impl/xlsx-phase1` @ `8331f11` の同名ファイルと差分はない。
照合したもの:
- `impl/xlsx-phase1` の `spikes/xlsx/o3_recalc.py`、`crates/ooxml-xlsx/tests/phase1_o3.rs`、`tests/common/o3cases.rs`、`examples/o3_inputs.rs`。
- `ci.yml`、`.gitignore`。
- downloadarchive.documentfoundation.org の実際の応答（2026-11-16 に取得）。

## 結論（先に）

**未収束（Major 2 件）。** 構成の大筋は良い。

- `workflow_dispatch` だけで起動する。
- `permissions: contents: read` にしている。
- input は `env` 経由で渡している。
- 版を厳密に固定している。

既定の版 25.2.7.2 は archive に実在する（HTTP 200、208,730,836 bytes、Last-Modified 2025-10-28）。fixture `70005-countifs.xlsx` の `docProps/app.xml` も `LibreOffice/25.2.7.2$Linux_X86_64` で、版を選んだ根拠と一致する。

- **M1 完全性の検証が fail-open で、しかも配布元と同じ場所から取った hash に頼っている。** `.sha256` は directory listing には出ないが、MirrorBrain が動的に返す（200、`sha256sum -c` の形式）。そのため今は実際に照合される。しかし取得に失敗すると `|| true` で黙って素通りする。そもそも同じ origin から取った hash では、改ざんを検出できない。
- **M2 「Check」が空振りで通り得る。** LibreOffice が再計算をしなかった場合（`OOXMLRecalcMode` の設定が効かない、など）、LO は cache の値をそのまま書き戻す。すると、kept セルの比較は全件一致し、`compared > 100` も満たして**通る**。その TSV が oracle として commit されると、O3 は恒久的に自明に通る oracle になる。

ほかは Minor が 5 件、メモがある。

## 指摘

### M1 [Major] 完全性の検証は任意（fail-open）で、hash は配布元と同じ場所から取っている

**根拠**
- ワークフローの L38-39:
  ```sh
  curl -fsSL -o "$RUNNER_TEMP/$tgz.sha256" "$base/$tgz.sha256" || true
  if [ -s "$RUNNER_TEMP/$tgz.sha256" ]; then (cd "$RUNNER_TEMP" && sha256sum -c "$tgz.sha256"); fi
  ```
  hash の取得が 404、5xx、timeout のどれで失敗しても、検証なしで `dpkg -i`（root）に進む。
- 実際の応答（`…/25.2.7.2/deb/x86_64/`）:
  - listing に載っているのは `.tar.gz`、`.tar.gz.asc`、`.mirrorlist` だけで、`.sha256` は載っていない。
  - ただし `GET …deb.tar.gz.sha256` は `200 text/plain` を返し、中身は `3f6a85c57d2d16a222b219b5eb42aacda82fa8ce9590c90e2f9b649c6e3d1581  LibreOffice_25.2.7.2_Linux_x86-64_deb.tar.gz` である。`.md5` と `.sha1` も同じように返り、`.sha256sum` は 404 だった。
  - `.mirrorlist` のページにも同じ SHA-256 が載っている。MirrorBrain が動的に生成するものである。
  - 結論として、今の URL で `sha256sum -c` は実際に機能する。ただしそれは、配布元が listing に載せない仕組みに依存した偶然である。
- 同じホストから tarball と hash を取るので、防げるのは転送中の破損だけである。配布元やその経路が改ざんされた場合には、hash も一緒に差し替わる。
- 実害の面: このジョブは root でパッケージを入れる。出力（`expected.tsv`）は owner が oracle として commit する。完全性の弱さは、oracle データの信頼性に直結する。

**提案**
1. 既定の版について、hash をワークフローに**固定**する。値は `3f6a85c57d2d16a222b219b5eb42aacda82fa8ce9590c90e2f9b649c6e3d1581` で、MirrorBrain の `.sha256` と `.mirrorlist` で確認した。ただし、この sandbox の帯域では 208 MB を最後まで取得できず、ファイルそのものからは再計算していない。commit する前に、owner が手元で `sha256sum` して確認すること。照合は `echo "$SHA  $RUNNER_TEMP/$tgz" | sha256sum -c -` とし、**必須**にする。
2. 版を input で変えられる設計を残すなら、`libreoffice_sha256` も必須の input にする。あるいは「版 → hash」の表をワークフローに持ち、表にない版は失敗させる。そうしないと、既定以外の版では「固定」の主張が成り立たない。
3. 任意で、`.asc` を TDF の署名鍵（fingerprint をワークフローに固定）で `gpg --verify` する。`.asc` は archive に実在する。

### M2 [Major] 「Check」は、LibreOffice が再計算しなかったときに空振りで通る

**根拠**
- `o3_recalc.py` は、使い捨ての profile に `registrymodifications.xcu`（`OOXMLRecalcMode=0`、`ODFRecalcMode=0`）を書いてから `--convert-to xlsx` する。この設定が効かない場合、LO は cache された値を再計算せずに保存する。効かない例としては、初回起動の profile 移行で設定が上書きされる、パスや名前が版で変わる、などがある（既定は 1 = never）。
- `phase1_o3.rs` が比べるのは「output で値を**保持した**セル」だけである。しかもその前に、「input の cache が LO の値と一致したセル」に絞る（baseline）。再計算しなければ、LO の値はすべて cache と等しい。そのため baseline は全件通り、比較も全件一致し、`compared > 100` も満たす。**試験は緑になる。**
- 値を消したセル（dependents）は比較の対象外である。だから、Phase1 が値を残しすぎるという偽陰性があっても、この状態では検出されない。
- ケースの定義（`tests/common/o3cases.rs`、`examples/o3_inputs.rs`）には、再計算されたことを示す canary（わざと誤った cache 値を持つセル）がない。`grep canary|stale|recalc` は 0 件だった。
- TSV の header にある `# OOXMLRecalcMode 0 (always)` は、スクリプトが書く固定文字列であって、観測した結果ではない。

**提案**
- `o3_recalc.py` に **positive control** を入れる。例として、`<f>1+1</f><v>3</v>` のような誤った cache を持つ小さな xlsx を毎回変換し、結果が `2` でなければ `sys.exit` する。
- 試験の側にも、「baseline で一致しなかったセルが 1 件以上ある」か、canary の行が TSV にあることを要求する assert を足す。こうすると、空振りの TSV は commit される前に落ちる。

### m1 [Minor] 再現性: locale と timezone が固定されておらず、版の照合もない

- ランナーの既定 locale は `C.UTF-8` である。LO の数値の書式、文字列から数値への変換（`"1,5"` と `"1.5"`）、`TEXT()`、`DATEVALUE()` などは、locale で結果が変わる。ジョブで `LC_ALL=en_US.UTF-8`（必要なら `locale-gen` も）と `TZ=UTC` を明示して、TSV の header に記録する。volatile なケース（`NOW()` など）は試験で除外しているので、TZ が効く範囲は小さい。それでも明示しておくほうがよい。フォントは値に影響しない（`CELL("width")` は volatile として除外される）。
- `soffice --version` を出力するだけで、照合していない。`"$dir/soffice" --version | grep -F "$LO_VERSION"` で失敗させる。`ls -d /opt/libreoffice*/program | head -1` は、`/opt` に別の版があれば別のものを選び得る。`/opt/libreoffice${LO_VERSION%.*.*}/program` のように版から導く。tarball の構成（`LibreOffice_25.2.7.2_Linux_x86-64_deb/DEBS/*.deb`）は glob と合うことを確認した。
- `Cargo.lock` は `.gitignore` に入っている。そのため、依存（zip、flate2 など）の版は実行するたびに解決し直される。o3 の入力の fnv は同じ実行の中で照合するので、ここで偽の通過は起きない。ただし、後で別の依存版で試験を回すと「input changed since the data was generated」で落ち得る。`cargo tree` か、生成した `Cargo.lock` を artifact に含めて記録しておくとよい。
- `dispatch` したブランチの commit SHA が TSV に入らない。`GITHUB_SHA` を header に書くか、artifact 名に含める。

### m2 [Minor] artifact の扱い

- `if: always()` なので、Check が失敗した実行（Phase1 が stale な値を残したことが見つかった、まさにその場合）でも、同じ名前 `o3-expected` で TSV が上がる。owner が取り違えて commit する危険がある。成功したときだけ `o3-expected` にして、失敗したときは `o3-expected-FAILED`（`if: failure()`）のように分ける。
- `if-no-files-found` の既定は `warn` である。`error` にする。

### m3 [Minor] 依存の導入

- `sudo dpkg -i … || sudo apt-get -f install -y` は、`apt-get update` をしていないので、依存の解決に失敗し得る。さらに `-f install` は、壊れたパッケージを**削除**して解決することもある。その場合は、後の `ls -d` で失敗して止まるので、黙って通ることはない。`sudo apt-get update && sudo apt-get install -y --no-install-recommends "$RUNNER_TEMP"/lo/*/DEBS/*.deb` の 1 行にするほうが素直である。

### m4 [Minor] action の固定

- `dtolnay/rust-toolchain@master` は、第三者の**可変な branch** である。`actions/checkout@v4` と `upload-artifact@v4` は可変な tag である。
- 書き込み権限はないので、侵害されたときの影響は、出力の TSV（oracle データ）が汚染されることに限られる。それでも、このワークフローの存在理由は再現できるデータを作ることである。少なくとも第三者の action は commit SHA に固定することを推奨する（`# v…` のコメントを付けて）。
- `ci.yml` も同じ慣行なので、ここだけ変えるか、リポジトリ全体の方針として扱うかは owner の判断でよい。

### m5 [Minor] input の検証と checkout の資格情報

- `LO_VERSION` は `env` 経由なので、シェルへの injection はない（step の `name:` に式を埋めても実行はされない）。ただし値は検証されないまま URL とパスに入る。dispatch できるのは write 権限を持つ人だけなので、実際の危険は小さい。それでも `[[ "$LO_VERSION" =~ ^[0-9]+(\.[0-9]+){3}$ ]] || exit 1` で形を限るとよい。M1 の 2 と組み合わせる。
- `actions/checkout` の既定では、`GITHUB_TOKEN`（read 権限）が `.git/config` に残る。後続の `cargo run`（build script を実行する）と LibreOffice から読める。`persist-credentials: false` を付ける。

## メモ（指摘ではない）

- `permissions: contents: read` をトップレベルに置き、`upload-artifact` にも追加の権限は要らない。適切である。
- `python3 -I` で起動し、xlsx は `zipfile` と `ElementTree` で読む。LO の出力（自分で生成した入力の変換結果）を読むだけなので、XXE や billion laughs の対象にはならない。
- `--convert-to` が変換に失敗しても `soffice` の終了コードが 0 になる場合がある。しかし出力ファイルがなければ `cell_values` が `FileNotFoundError` で落ちる（fail-closed）ので、問題ない。
- TSV の header にある `# generated <UTC>` は実行ごとに変わる。データ行だけを比べれば再現性を確かめられる。

## 判定

**converged: no**。

- Blocker: なし。
- Major:
  - M1: hash の照合が fail-open で、同じ origin の hash に頼っている。固定した SHA-256（と、任意で GPG）で必須の検証にする。
  - M2: 再計算されなかった場合に Check が空振りで通る。positive control（誤った cache の canary）を入れ、試験で assert する。
- Minor: m1（locale、TZ、版の照合、Cargo.lock、commit SHA）、m2（失敗した実行の artifact）、m3（`apt-get` の手順）、m4（action の SHA 固定）、m5（input の形の検証、`persist-credentials`）。
