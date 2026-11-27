# ADR-0021 敵対的レビュー Round 1

- レビュー担当は独立した Codex サブエージェント。
- 対象は Proposed rev 1、ベース `f88d02c`。未コミット草案の SHA-256 は `53297aeeba6d59abfe73491acee5f81b7b9528225f96f36a84a4d4528446739c`。
- 判定は **要修正・再レビュー**。公開 API と既存 Rust コードの読解に基づく。互換バインディングの実行検証は未実施。

## Blocker

### B1 ExcelJS の setter 失敗後に保存不能になる

ADR L67・88 は各 setter の失敗後にも直前と同一のバイト列を保存できるとする。通常の事前拒否はこの契約に合う。一方、Rust の適用段階で内部エラーが起きるとワークブックが poison され、`to_bytes()` 自体がエラーになる。例えば先に A1 を正常に編集し、続く文字列編集が共有文字列の更新後に失敗すると、その後の `writeBuffer()` で A1 の編集済みファイルを取得できない。

公開契約がすべての setter 失敗を対象とするなら、直前の確定状態を保ち、隔離した状態に適用して成功後に差し替える経路を定義する。通常の拒否と復旧不能な内部障害を別契約にする案もある。どちらの場合も先行する成功編集と複数の apply 失敗箇所を含む注入テストを指定する。[Rust の apply と poison](../../crates/ooxml-xlsx/src/edit.rs#L833) · [保存時のチェック](../../crates/ooxml-xlsx/src/workbook.rs#L481)

## Major

### M1 SheetJS の `!ref` が保存結果と一致する条件が未定義

ADR L38・68。初期値を元の `<dimension>` から取るかセルから求めるか、範囲の欠落や範囲外セルをどう扱うかが未定義。例えば `!ref` を A1 から A1:C3 に広げつつ B2 だけ追加する場合、現在の Rust のセル編集では範囲は A1:B2 までしか広がらず、元の dimension が欠落している場合は追加されない。上流では自己申告範囲がデータ利用に影響する。

元の範囲・未投影セル・範囲変更と再読込後の契約を明記する。空白の余白を含む範囲を保存できない場合は保存前に拒否し、元 XML の範囲欠落・過小・余白を fixture に含める。[Rust の範囲処理](../../crates/ooxml-xlsx/src/worksheet.rs#L306) · [SheetJS の範囲仕様](https://docs.sheetjs.com/docs/csf/sheet/#worksheet-range)

### M2 未投影セルをどう見せ、どう編集から守るかが未定義

ADR L63・68。疎なプレーンオブジェクトで、未対応セルの直接アクセスは例外、実際の欠損は `undefined`、列挙は可能という組み合わせには、アクセサーや Proxy 等の具体的な表現が要る。また `CellRef::value()` 単体では、共有文字列のリッチテキストや数式キャッシュが単純値に見える。配列式の従属セルはその位置に独自の `<f>` がない場合がある。走査の失敗が座標単位で示せない場合もある。

読み出しの公開契約と編集可否の分類を分けて設計する。判定できない対象は保守的に編集を拒否し、数式・結合・日付書式・リッチテキストの検出と無変更保存を fixture で確かめる。[Rust の値の射影](../../crates/ooxml-xlsx/src/worksheet.rs#L113) · [セルの走査](../../crates/ooxml-xlsx/src/worksheet.rs#L526)

### M3 ExcelJS のオブジェクト同一性が未定義

ADR L39・67・86–87。上流は同一シート内の同じ座標に対してセルを再利用し、`worksheets` の新しい配列には同じワークシートが入る。毎回ハンドルを作る実装は、`ws.getCell('A1') === ws.getCell('A1')` やシートをキーとする Map の既存コードを壊す。

同一ロード世代内のハンドル同一性をテストするか、明示的な差として扱う。再ロード後の古いハンドルは上流の挙動を測定し、差を設ける場合は記録する。[Worksheet](https://github.com/exceljs/exceljs/blob/v4.4.0/lib/doc/worksheet.js#L557-L562) · [Row](https://github.com/exceljs/exceljs/blob/v4.4.0/lib/doc/row.js#L38-L49) · [Workbook](https://github.com/exceljs/exceljs/blob/v4.4.0/lib/doc/workbook.js#L100-L117)

### M4 import-only の対象と配布物の検証が弱い

ADR L30・34・61・86–96。SheetJS は既定で `cellText: true` のため、通常の数値セルでも既存コードが `cell.w` を使う場合がある。また開発用のローカル import が成功しても、配布物の ESM エントリが Wasm を解決できるとは限らない。

実際の対象呼び出し、import 形式、オプションと戻り型を固定する。パッケージ化した成果物をブラウザー側の別プロジェクトにインストールし、import 元だけを差し替えたテストで Wasm 読み込みと同期性を確認する。対応ブラウザー・バンドラーと初期化失敗の挙動も定義する。[SheetJS 読み込みオプション](https://docs.sheetjs.com/docs/api/parse-options/)

### M5 JS と Wasm の間の文字列・数値の境界が未定義

ADR L34・38–39・63・69。例えば孤立した UTF-16 サロゲートを JS 文字列として受け付けると、UTF-8 変換経路で置換文字になる可能性がある。実装後のバインディングで要確認。

入力の型・範囲と正規化の契約を決め、変換できない入力はトランザクション前に拒否する。空文字、補助文字、孤立サロゲート、NaN、符号付きゼロを受け入れテストに含める。

## Minor

### m1 互換対象の名称と配布ブランドの方針を明記する

ADR L26・30・95。上流の呼び出し名、互換対象の表示、配布ブランドを区別する。型定義・文書・テストの独自作成、必要な上流ライセンス表示、検証した対象バージョンの記載を ADR に残す。[SheetJS CE ライセンス](https://docs.sheetjs.com/docs/miscellany/license/) · [ExcelJS ライセンス](https://github.com/exceljs/exceljs/blob/v4.4.0/LICENSE)
