#!/bin/sh
# Runs the suite once per mutant (or "rule as written in the ADR" switch) and
# prints the tests that fail. Every line must list at least one test.
# Usage: ./mutants.sh [feature-set ...]   (default: the list below)
# Set EJ_MODE=compile for the compiled-Commit mutants. CARGO overrides cargo.
set -u
cd "$(dirname "$0")"
CARGO=${CARGO:-cargo}
if [ $# -eq 0 ]; then
    set -- \
        "r1_unvalidated mutant_no_noop" mutant_no_revert mutant_no_defer mutant_no_gc \
        mutant_r3_subtract_changed "mutant_r3_subtract_changed mutant_half_cache_predicate" \
        mutant_r3_subtract_content "mutant_r3_subtract_content adr_r3_seed_raw" adr_r3_seed_raw \
        adr_r3_formula_only adr_missing_is_content adr_dead_mark_drops_fx \
        r1_unvalidated "r1_unvalidated mutant_r1_group_cells" mutant_r1_skip_group \
        mutant_no_handoff_target mutant_r2_no_derived \
        mutant_drop_marks mutant_drop_records "mutant_drop_records mutant_no_structural_clause" \
        mutant_close_own_sheet "mutant_close_own_sheet mutant_no_probe_clause" \
        mutant_restore_clears_mark mutant_no_sst_removed mutant_c7_created_keeps_s0 \
        mutant_tf_from_mark mutant_tf_no_restore mutant_no_rebase
fi
for f in "$@"; do
    fails=$($CARGO test --release --no-fail-fast --features "$f" 2>&1 | grep -E '^test .* FAILED$' | sed 's/^test //; s/ \.\.\. FAILED$//' | sort -u | tr '\n' ' ')
    echo "$f: ${fails:-NONE}"
done
