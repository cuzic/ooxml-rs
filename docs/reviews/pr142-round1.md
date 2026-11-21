# PR #142 レビュー（Opus, round 1）

対象: `fix/docx-d3e-followups` @ b7ede127556651dc61636e7590b5839178a551c6

## 結論（先に）
m4 と m5 はどちらも挙動で解消を確認した。SEQ の識別子は Unicode の文字で始まる規則に変わり、`図` と `表` を受理する。不正な識別子は byte-identical のまま拒否される。LibreOffice のケースは、空のキャッシュと `w:dirty` だけ（`updateFields` なし）の DOCPROPERTY CreateTime を実際に通している。
壊れたファイルを書く入力、黙って誤る入力は見つからなかった。残りは、Word が文書に書いている 40 文字の上限を検査していない Minor 1 件と nit 3 件。
converged: yes。CI は 5 ジョブとも緑。

## 前ラウンドの解消状況（PR #137 round 2）
- m4: 解消。`insert_field.rs:302-310` は、先頭を `char::is_alphabetic`、2 文字目以降を `is_alphanumeric` か `_` で検査する。テストは `図`、`表`、`図表_２` を受理し、`1図`、`２表`、`図 表`、`図-表` を拒否する。拒否は `assert_refusal` で byte-identical を確認している。ASCII だけの検査に戻すと、`図` の `unwrap_or_else(panic)` が落ちる。既存の `SEQ 図 \* ARABIC` を数えたキャッシュが `2` になることも確かめている。
- m5: 解消。`53-field-docproperty-createtime.docx` は opt-in なし、settings なしで作られる（`synthetic_with_extra_parts(&body, None, …)`）。`expected.json` は、begin の `w:dirty='true'` と、`w:t` がラベルの 1 個だけであること（キャッシュが空）を確かめる。CI のログでも LO_VERSION 25.2.7.2、LANG C.UTF-8、TZ UTC だった。ADR の記述は合っている。

## プローブ（識別子の検査と同じ式を単体で実行）
| 入力 | 結果 | 評価 |
| --- | --- | --- |
| `Ｆｉｇ`、`Ｆｉｇ１`（全角） | 受理 | 妥当（Lu/Ll と Nd） |
| `１Fig`、`٣Fig` | 拒否 | 妥当（先頭が数字） |
| `Fig٣`、`Fig௧` | 受理 | 妥当（Nd） |
| `_Fig` | 拒否 | 妥当 |
| `e` + U+0301（分解形） | 拒否 | 安全側。NFC の `é` は受理 |
| `कि`（Mc 結合文字） | 受理 | 妥当（Other_Alphabetic） |
| `😀`、`Fig😀`、`©`、`Fig™` | 拒否 | 妥当 |
| `図Fig`、`ال`、`𠮷`、`々`、`ー`、`ǅ`、`ª` | 受理 | 妥当 |
| `Fig②`、`Fig½`、`Fig²`、`Ⅻ` | 受理 | n1 を参照 |
| `Fig`+U+200D、`Fig`+U+3000、`Fig＿` | 拒否 | 安全側 |
| `A` を 41 個 | **受理** | m1 を参照 |

受理される文字には、制御文字も、XML で書けない文字も、`"`、`\`、空白もない。識別子は `" SEQ "<id>" "` のように引用符で囲まれるので、フィールドの構文は壊れない。

## Findings

### m1（Minor）: SEQ の識別子に 40 文字の上限がない
- 根拠: Microsoft の SEQ フィールドの文書は、識別子を「文字で始まり、40 文字まで（文字、数字、`_`）」としている。同じ規則のブックマーク名は `bookmarks.rs:62` で `name.chars().count() > 40` を拒否するが、SEQ の分岐には上限がない。プローブでは 41 文字の識別子を受理した。41 文字以上を Word がどう扱うかは *(u)*（切り詰める、エラーを表示する、受理する、のどれか）。壊れたファイルは書かないが、規則が不確かなところで fail-closed になっていない。上限がないのは ASCII のときからだが、今回の PR がこの規則を書き直している。
- 修正案: `identifier.chars().count() > 40` を `InvalidValue` にし、テストの拒否側に 41 文字の例と、受理側に 40 文字の例を加える。ADR の D3e note にも 40 文字と書く。

### n1（nit）: 2 文字目以降に No（`②`、`½`、`²`）と Nl（`Ⅻ`）を許す
- `is_alphanumeric` は Nd だけでなく No と Nl も含む。Word の「numbers」がこれらを含むかは *(u)*。引用符の中なので構文上の危険はなく、ブックマーク名の規則（ADR-0011 Q3）とも同じなので、そろえたままでよい。Word で確かめるときに 1 件加えるとよい。

### n2（nit）: キャッシュを数えるときの正規化が Unicode の `to_uppercase`
- `fields.rs:223` の `normalize_name` は `ß` を `SS`、`ﬁ` を `FI` にする。そのため `Maß` と `MASS` を同じ系列として数える。Word の大文字・小文字の扱いは ASCII 以外では *(u)*。SEQ は dirty で、`updateFields` の opt-in も必須なので、Word が更新すれば消える差。影響はキャッシュだけ。

### n3（nit）: open question が owner 向けの一覧に載っていない
- 新しい「Open question: date-valued DOCPROPERTY refresh」は §7 の中にあり、`## Open questions for the project owner`（1〜10）にはない。また、すぐ上の Freshness の項は「`w:dirty` alone is not relied on」と書いたままで、日付型 DOCPROPERTY がその例外であることは §7 の open question と D3e note にしかない。
- 修正案: 一覧に 11 番として 1 行で参照を加え、Freshness の項から例外へ参照を張る。

## 他の open PR との干渉
`insert_field.rs`、`phase3e.rs`、ADR-0015、`tools/lo-check/expected.json`、`manual_check.rs` に触る open PR は #142 だけ。`CORE` に `dcterms:created` を足したので、ほかの DOCPROPERTY のケースも入力が変わるが、CI の libreoffice ジョブは緑。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 3）
