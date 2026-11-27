# ADR-0022 Claude Opus review round 1

Job `01a20a45-dc79-78ff-8832-11a8d516a037`. Opus reviewed the supplied revision 1 text only.

| ID | Severity | Finding | Revision 2 disposition |
| --- | --- | --- | --- |
| R1-1 | Major | The spike cannot reject duplicate scanning and omits dense input. | Define workload sizes, timing and memory thresholds, and count boundary copies. |
| R1-2 | Major | Both runtimes implement the cutoff. | Rust determines the prefix once. JS only receives the selected records. |
| R1-3 | Major | Descriptor policy and error order are underspecified. | Keep the existing shared descriptor policy and surrounding input checks. Test invalid descriptors locally. |
| R1-4 | Minor | Wire layout and failure mapping are unspecified. | Define row and column array strides, ownership and errors. |
| R1-5 | Minor | DSL and format independence lack falsifiable checks. | Choose `cellFormula` up front and narrow the projection evidence to XLSB. |
