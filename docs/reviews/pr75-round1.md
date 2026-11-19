# PR #75 レビュー（Opus, round 1）

対象: impl/core-perf-stage1 @ 9dbda0ba7fc8f51a45111014298072a0698db328

## 結論（先に）
解析器の中身は main の解析器と同じ結果を返す。frozen parser との照合（全 fixture と生成入力）に加え、
手元で 40,000 件のランダムなトークン列（実体参照・CDATA・CR/LF・`xmlns=""`・xml 名前空間の別名・未定義実体を含む）を
frozen と照合し、木・スパン・エラー・T1・全再描画・T1b がすべて一致した。interner の前段照合はキー全体を比べ、URI のアドレスは
`Scope.interned` が解析中ずっと保持するので、アドレスの再利用による誤一致はない。編集 API に新しい拒否は無く、`apply_prefixes`
の途中失敗は clone の上で起きるので Err ⇒ byte-identical も保たれる。
ただし `CompactSpan` が ADR の配置（u32 4 個、フラグは同じ語に詰める）と違い 24 B あるため、stage 2 で `Deferred` の box を
足すと `RawNode` が 80 B になり、ADR §2.4 の 72 B の assert を満たせない（M1）。converged: no。

## 前ラウンドの解消状況
（round 1 のため無し）

## Findings

### M1（Major）: `CompactSpan` が 24 B で、ADR §1/§2.4 の配置と違い stage 2 の 72 B 予算を使い切っている
- 根拠: `crates/ooxml-core/src/xml/node.rs:6-18` は `open_end` とフラグを `u64` にまとめ、`start`/`close_start`/`close_end` の
  u32 と合わせて 20 B、整列で 24 B になる。プローブで `size_of::<RawNode>() == 72`（XName 8 + attrs 16 + children 24 + span 24）。
  ADR-0010 §1 の表（L170）は「four `u32` offsets, with the two dirty flags packed into the same word」で span を 16 B とし、
  §2.4（L413）は「`RawNode` keeps one `Option<Box<Deferred>>` … `const { assert!(size_of::<RawNode>() <= 72) }` … 72 B holds only
  with the two dirty flags packed into the span's word」としている。今の配置では stage 2 で 8 B 足すと 80 B になり、ADR が却下した
  「別の bool と同じ 80 B」に戻る。`node.rs:32` の assert（≤ 72）はこれを検出しない。
- 修正案: span を u32 4 個（16 B、align 4）にし、`open_end` の bit 31 を own_dirty、`close_end` の bit 31 を subtree_dirty に使う。
  Layer 2 の入力上限は `check_span_limit`（`parse.rs:81`）で 2 GiB − 1 に下げ（既定の `DEFAULT_MAX_PART_BYTES` 1 GiB より上）、
  `XmlPartTooLarge` の文言と ADR-0010 の post-acceptance note に上限の変更を書く。assert は `<= 64` に締め、stage 2 で 72 になるようにする。
  4 GiB を保ちたいなら §2.4 の 72 B 予算の方を owner 判断で改める必要があるので、その場合は Open questions に書いて止める。

### m1（Minor）: `XmlPartTooLarge` の判定が UTF-8・BOM・NUL の検査より前にあり、今日失敗する入力のエラーが変わる
- 根拠: `crates/ooxml-core/src/xml/parse.rs:81` の `check_span_limit` が `from_utf8`（L82）より前。4 GiB を超える非 UTF-8 の入力は
  main では `UnsupportedEncoding`、本 PR では `XmlPartTooLarge` になる。ADR-0010 Context は「The parse still fails, with the same
  error, for every input that fails today」。`Package` 経由では既定の 1 GiB 上限が先に効くので、`XmlDocument::parse` 直呼びか
  `with_max_part_bytes` で上限を上げた場合だけの差。
- 修正案: `check_span_limit` を NUL 検査の後、`Parser::new(...).run()` の直前に移す（`run` 以前の検査は u32 に依存しない）。

### m2（Minor）: main 側の数値を出した手順が PR から再現できない
- 根拠: `crates/ooxml-bench/benches/breakdown.rs:82,121` の `--stage1` は本 PR で追加したもので、main `b64559f` には無い。
  PR 本文の main 列はブランチ外で何らかの移植をして測ったはずだが、その手順が書かれていない。ADR の「same session on the same host」を
  第三者が追試できない。数値自体はコードと整合する（シートは子配列とスパン値の非確保でヒープ 1/5、異なる名前 262,144 件は上限
  16,384 を超えた分が毎回 SipHash の miss と `XName::new` になるので 1.28 倍は妥当）。
- 修正案: PR 本文に「`stage1()` を `b64559f` に cherry-pick して測った」など main 側の手順を 1 行書く。

### m3（Minor）: DTD を含む part が frozen との照合から丸ごと外れ、`DtdNotAllowed` の一致を見ていない
- 根拠: `crates/ooxml-core/tests/fuzz_regressions.rs:414` は `<!DOCTYPE` を含む fixture を照合しない（安全規則として妥当）。
  生成入力の不正 XML（L85）にも DOCTYPE が無いので、DTD 拒否のエラーと位置の一致は検査されていない。
- 修正案: L85 の配列に合成の `"<!DOCTYPE r><r/>"` と `"<r/><!DOCTYPE r>"` を足す（どちらも実体を定義しない自作入力なので安全規則に触れない）。

### n1（nit）: import の書き方
- 根拠: `crates/ooxml-core/src/xml/mod.rs:33` の `use crate::name::{XName};`（不要な波括弧）、`parse.rs:6` の
  `use super::interner::Interner;` が std の use の間にある。
- 修正案: `use crate::name::XName;` にし、`super::` の use を std の後にまとめる。

## 確認したこと（指摘なし）
- interner: 前段 256 スロット・`RandomState`・上限 16,384（`interner.rs`）。前段照合はキー全体、同一スロット衝突のテスト、
  262,144 件で表のキー比較回数 ≤ 入力件数のテストあり。owner 承認済みの受け入れ方法（専用の衝突入力なし）に合う。
- 32 ビット化: `SStr::span`・`RawText::parsed`・`CompactSpan` の `as u32` はすべて `check_span_limit` の後。編集値は逆転範囲の
  所有バッファで、長さに上限が無い。
- テキスト結合: `merged_text` は Text/GeneralRef 以外のイベントの前に必ず flush され、結合のスパンと値は main と一致（上記プローブ）。
- T1/局所性: 書き出しは span の位置だけを使い、`RawText` の WRITTEN 番兵と `forget_span` は main の `span: None` と同じ扱い。
  別文書への複製（`detach_node`）はテキストを written にし、markup は値（`SStr` の中身）で書くので元文書のスパンに依存しない。
- 公開名: 生成物に変更なし。公開 API の差は `Error::XmlPartTooLarge`（`Error` は `#[non_exhaustive]`）と `XName` の `Debug` 形のみで、
  PR 本文に記載あり。
- 他の open PR（#66, #72, #74, #76, #77）は `ooxml-core/src` に触れていない。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 3 / nit 1）
