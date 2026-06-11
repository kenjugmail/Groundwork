# Prompt prefix (M2)

This directory will contain the stable, cache-friendly prompt prefix:

- `system.md` — extraction rules (no inference beyond the text; provenance
  mandatory; empty output valid)
- `exemplars/` — few-shot examples, one file per case: positive (pantry hours
  cut), negative (news that is NOT a signal), edge (ambiguous geography)

Ordering contract: everything in this directory is byte-stable across
documents within a deployment; the document body is always the final, fresh
segment of the prompt. Any edit to this prefix invalidates the cache for the
whole fleet — edits are PRs, reviewed like weight changes.
