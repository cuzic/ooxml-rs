# PR #85 レビュー（Opus, round 1）

対象: impl/docx-phase2-part3 @ 860eeb6fdb50e68dcb77701a5ffea513ee1c88ce

## 結論（先に）
converged: no。Major が 1 件ある。`restart_list` は、再開始するレベルの既存 `w:lvlOverride` を捨てる。
その override が `w:lvl`（書式の上書き）を持つと、`Ok` のまま番号の見た目が変わる。
それ以外の ADR 規則はプローブで守られていた。確認した規則は、注釈 id を再利用しないこと、
`numId` 0 を採番しないこと、`numIdMacAtCleanup` の前への挿入、nsid の新規性、
拒否を書き込み前にまとめること、style id の規則（ぶら下がり参照・大小無視を含む）、`BuiltInStyleName`、
based_on/next の種類検査、`Err ⇒ byte-identical`。生成型の公開名は main から削除も改名もない
（wml +285、dml +5、追加のみ）。

## 前ラウンドの解消状況
round 1 のため無し。

## プローブの要約（コミットしない）
- 書き込める fixture すべてで create/apply/restart/remove_from_list、add/remove_bookmark、add_style を順に実行した。
  拒否はすべて ZIP 全体がバイト一致した。出力の `num/@numId`、`abstractNum/@abstractNumId`、`style/@styleId`、`nsid` に重複は無かった。
- 既定スタイル（`w:default="1"`）の numPr だけで番号が付く段落に `remove_from_list` → `<w:numId w:val="0"/>`。正しい。
- `restart_list` で、後続段落の 1 つが `w:sdt` 内 → `UnsupportedContext` になり、出力はバイト一致。正しい。
- 削除後に `add_bookmark` → id はハイウォーターから採番され、再利用しない。comments.xml の `w:comment/@w:id` も採番の範囲に入る。
- 小文字の `ref Mark` と、分割された instrText の `PAGEREF` → `NameReferencedByField` / `BookmarkReferenced`。
- style: `My-Style` の stem `MyStyle` が既存で、`mystyle0` がぶら下がり参照 → `MyStyle1`。`HEADING1` の明示 id → `StyleIdInUse`。

## Findings

### M1（Major）: `restart_list` が再開始レベルの `w:lvlOverride/w:lvl` を落とし、番号書式が黙って変わる
- 根拠: `crates/ooxml-docx/src/numbering.rs:241-255`。他レベルの override だけをコピーし（`!= Some(level)`）、
  再開始レベルの override を新しく作る。その中身は `startOverride` だけになる。
  プローブ: `<w:num w:numId="4"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:lvl w:ilvl="0"><w:numFmt w:val="upperRoman"/></w:lvl></w:lvlOverride></w:num>`
  の段落 1 に `restart_list(p(1), 3)` を実行すると、新しい num は
  `<w:lvlOverride w:ilvl="0"><w:startOverride w:val="3"/></w:lvlOverride>` だけになる。
  再開始した段落は upperRoman から abstractNum の書式に変わる。`Ok` での黙った誤りになる。
  ADR §5a がコピーする理由は "so their look does not change" であり、同じレベルにもこの理由が当てはまる。
- 修正案: 再開始レベルの既存 `lvlOverride` も `detach_as` でコピーし、その中の `w:startOverride` を
  `get_or_insert` で値に置き換える（`w:lvl` は残す）。または、その override が `w:lvl` を持つときは書き込み前に
  `InvalidNumbering` で拒否する。どちらの場合も、`w:lvl` 付きの override の回帰テストを足す。

### m1（Minor）: run の分割で `w:noBreakHyphen`、`w:softHyphen`、`w:cr` が文字や `w:br` に変わり、`w:lastRenderedPageBreak` が消える
- 根拠: `crates/ooxml-docx/src/bookmarks.rs:307-330`。前半の run を、読み取りモデルのテキストから `write_pieces` で書き直している。
  プローブ: `<w:t>ab</w:t><w:noBreakHyphen/><w:t>cd</w:t>` の "d" を範囲に `add_bookmark` すると、前半は `<w:t>ab‑c</w:t>`（U+2011）になる。
  `softHyphen` は U+00AD の文字になる。`<w:cr/>` は `<w:br/>` になる。
  段落テキストは変わらないが、ADR §10 は split_run について "the text and its formatting are unchanged" と書いている。
  要素の書き換えは、この記述より広い。
- 修正案: 分割は子要素の単位で行う。分割点を含む `w:t` だけを 2 つに分け、それ以外の子要素は前半と後半へそのまま振り分ける。
  当面の代替としては、分割する run にこれらの子要素があれば `RunNotTextOnly` で拒否する。

### m2（Minor）: ADR §12 の T-REG の fixture スイープがリポジトリに無く、python-docx オラクルも CI で実行されない
- 根拠: `crates/ooxml-docx/tests/phase2_registries.rs` は手書きの入力だけを使う。
  画像（`tests/images.rs` の `fixture_sweep`）と違い、writable fixture で採番の範囲を検査し直すテストが無い。
  `tests/oracle/phase2_registries.py` は `.github/workflows/ci.yml` から呼ばれない。
  レビュー時に同等のスイープを手で回し、問題は出なかった（挙動は正しい）。
- 修正案: `phase2_registries.rs` に `fixture_sweep` を足す。各 writable fixture に create/apply/restart/remove_from_list、
  add/remove_bookmark、add_style を実行し、次を検査する: 拒否時のバイト一致、再オープン後のテキスト一致、
  numId・abstractNumId・nsid・styleId・注釈 id の一意性、変更されたパートの集合。
  python-docx オラクルは CI の手順に足すか、手動確認の手順だと ADR の注記に書く。

### m3（Minor）: ブックマーク操作が、フィールドや `pPrChange` を含む段落を一律に拒否する（安全側の過剰拒否）
- 根拠: `crates/ooxml-docx/src/bookmarks.rs:110-111` が `check_structure_neighbour` を使っている。
  そのため、`PAGE` フィールドを 1 つ含むだけの段落も `InsideField` になる。fixture スイープでは bm/bm2 の拒否 6 件のうち 3 件がこれだった。
  `match_boundaries`（`bookmarks.rs:256-261`）も、範囲内にあるだけの run に Direct 文脈と `check_settable` を要求する。
  範囲の途中に `w:hyperlink` があると拒否される。
- 修正案: 境界の run と、マーカーを置く段落だけを検査する。ADR §5 はフィールドを含む段落を禁止していない。
  今回直さないなら、拒否する理由を API の文書に書く。

### m4（Minor）: `remove_bookmark` と名前の重複検査が、別文書である glossary を含めている
- 根拠: `bookmarks.rs:155` と `bookmarks.rs:208` の `registry_parts(&[])` は glossary document を含む。
  本文に無く glossary にだけある名前は、`remove_bookmark` が glossary から消す。
  glossary と本文に同名があると、`DuplicateBookmarkName` で拒否される。
  ADR §2 が glossary を含めると書いているのは、注釈 id の範囲（"harmless"）だけである。
- 修正案: 名前の数え上げと削除は story パートだけを対象にする。id の最大値の走査には glossary を残す。

### m5（Minor）: 文書の不一致
- `codegen/allowlist/wml.toml` の `[detach]` に `w:CT_RPr/w:rPr` が追加されているが、ADR-0011 §11 と Part 3 の注記に記録が無い。
  ADR §11 は "Each review checks that the subtree holds no document-unique id" と定めている。
  rPr の中の `rPrChange` は拒否するので、id は残らない。この旨を注記に書く。
- `add_bookmark` の文書に、分割で以後の run 番号が 1 つずれることが書かれていない（ADR §9.2: "The index API documents it"）。

### m6（Minor）: style id の範囲に settings.xml の参照が無い
- 根拠: `crates/ooxml-docx/src/styles.rs:363` は styles、stylesWithEffects、numbering、stories だけを走査する。
  `settings.xml` の `w:clickAndTypeStyle`、`w:defaultTableStyle` も style id を参照する。
  これらがぶら下がっていると、新しいスタイルが参照を取り込む（ADR §2 の M3 と同じ理由）。
  これは ADR の範囲表の漏れなので Minor とする。
- 修正案: 走査に settings パートの `clickAndTypeStyle`/`defaultTableStyle` の `w:val` を足し、ADR の注記に書く。

### n1（nit）: 明示の style id に空白を含む値（`"has space"`）がそのまま通る
- `styles.rs:374`。`ST_String` としては有効だが、Word は空白を含む id を作らない。`[A-Za-z0-9]` 以外を拒否するか、文書に書く。

### n2（nit）: `based_on` が無いスタイルの段落書式を、既定段落スタイルの継承値に対して計画している
- `styles.rs:391`。`inherited_ppr_attr(None, …)` は既定段落スタイルにフォールバックするが、basedOn の無いスタイルは docDefaults だけを継承する。
  `based_on` が `None` のときは、docDefaults だけを見るようにする。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 6 / nit 2）
