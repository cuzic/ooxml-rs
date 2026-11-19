# Provenance for ECMA-376 DrawingML preset geometries

- Source is Ecma International, ECMA-376 Part 1, 5th edition (December 2016).
- [Outer archive](https://ecma-international.org/wp-content/uploads/ECMA-376-1_5th_edition_december_2016.zip) SHA-256 is `9d0bcad9cf06054785b03762fcfadbf6bab7e54a5f9d69434e34b7fd464d4129`.
- Inner archive `OfficeOpenXML-DrawingMLGeometries.zip` SHA-256 is `a8517838a3a60492d267d1ff92e1ebd831adc04584de79a38a5532a68902abf6`.
- The XML is vendored byte for byte. The Ecma copyright notice is in `COPYRIGHT-NOTICE.txt`.

| File | SHA-256 |
| --- | --- |
| `presetShapeDefinitions.xml` | `2f7c868d857c1e3c4b5a6068759fe0e07d77ad58377a6618d1b02ba3507b6939` |

The XML contains two identical `upDownArrow` definitions and omits
`upArrow`. The generator accepts identical duplicates and rejects conflicting
ones. The handwritten `supplement.toml` supplies the adjustment defaults
for `upArrow`, checked against `downArrow` in the same XML, and the
resulting name set is checked against the XSD.
