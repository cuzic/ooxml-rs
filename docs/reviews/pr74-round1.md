# PR #74 レビュー（Opus, round 1）

対象: test/fuzz-docx-replace @ 37bc5b22104b805243b93af58c8e28eb747da272（main の #62 を含む）

## 結論（先に）

`docx_replace` は find の範囲、件数、テキストの読み戻し、世代、段落外のバイト局所性、Err ⇒ byte-identical を、ライブラリとは独立に再計算している。決定的で、CI での実行時間も抑えられている。
ただし ADR-0007 §1.4 の拒否規則は、fuzz オラクルが予測していない。skipped と refused はライブラリの報告をそのまま期待値にしている。
そのため、境界・マーカー・空白の拒否を外す変異と、正しい一致まで拒否する変異（過剰拒否）は、smoke を 50 iters にしても生き残る（M1）。
converged: no（Major 1）。

## 前ラウンドの解消状況

round 1 のため、なし。

## 検証したこと

- オラクルの独立性: 一致範囲（`str::match_indices`）、置換後の段落テキスト（applied から組み立てる）、`replaced` の件数、新しい世代（以前の最大値より大きく、かつ一意）、段落外のバイト列、他パーツの L1、`w:rPr` の列は、ライブラリを呼ばずに期待値を作っている。良い。
- 実行時間: stable smoke（release）は既定の iters=1 で 0.6 秒、50 iters で 12.4 秒。cargo-fuzz は `-max_total_time` と `-timeout=10` で上限がある。512 KiB を超える ZIP では 1 ステップに減らす。
- 決定性: 入力だけで分岐する。`STATS` は smoke テストの集計にしか使わない。flaky になる要素は見当たらない。
- コーパスの扱い: `seed_corpus` と `replay` は既存ターゲットと同じ経路で、引数のディレクトリにだけ書く。問題なし。
- 他の open PR（#66, #72, #75, #76, #77）は fuzz、robustness、`replace.rs` に触れていない。干渉なし。
- `crates/ooxml-docx/src/replace.rs` に手で 7 個の変異を入れ、テストが落ちるかを見た（コミットしていない）。

| 変異 | ooxml-docx tests | robustness `--lib docx_replace` | smoke iters=1 | smoke iters=50 |
| --- | --- | --- | --- | --- |
| A: `MatchCrossesBoundary` を無効化 | 検出 | 検出（hyperlink の手書きケース） | **生存** | **生存** |
| B: `MarkerInsideMatch` を無効化 | 検出 | 検出（bookmark の手書きケース） | **生存** | **生存** |
| C: 複数 run にまたがる一致を全部 `MixedFormatting` で拒否（過剰拒否） | 検出 | **生存** | **生存** | **生存** |
| D: 書式比較を常に同一とみなす | 検出 | 検出 | **生存** | 検出（`assert_uniform_formatting`） |
| E: `UnpreservedWhitespace` を無効化 | 検出 | **生存** | **生存** | **生存** |
| I: run の拒否状態（field、R1–R3）を無視 | 検出 | 検出 | 検出 | 検出 |
| H: 書き込み後に世代を更新しない | 検出 | **生存** | 検出 | 検出 |

## Findings

### M1（Major）: fuzz オラクルが §1.4 の拒否を予測しない。境界・マーカー・空白の受け入れ過ぎと過剰拒否を見逃す

- 根拠:
  - `docx_replace.rs:235-239` では、期待する applied を「attempted から `report.skipped` を除いたもの」として作っている。何を拒否すべきかは、オラクル側で計算していない。
  - `Match::status` と replace の結果が一致するかも確かめていない（ADR-0007: find の status は replace の判定と同じ）。
  - 書式の確認（`assert_uniform_formatting`）は独立している。しかし境界（hyperlink、fldSimple、sdt など）、マーカー（bookmark、comment range）、`xml:space` のない端の空白については、対応するオラクルがない。
  - その結果、変異 A・B・E は 50 iters の smoke（9,700 以上の applied match）を通る。E では applied が 9,767 から 20,838 に倍増しても検出されない。
  - これらを捕まえるのは、`tests::replace_refusals_preserve_bytes` の 4 件の手書きケースと、ooxml-docx 自身のテストだけである。fuzz は変異した文書を探索するが、その中でこれらの規則を一度も検証していない。
  - 過剰拒否（変異 C）は、この PR のどのテストでも検出できない。ADR-0007 の主目的である「rsid で分割された `{{name}}` の置換」がすべて拒否されても、PR のテストは green のままである。smoke の `applied > 0` は単一 run の一致だけで満たされる。
  - 同じクレートの `docx_edit` オラクルは「結果（Ok か、どのエラーか）を予測して一致させる」設計になっている。それと比べても、この target は弱い。
- 修正案:
  - オラクルに、生の XML から §1.4 を判定する簡単な分類器を足す。段落の raw を走査してテキスト位置を数え、一致の内側に次のものがないかを見る。
    - run 以外のラッパーの開始・終了（`w:hyperlink`、`w:fldSimple`、`w:sdt`、`w:smartTag` など）
    - 折りたたまれていない bookmark と comment range のマーカー
    - preserve のない `w:t` の端の空白
  - applied の一致にこれらが含まれていたら fail にする（受け入れ過ぎの検出）。
  - skipped と refused の一致は、理由ごとに次を確かめる（過剰拒否の検出）。
    - `MixedFormatting`: `visible_rpr` が実際に異なること
    - `MatchCrossesBoundary` と `MarkerInsideMatch`: 分類器が境界かマーカーを見つけること
  - あわせて、find の `status` と replace の skipped・refused が一致することを確かめる。
  - 合成文書（`split`、`proofing`）では、`{{name}}` と `{{ab}}` が受け入れられることを決定的テストで固定する。

### m1（Minor）: 書式オラクルが既定の iters=1 では効かない

- 根拠:
  - 変異 D は `ci.yml` の `cargo test --workspace` 相当（iters=1）を通る。50 iters の週次 fuzz-smoke で初めて落ちる。
  - 合成文書は約 30 個のフィクスチャのうち 5 個しかなく、ランダムに選ぶ needle が `mixed` の境界をまたぐ確率は低い。
- 修正案: 合成文書ごとに、全部の部分文字列（短いので数十個）を決定的に走査するテストを `--lib` に足す。

### m2（Minor）: 局所性の確認が段落単位で、ADR-0007 §1.2 と T7 (iv) より粗い

- 根拠:
  - `check_parts` が見ているのは、書き込んだ段落の外側のバイト列と、`w:rPr` の構造的な一致だけである。
  - ADR は、各 run の開始タグと `w:rPr` がバイト単位で同じであること、置換文字を含まない content child がバイト単位で同じであることを求めている（ADR-0007 l.116, l.1021-1023）。
  - 同じ段落の中で、無関係な run の属性（例: `w:rsidR`）や、触っていない `w:t` が書き換わっても検出できない。README は「outside each written paragraph」と書いており、範囲の説明自体は正確である。
- 修正案: 置換前の文書の run の span ごとに、開始タグと `w:rPr` のバイト列を比べる。触れていない run は、run 全体のバイト列を比べる。

### m3（Minor）: オプションの組み合わせを fuzz していない

- 根拠: `ReplaceOptions` は `on_refused` しか変えていない。`Scope::Story`、`Scope::Paragraph`、`RunFormatting::Identical`、`RunFormatting::FirstRun` は一度も通らない。`assert_uniform_formatting` も既定値（IgnoreInvisible）を前提にしている。
- 修正案: `scope` と `formatting` も入力から選ぶ。範囲の期待値は scope で絞る。書式の確認は、`Identical` では比較をより厳しくし、`FirstRun` では省く。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 3 / nit 0）
