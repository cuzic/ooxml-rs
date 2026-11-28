# PR #174 review round 2 (Opus via herdr-cocla)

- head: fbc456f
- verdict: YES (m3/n4 fixed alongside)
- job: 01a2203b-8fa8-74cf-93fc-fa1d3aa2fde8

# PR #174 review round 2 @ fbc456f (f49e30e..fbc456f)

## Conclusion
converged: YES. M1, M2, m1, m2 (recorded in the PR body, since the ADR is still Proposed) and n1–n3 are all fixed. There is one new Minor (restoring from bytes drops non-byte state; latent today) and one nit.

## Verification
Static review: I read the full diff, `SlideInfo`, `RefusalReason::kind` and the `Document` fields. I did not build the wheel.

## Status of round-1 findings
- **M1, FIXED.**
  - When `pairs.len() > 1`, the binding takes `doc.to_bytes()?` inside the lock before the first pair. If a pair fails, it restores `*doc = Document::from_bytes(snapshot)` and then returns `Err`.
  - A single pair skips the snapshot; Rust `replace` is atomic per call, so the old behaviour is unchanged.
  - If the snapshot fails to reopen, `expect` panics, the mutex is poisoned and the handle dead-ends as HandlePoisoned. That is loud and intentional.
  - New tests: `test_many_is_atomic_when_a_later_pair_is_refused` (the round-1 repro) and `..._empty_in_skip_mode` (a later EmptyNeedle → `InvalidInputError`); both pin `to_bytes() == before`.
- **M2, FIXED.** `duplicate_slide` returns `deck.duplicate_slide(index, at)?.info().index`. `SlideInfo.index` is documented as "Position in `p:sldIdLst` (0-based)" (read.rs:21), so Rust is the one source of truth for both the default (`index + 1`) and an explicit `at`. The test now expects `duplicate_slide(0) == 1` and checks `slide_texts()[1] == first`, and the docstring says "right after it". `add_slide` also returns `info().index`.
- **m1, FIXED.** `add_slide` resolves the layout and adds the slide in one `run`. An out-of-range layout returns `Ok(Err((asked, count)))` out of the closure, and `IndexError` is built after the lock is released; the `match="layout index"` test still holds. No PyErr is built while the GIL is released.
- **m2, FIXED.** I accept recording `("binding","ReplaceAborted")` and the final signatures in the PR body while the ADR is Proposed. They should go into ADR §12 as a post-acceptance note once it is Accepted.
- **n1, FIXED.** The overload order is Literal["refuse"] (default) → Literal["skip"] → `str` (`int | tuple[...]`). Specific overloads come first, and the fallback's return type is a supertype of the earlier ones, so mypy does not flag them as overlapping. I did not run mypy.
- **n2, FIXED.** `reason` is now `RefusalReason::kind()`, a stable string from a match with no wildcard arm (InsideField, InsideRevision, UnsupportedContext, …). Variants with fields no longer leak Debug output.
- **n3, FIXED.** The `slide_texts` docstring now says: group members included, depth-first (the order `Slide::shapes` gives), table cells row-major after their shape, joined with `\n`.

## New findings
**m3 (Minor, latent): restoring from bytes resets the handle's non-byte state.** `ooxml_docx::Document` holds state that is not in its bytes: `fields_opt_in`, `gates`, `update_fields`, `annotation_high_water`/`note_high_water`, `generations`/`id`. The restore with `Document::from_bytes(snapshot)` resets all of these to what reopening the file would give. Through the binding's current API (`paragraph_texts`, `append_paragraph`, `replace*`) none of them can be changed, so it is harmless today. But once a later binding PR exposes, say, a field-edit opt-in, a failed `replace_many` will silently drop the caller's opt-in. The high-water marks only matter after an in-session deletion, which is not exposed either.
- Fix: one comment at the restore site ("valid only while the binding exposes no non-byte state"), or longer term the Rust-side `Document::replace_many` (plan everything before the first write) from round 1's option (a). Recording it in the U9 implementation note is enough.

**n4 (nit): the snapshot serializes the whole document on every multi-pair call, even on success.** This is correct; just know the cost for large documents. It is worth stating in the docstring, or skipping when every pair is a no-op.

## Verdict
YES. m3 and n4 can be handled with a comment or note, accepted on CI.
