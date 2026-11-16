# Provenance: ECMA-376 Transitional XML Schemas

- Source: Ecma International, ECMA-376 Part 4, 5th edition (December 2016)
- Outer download: https://ecma-international.org/wp-content/uploads/ECMA-376-4_5th_edition_december_2016.zip
  - SHA-256: `bd25da1109f73762356596918bf5ff8b74a1331642dba5f1c1d1dfc6bed34ecd`
- Inner archive: `OfficeOpenXML-XMLSchema-Transitional.zip`
  - SHA-256: `d34187520749998af306faf1b730e568b0ca6d88ad24638a407c0a9bb4ca04fc`
- Retrieved: 2026-11-16
- Licence: Ecma default copyright notice, see `COPYRIGHT-NOTICE.txt` (ADR-0002 §1.2).

Every `.xsd` is vendored **unmodified**, CRLF line endings included
(`.gitattributes`: `reference/ecma-376-transitional-xsd/** -text`).
Corrections live only in `codegen/overrides.toml`. `cargo xtask codegen --check`
verifies the checksums below.

| File | SHA-256 |
| --- | --- |
| `dml-chart.xsd` | `4de4390a26e7e44e682ed98b39872cce2620f807f5b0de421567a43f74526388` |
| `dml-chartDrawing.xsd` | `3fd0586f2637b98bb9886f0e0b67d89e1cc987c2d158cc7deb5f5b9890ced412` |
| `dml-diagram.xsd` | `809f77f658f71e10f5cfbf99e25c895a23a4e62c81210b59a2fe8c7fb24849d2` |
| `dml-lockedCanvas.xsd` | `5cb76dabd8b97d1e9308a1700b90c20139be4d50792d21a7f09789f5cccd6026` |
| `dml-main.xsd` | `6978ba7e889070b0c3cb5b546b23e5a6c3516134afc53b87a21f482ca33f3858` |
| `dml-picture.xsd` | `5d389d42befbebd91945d620242347caecd3367f9a3a7cf8d97949507ae1f53c` |
| `dml-spreadsheetDrawing.xsd` | `b4532b6d258832953fbb3ee4c711f4fe25d3faf46a10644b2505f17010d01e88` |
| `dml-wordprocessingDrawing.xsd` | `2dbfd4719505bf39b568005994a433997cbeac59cebc9d4867d564c94e9ba03a` |
| `pml.xsd` | `39c6396b7b1f096885c3ceba5dab30e3ee14291c45a1d6fa6038ec4db69d88d0` |
| `shared-additionalCharacteristics.xsd` | `3c6709101c6aaa82888df5d8795c33f9e857196790eb320d9194e64be2b6bdd8` |
| `shared-bibliography.xsd` | `0b364451dc36a48dd6dae0f3b6ada05fd9b71e5208211f8ee5537d7e51a587e2` |
| `shared-commonSimpleTypes.xsd` | `48675c4f82f6b097434d4b7e313635b790257cca58fa90dd0e8f19c9affa18ed` |
| `shared-customXmlDataProperties.xsd` | `0ef4bb354ff44b923564c4ddbdda5987919d220225129ec94614a618ceafc281` |
| `shared-customXmlSchemaProperties.xsd` | `0d103b99a4a8652f8871552a69d42d2a3760ac6a5e3ef02d979c4273257ff6a4` |
| `shared-documentPropertiesCustom.xsd` | `9c085407751b9061c1f996f6c39ce58451be22a8d334f09175f0e89e42736285` |
| `shared-documentPropertiesExtended.xsd` | `bc92e36ccd233722d4c5869bec71ddc7b12e2df56059942cce5a39065cc9c368` |
| `shared-documentPropertiesVariantTypes.xsd` | `7b5b7413e2c895b1e148e82e292a117d53c7ec65b0696c992edca57b61b4a74b` |
| `shared-math.xsd` | `132e44dc10f2959e3f585584e5d82e7b2e329691e3010646e497710bd9774152` |
| `shared-relationshipReference.xsd` | `12264f3c03d738311cd9237d212f1c07479e70f0cbe1ae725d29b36539aef637` |
| `sml.xsd` | `495debc8fa967b77ed37799747b049832f2c95b2ecdb9f19ddcd68f8c9f96ab9` |
| `vml-main.xsd` | `335e9ecd40bb594ec4b916ac8c7b9eead4e16fe1b79f9d4105fdb1c43b66bb1c` |
| `vml-officeDrawing.xsd` | `585bedc1313b40888dcc544cb74cd939a105ee674f3b1d3aa1cc6d34f70ff155` |
| `vml-presentationDrawing.xsd` | `133c9f64a5c5d573b78d0a474122b22506d8eadb5e063f67cdbbb8fa2f161d0e` |
| `vml-spreadsheetDrawing.xsd` | `6bdeb169c3717eb01108853bd9fc5a3750fb1fa5b82abbdd854d49855a40f519` |
| `vml-wordprocessingDrawing.xsd` | `475dcae1e7d1ea46232db6f8481040c15e53a52a3c256831d3df204212b0e831` |
| `wml.xsd` | `0f7784b36ddc42950e5ff354cd608b83b60e4de39325d5568913f1225365029b` |
