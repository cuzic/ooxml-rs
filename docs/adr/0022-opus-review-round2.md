# ADR-0022 Claude Opus review round 2

Job `01a20a46-ef99-75ee-9c28-f11844bccbd6`. Opus reviewed the supplied revision 2 text only.

| ID | Severity | Finding | Revision 3 disposition |
| --- | --- | --- | --- |
| R2-1 | Major | The cutoff could remove trailing features. | Freeze the existing contract and corpus for this spike. Probe the pinned reader before promising any post-cutoff metadata. |
| R2-2 | Major | The mapper could narrow accepted keys or reorder errors. | Generate the complete current allowlist at the original validation point. |
| R2-3 | Minor | Wasm trap recovery is unspecified. | Evaluate only recoverable decode failures. Trap recovery remains a production design question. |
| R2-4 | Minor | Unselected sheets might incur work. | Keep archive projection inside the selection branch and test projections and bookSheets. |
| R2-5 | Minor | The generator has no code-reduction gate. | Require cellFormula to change only declarations and fixtures, then count authored mapper lines. |
