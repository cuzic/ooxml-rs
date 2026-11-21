# PR #131 レビュー（Opus, round 2）

対象: impl/docx-phase3d-content-controls @ 8d622505d9b6cedc5df746b3cc8177bd1764fb11

## 結論（先に）
round 1 の M1・M2・m1・m2・m3・n1 は、すべて挙動で解消を確かめた。
round 1 で生き残った 4 件と、今回の修正を外す 3 件、合わせて 7 件の手書き変異は、どれもテストで落ちる。
新しいコード（暦の判定、ASCII 英字だけを token とする判定、`w:lastValue` の更新、id のスコープのテスト）を壊しにいったが、
Blocker と Major は見つからなかった。残りは nit 2 件。converged: yes。

## 前ラウンドの解消状況
- M1 解消: `hijri`、`japan`、`taiwan`、`thai` のどれでも、`set_date` は `UnsupportedDateFormat { token: <暦名> }` を返し、出力はバイト一致する（テストは `hebrew`、`korea`、`saka`、`none` も含む）。`w:calendar` が無い場合は `gregorian` として扱われ、素の `<w:date/>` は `3/2/2026` になる（ECMA の既定と一致する）。`w:date` は `sdtPr` の直下にしか書けず、スタイルからは継承されない。そのため、読む要素は `sdtPr/w:date/w:calendar` だけで正しい。バインドしたピアでも次を確かめた（プローブ）。
  - グレゴリオ暦の主コントロールに `set_date` を呼ぶと、hijri のピアで全体が拒否され、出力はバイト一致する。
  - `set_date_with_display` でも同じく拒否される。暦が違うと `same_picture` が偽になり、ピアは自分で整形しようとして Err になる。
  - プレーンテキストの主コントロールから store 経由で hijri のピアに書く場合も、同じく拒否される。
- M2 解消: 囲い込みの拒否について、バイト一致のテストが加わった。インラインでは、フィールドの instrText と結果の run、囲んでいる SDT。段落では、段落をまたぐフィールド、`moveFrom` の範囲、段落記号の `w:ins`、SDT の中、`sectPr`、連続していない範囲、ストーリーをまたぐ範囲。`BindingMismatch` はインラインと段落の両方。変異の結果は次のとおり。
  - `wrap_inline` の `check_settable` を外すと、`inline_wrap_refuses_field_runs_and_enclosing_controls` が落ちる。
  - `wrap_blocks` の `check_control_runs` を外すと、`block_wrap_refuses_fields_and_revisions` が落ちる。
  - id の収集を最初のストーリーに限ると、`controls::wrap::tests::wrap_skips_ids_in_other_stories` が落ちる。
- m1 解消: `yyyy年M月d日`（ja-JP、Gregorian 系の 6 種類の暦）は `2026年3月2日` になる。サポート外の token は引き続き拒否され、出力はバイト一致する（プローブ）。拒否される token の例: `yyyy年 ggg` の `ggg`、`gggee年M月d日` の `ggg`、`e`、`ee`、`gg`、`(aaa)` の `aaa`、`dddd`、小文字の `am/pm` の `a`。`is_ascii_alphabetic` を `is_alphabetic` に戻す変異は、`gregorian_date_pictures_accept_japanese_literals` で落ちる。
- m2 解消: `INFO Title` のケースが加わり、拒否されたコードの値まで照合している。`INFO ` の判定を消す変異は `core_properties_follow_package_relationship_and_gate_fields_in_other_stories` で落ちる。id のスコープは M2 のとおり。
- m3 解消: 次の場合に、既存の `w:lastValue` が選んだ値に更新される。
  - ドロップダウンとコンボボックス。コンボボックスの自由テキストも含み、`&"<` はエスケープされる。
  - store 経由で更新されるピア（プローブ: プレーンテキストで `k1` を書くと、ドロップダウンとコンボボックスのピアの `lastValue` が `k1` になる）。
  
  属性が無い場合は追加されない。値がリストに無い場合は `InvalidValue` になり、出力はバイト一致する。更新を `remove_attr` に置き換える変異は `list_selection_updates_existing_last_value` で落ちる。ADR §6.1 の表にも 1 文が加わった。
- n1 解消: `wrap_in_control` のドキュメントコメントと、ADR の D3d 注記に記載された。

## Findings

### n1（nit）: 全角の英字（`ｙｙｙｙ`）は token ではなくリテラルとして扱われる
- 根拠: `ｙｙｙｙ年` は `Ok` になり、表示は `ｙｙｙｙ年` のままになる（プローブ）。Word が全角の token を受け付けるかは未確認 *(u)*。ASCII だけを token とする判定は、Q6 の token の一覧と矛盾しない。
- 修正案: 必要なら、ADR の注記の「Non-ASCII picture characters … are literals」に全角英字も含むことを明記する。

### n2（nit）: ライブラリの単体テストが、統合テストのヘルパーを `#[path]` で取り込んでいる
- 根拠: `wrap.rs` の `#[cfg(test)] #[path = "../../tests/common/mod.rs"] mod test_package;`。`tests/common` の変更が lib のテストにも影響する。CI の clippy は通っている。
- 修正案: 将来 `tests/common` が lib から使えない形になったら、単体テストの側に最小限の docx 生成を持たせる（follow-up でよい）。

## その他の確認
- CI: 5 ジョブすべて pass（test/clippy/codegen、python-docx、python-pptx、openpyxl、libreoffice）。
- 干渉: origin/main と `git merge-tree` で衝突しない。open PR の #125、#132、#134、#135、#136 とも、それぞれ merge-tree で衝突しない。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）
