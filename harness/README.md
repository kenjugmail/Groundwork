# Agentic extraction harness (M2 — stub)

This directory will hold Groundwork's open, ground-up agentic harness: the
component that turns unstructured documents (local news, prose notices, PDFs)
into typed `Signal`s — e.g. "St. Anne's pantry in Mount Vernon cut hours to
two days a week" → `Signal{type: pantry_capacity, direction: -, geo_unit:
<tract>, confidence, provenance_url}`.

Built from scratch in the open. It reuses *patterns* from closed prior work —
prefix-caching discipline, record/replay, drift gates — but none of the code.

## Design rules (binding for M2)

1. **Prefix-caching discipline.** The system prompt, signal schema, type enum,
   and few-shot exemplars in `prompt/` are stable across every document; only
   the document body varies. The whole harness prefix must be cacheable —
   the difference between an affordable always-on civic pipeline and an
   expensive toy.
2. **Schema-constrained output.** The agent emits `Signal[]` against
   `schema/signal_extraction.json`; malformed output is rejected and re-asked,
   never parsed from prose. An empty array is valid and common.
3. **Provenance mandatory.** Every signal MUST carry `raw_excerpt` (the
   justifying span, verbatim from the document) + `provenance_url`. A signal
   not groundable in a quotable span is hallucination and is dropped.
4. **No inference beyond the text.** "Plant closing, 400 jobs" → a `layoff`
   signal. The agent does NOT decide that means food insecurity rises — that
   leap belongs to the fusion layer, explicit and auditable, never hidden
   inside the LLM.
5. **Confidence, not certainty.** Low-confidence extractions survive,
   down-weighted and flagged for review. Discarding them is its own bias.
6. **Model-agnostic.** Implemented against a provider-neutral interface so
   contributors can swap in a local OSS model.

## Layout

- `prompt/` — stable, cache-friendly prefix: system prompt + exemplars
- `schema/` — JSON Schema the agent's output is validated against
