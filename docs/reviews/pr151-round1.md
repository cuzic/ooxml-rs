# PR #151 レビュー（Opus, round 1）

対象: impl/core-perf-stage2 @ 54b351d26eeceb9a486abc107ee810b1f9c1c3bf

## 結論（先に）
挙動は堅い。全 fixture（2,420 part）で「全要素名を defer」した parse が T1、census、span、AC ビット、属性だけの編集後の直列化、release 後の直列化のすべてで full parse と一致した。fixture を変異させた 58,056 件でも、エラーの種類と位置が一致した（深さ 4 の制限付きを含む）。
ただし ADR の Stage 2 の受け入れ条件である property test が CI に無い。そのため、名前空間 snapshot の無効化を外すとプレフィックスが黙って別の URI に解決されるが、この変異が PR の全テストを通る。また `census` は「数えていない」と「0 件」を区別できず、stage 2x で黙った誤りになる。
**converged: no**（Major 2）。

## 前ラウンドの解消状況
round 1 のため該当なし。

## 確認できたこと（プローブ。コミットしない）
- **deferred と full の同一性**: `reference/fixtures` と `round_trip_corpus` の XML part 2,420 件を対象にした。各 part に出る全要素名を `with_defer` と `with_count` に渡し、次を確認した。
  - `serialize` が入力とバイト一致した（T1）。
  - `census` が full の木の要素数と一致した。
  - 各 part の約 40 ノードに属性の変更・削除・追加を行い（対象ノードは展開しない）、full と直列化が一致した。
  - 全ノードで `release_children` した後も直列化が一致した。
  - lockstep の比較で、要素とテキストの span、`may_have_alternate_content` が全ノードで一致した。展開時の panic は無かった。
  - debug ビルドでも同じ結果だった（`deferred_content` の ptr 一致と U1 の `debug_assert` を含む）。
- **エラーの同一性**: 40 万 B 未満の part に、29 種の断片をランダムな位置に挿入した。断片は実体、`<!DOCTYPE`、`<?xml`、重複属性、未束縛のプレフィックス、`xmlns:xml`、`xmlns:q=''`、BOM、`\xff`、閉じタグの交差など。範囲の削除も加えた。合計 58,056 件のうちエラーは 33,574 件で、`Debug` 表現は全件一致した。`with_max_depth(4)` 付きの比較も含む。Ok 同士の場合は T1 と lockstep も一致した。
- **個別の構成**: 次の構成で T1、構造の一致、`force_rerender` 後の一致、エラーの一致を確認した。
  - 兄弟間での同じプレフィックスの再宣言、`xmlns=""`
  - `xml:space` の入れ子
  - CDATA、コメント、PI、文字参照（`&#x10FFFF;`、`&#xD800;`）、CR/LF
  - 遅延内容の中の BOM と NUL
  - ルートを遅延した文書の後続要素
  - `xmlns:xmlns`、`xmlns:xml`、XMLNS_NS を既定の名前空間にした宣言
- **§2.3 の不変条件**: 構造を変える mutator（`insert_child`、`remove_child`、`splice_children`、`retain_children`、`set_children`、`force_rerender`）は `discard_deferral` を通り、`Built` に戻る。`child_mut` と `children_slice_mut` は展開するが、要素数を変えない。lens の降下は通過するノードだけを展開し、到達先のノードは展開しない。`set_attr` で `xmlns` を書くと `NamespaceDeclaration` で拒否され、出力は変わらない（PR のテストで確認）。Detached は再帰コピーで遅延 payload を持たない（`detach_node` の `set_children`）。
- **深さ**: skip では `stack + ancestor_depth + skipped` で絶対深さを数える。10k と 40k の深さの入力は parse 時に拒否された（PR のテスト）。
- **privacy**: `children` は `node.rs` の private な `Children` enum で、外部からはコンパイルエラーになる。
- **AC ビット**: skip 中は直下の AC だけでビットを立て、`close` は上位ビットを保つ。展開と release を経てもビットは残り、全 fixture の lockstep で full と一致した。
- **release**: `&mut self` なので、展開済みの子への参照が残ったまま release することはできない（use-after-release は型で防がれる）。子孫に編集があると `subtree_dirty` で拒否される。テキストの編集は構造の編集として扱われ、`Built` に戻る。
- **ベンチ**: 同じホストで `--stage2` を 1 回実行した（best of 5）。結果は Package open と遅延 worksheet の parse で 398 ms、ピーク **100.5 MiB**、保持 **60.9 MiB**。PR 本文の値を再現した。`--stage1` と main の比較は再実行していない。
- CI: 5 ジョブすべて pass（`gh pr checks 151`）。他の open PR（#140、#144、#152）は core の xml、lens、package に触れていない。

## Findings

### M1（Major）: Stage 2 の property test が CI に無く、名前空間 snapshot の無効化を壊してもテストが通る
- 根拠:
  - CI で走る `DeferredOracle`（`tests/fuzz_regressions.rs:398-430`）には 3 つの穴がある。
    - 各 part の**ルートだけ**を defer している。そのため、入れ子の遅延、兄弟間の `snapshot` の共有、中間の要素の `xmlns` は通らない。
    - 「編集あり」が `force_rerender` だけである。`force_rerender` は `edited_children` で全体を展開するので、§2.3 の「未展開で `own_dirty` なノードの描画」を一度も通らない。
    - seed の再生（`deferred_parser_matches_on_xml_fuzz_seeds`）は `OOXML_XML_SEEDS` が未設定だと何もせずに成功する。CI はこの変数を設定していない。
  - 属性だけの編集と入れ子の遅延を確かめているのは、`tests/deferred.rs` の手書きの 4 ケースだけである。
  - 変異で確かめた。`parse.rs` の `restore_scope` から `self.snapshot = None` を外し、`cargo test -p ooxml-core --release --no-fail-fast` を実行すると、PR の全テストが通る。失敗したのはレビュー側のプローブだけだった。
  - この変異では、`<r xmlns:p='u1'><x xmlns:p='u2'><row/></x><row><p:a/></row></r>` の 2 つ目の row を展開すると、`p:a` が `u2` に解決される。これは Ok での黙った誤りである。
  - ADR-0010 の Acceptance Stage 2 は「deferred と full の直列化が同一であることの property test（属性だけの編集を含む）」と「壊れた入力でエラーが同一であることの property test」を求めている。
- 修正案: CI で走る oracle を追加する。
  - fixture の各 part で、そこに出る**全要素名**を defer して比較する。対象は T1、census、ノードを抜き出した属性の編集と削除と追加、全ノードの release 後の直列化、lockstep（span と AC ビット）。
  - fixture にランダムな断片を挿入した変異で、エラーが同一であることを比較する（`with_max_depth` を小さくした場合も含む）。
  - 実装例はレビューのプローブにある（下記の手順）。実行時間は release で 5 秒以下。
  - seed の再生は、seed が無い場合に失敗させるか、CI で seed を生成する。

### M2（Major）: `census` は「数えていない」と「0 件」を区別できない。parse の順序によって stage 2x の formula census が黙って 0 になる
- 根拠:
  - `XmlDocument::census` は、要求されていない名前に 0 を返す（`xml/mod.rs`）。
  - `xml_part_with` は、parse 済みの part では最初の options を保つ。このため `xml_part(sheet)` が先に呼ばれると、後から `with_count(&[f])` を渡しても `census(f) == 0` になる。PR 自身のテスト `per_part_parse_options_are_cached_and_keep_the_metadata_floor` が、この 0 を正しい値として固定している。
  - ADR §2.1 は、この順序依存を「失われるのは性能だけ」としている。しかし census が失われると値が誤る。
  - 呼び出し側は、文書がどの options で parse されたかを調べられない（`max_depth()` 以外の getter が無い）。そのため誤りを検出できない。
- 修正案: 追加の API で区別できるようにする。例は `census_of(name) -> Option<usize>`（数えていなければ `None`）や、数えた名前を返すアクセサ。ADR §3 に、順序依存の帰結を 1 行追記する。

### m1（Minor）: 100.5 MiB の内訳と 80 MiB への道筋の記録が不正確
- 根拠:
  - ピークは再現した。ただし内訳は 33 MiB の Vec と 33 MiB の Arc の重なりではないと推定する。重なりだけなら約 70 MiB になる。
  - `read_to_end` は 32 MiB から 64 MiB へ倍々で伸びる。`PeakAlloc::realloc` は新旧のブロックを同時に数えるので、ここで 96 MiB と入力 3.4 MiB が数えられる。`shrink_to_fit` でも 64 + 33 MiB が同時に数えられる。合計は約 100 MiB になる。
  - ADR の注記（`docs/adr/0010-core-performance.md` の post-acceptance note）は「inflate/source のピークを減らす必要」とだけ書いている。ADR の Targets が必須としている `Arc::new_uninit_slice` による解凍に触れていない。また、F-6（ZIP ヘッダーの宣言サイズを信用しない）との衝突をどう解くかも書いていない。
  - 保持の 60.9 MiB から見ると、ソースへ直接解凍すればピークは約 61 MiB になり、80 MiB に収まる見込みがある。
- 修正案: 注記に次を書く。
  - ピークの主因が倍々の伸長であること。
  - stage 2x での方法。例えば、`min(declared, limit)` を圧縮率の上限で検査したうえで `Arc::new_uninit_slice` へ解凍し、宣言サイズを超えたらエラーにする。

### m2（Minor）: `content_span` が構造の編集後も元の範囲を返す
- 根拠: プローブで deferred row に `remove_child(0)` を行った。その後も `content_span() == Some(14..30)` で、`is_deferred() == false` だった。ADR §2.4 の用途は、呼び出し側が生の内容を走査することである。このため、編集済みのノードで古い内容を読む誤用が起こりうる。doc の「Original byte range」は正しいが、警告が無い。
- 修正案: `is_deferred()` でないときは `None` を返すか、doc に「`is_deferred()` のときだけ現在の内容と一致する」と書く。

### m3（Minor）: `xml_part_with` で package の `max_depth` が黙って置き換わる
- 根拠: `ensure_parsed` は、バイト数の上限には `min(package, caller)` を使う。一方、深さは呼び出し側の値を使い、metadata の下限だけを適用する。stage 2x で自然に書く `ParseOptions::default().with_defer(..)` を使うと、`open_with` で設定した深さが既定値に戻る（上げる方向にも下げる方向にも）。
- 修正案: doc に `pkg.parse_options().with_defer(..)` を使うよう書く。または、深さにもバイト数と同じ合成規則を適用する。

### m4（Minor）: `release_children` で子の `preserve_attribute_spelling` が消える
- 根拠: プローブで、展開した子に `preserve_attribute_spelling()` を設定してから release した。release は成功し、再展開した子のフラグは `false` だった。出力の意味は変わらないが、オプトインが黙って消える。
- 修正案: 子にこのフラグがあれば release を拒否するか、doc に書く。

### nit
- PR 本文に `Arc::new_uninit_slice` への言及が無い（m1 と同じ）。
- `deferred_parser_matches_on_xml_fuzz_seeds` は、環境変数が無いと黙って成功する。少なくとも `eprintln` でスキップを示すとよい。

## プローブの再現
`src/xml/probe_pr151.rs` を `#[cfg(test)] mod probe_pr151;` で xml モジュールに入れ、`cargo test -p ooxml-core --release --lib probe_ -- --nocapture` で実行した。対象は 5 つのテスト（全名の defer と編集、変異でのエラーの同一性、個別の構成、census、release と `content_span`）。コミットはしていない。

## 判定
converged: no（Blocker 0 / Major 2 / Minor 4 / nit 2）
