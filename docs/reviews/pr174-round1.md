# PR #174 review round 1 (Opus via herdr-cocla)

- base: impl/0023-u7-binding 209b476, head: f49e30e
- verdict: NO (M1 atomicity + M2 index, m1/m2/n1-n3)
- job: 01a22036-6b66-79cf-9731-bdda0f9303c1

# PR #174 (U9 Python docx/pptx) review round 1 @ f49e30e

## Conclusion
converged: NO. There are two Major findings:
- **M1:** `replace_many` writes partially in refuse mode.
- **M2:** the index `duplicate_slide` returns is wrong, and the test pins the wrong value.

## Verification
Static review: I read the full diff, the relevant Rust APIs (docx `replace.rs`; pptx `slides.rs` and `presentation.rs`) and the tests. I did not build the wheel.

## Major
**M1: `replace_many` in refuse mode leaves earlier pairs applied when it raises.**

`bindings/python/src/document.rs` `replace_many`:
```rust
for (needle, replacement) in &pairs {
    let report = doc.replace(needle, replacement, &opts)?;
```
Under `OnRefused::Refuse`, Rust's `replace` is atomic only per call ("One refused match refuses the whole call: nothing is written", replace.rs:55). In the loop, when pair k is refused, pairs 1..k-1 have already been written and `?` raises `RefusedError`. That contradicts the docstring ("one refused match refuses the whole call (nothing written)") and the done-definition's `Err ⇒ byte-identical` (a raising call must leave the handle unchanged; ADR §11.2 "Refusals leave the Rust object unchanged"). The same happens in skip mode when a later pair fails with `EmptyNeedle` or `InvalidXmlChar`.

Repro:
```python
doc = R.Document.from_bytes(package(para("a {{x}}") + f"<w:p>{FIELD}</w:p>"))
before = doc.to_bytes()
with pytest.raises(R.RefusedError):
    doc.replace_many([("{{x}}", "X"), ("{{d}}", "Y")])
assert doc.to_bytes() == before   # fails: paragraph 0 is already "a X"
```
The existing `test_refuse_mode_raises_and_writes_nothing` uses a single `replace`, so it does not catch this.

Fix (`Document` is not Clone):
- (a) add a Rust-side `Document::replace_many` that plans every pair before the first write; ADR §10: "any rule it needs is added in Rust first";
- or (b) inside the same lock, keep `to_bytes()` before the loop and restore with `Document::from_bytes` on error.

Add the repro above as a test either way.

**M2: `duplicate_slide` returns the wrong index when `at` is omitted.**

`bindings/python/src/presentation.rs` `duplicate_slide`:
```rust
let before = deck.slides().count();
deck.duplicate_slide(index, at)?;
Ok(at.unwrap_or(before))
```
The Rust default is "right after it" (`slides.rs:734 let at = at.unwrap_or(index + 1)`), but the binding returns the end (`count`). The binding's docstring ("appended when absent") is wrong too.

Repro: in `test_slide_operations`, a 3-slide deck, `deck.duplicate_slide(0) == 3` passes, but the copy is at index 1. The test pins the wrong value, and a caller who edits or moves the slide at the returned index acts on a different slide.

Fix: `at.unwrap_or(index + 1)`, or better, return the position of the `SlideMut` Rust returns (one source of truth). Correct the test to `== 1` and check that `slide_texts()[1]` equals the original. `add_slide` (`at.unwrap_or(count)`) matches Rust's default, so it is fine.

## Minor
- **m1: `add_slide` uses two locks.** It calls `run` twice: one call takes `layouts()`, the next does `add_slide`. Another thread can work in between, which breaks one call = one lock. The current API has no way to change layouts, so this is not a real race today, but resolving the layout and adding the slide inside one closure is cleaner and also guards against U8-era changes. The `IndexError` would then be built from a value returned out of the closure.
- **m2: the binding kind `("binding","ReplaceAborted")` is not recorded anywhere.** ADR §11.3 lists binding kinds as `HandlePoisoned` and similar; this new one is not added to the ADR or a table. Add it to the U9 implementation note, alongside the final §12 signatures the ADR asks for ("the binding PRs record the final ones in an implementation note here"). Only `__kinds` covers the binding kinds, so the coverage test does not see it.

## nit
- **n1:** `replace` / `replace_many` return `Py<PyAny>` and switch type at runtime: int, or a tuple. The `.pyi` overloads with `Literal["refuse"]`/`Literal["skip"]` describe this correctly. But when `on_refused` is passed as a `str` variable, no overload matches; adding a fallback overload (`on_refused: str -> int | tuple[...]`) would help.
- **n2:** `reason` is the Debug output of `RefusalReason` (e.g. `"InsideField"`), which is what was asked for. It is not unit-only, though: some variants may carry fields, and then the dict gets values like `"Foo { .. }"`. Pin the reason strings for the possible variants (the test only checks InsideField), or write a mapping to stable names.
- **n3:** `slide_texts` collects shapes inside groups the way `slide.shapes()` does; whether that recurses into group shapes is not documented. If it does not, text inside groups is missed. State the join rule (shapes in document order, then table cells row-major, then `\n`) and the group handling in the ADR §12 note.

## Scope check (OK)
- **GIL:** every closure captures only owned values (`String`/`Vec`/`usize`/`opts`) and returns owned ones (the `Skipped` struct, `Vec<String>`, `ReplaceReport` → `usize`). No handle is borrowed.
- **Validation:** `on_refused` is validated before the lock (ValueError), and the test covers it.
- **pptx `replace_all`:** with `Abort`, Rust makes no change ("stop the whole call without any change"), then the binding raises `RefusedError("binding","ReplaceAborted")`; the test pins byte-identical output.
- **Slide errors:** out-of-range indexes for delete/move/duplicate go through the Rust `SlideIndexOutOfRange` to `InvalidInputError`, and the layout index raises `IndexError`.
- **Tests:** crafted docx packages and field/subtitle packages; refuse cases are checked byte-identical.

## Verdict
NO. Fix M1 (make refuse mode atomic across pairs, with the repro test) and M2 (the default return value, and correct the test).
