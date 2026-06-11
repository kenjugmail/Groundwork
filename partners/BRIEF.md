# Groundwork — partner brief (one page)

**For:** the NYC / Westchester 2-1-1 operator and food-security organizations
**From:** the Groundwork project (open source, Apache-2.0 / CC-BY-4.0)

## What this is

Groundwork is an open **needs-transparency layer** for food insecurity in NYC
+ Westchester. It fuses public real-time signals — WARN layoff filings, SNAP
enrollment changes, federal food-hardship surveys, local news — into a
census-tract map of where the gap appears to be **widening**, updated
continuously, with every number clickable down to its source document.

It is **not** an allocator (no money flows through it), not impact-proof, and
not a substitute for your local knowledge — see WHAT_THIS_IS_NOT.md in the
repo. It is a faster, evidence-linked early-warning picture than annual data
allows.

## What we're asking

A conversation about a **data-sharing agreement** for aggregated 2-1-1
food-assistance request counts (e.g. nightly, by ZIP). 211 data is the
proven leading indicator in this space — it surfaced the 2008 foreclosure
crisis, Flint, and the 2022 formula shortage **months before** other data.
NNIP publishes a sample DSA we can start from. Aggregates only; no caller
records; you control granularity and embargo.

## What you get back

- A free, continuously updated dashboard + alert feed (Atom / webhook) for
  your service area, with your data fused against signals you don't hold
  (layoffs, news, federal surveys).
- "We may be blind here" alerts — tracts with high baseline need where all
  sensing has gone quiet.
- CC-BY data exports for your own grant reporting and advocacy (sample
  exports in this folder).
- Attribution on every output, and veto over how your data is displayed.

## The honesty layer (why this is safe to join)

Every signal carries a provenance URL and verbatim excerpt; methodology and
all model weights are versioned in public; coverage gaps are displayed, never
hidden; gameable sources are explicitly down-weighted. The system is built so
that being wrong is visible.

**Repo:** github.com/kenjugmail/Groundwork · **Demo:** see DEMO_SCRIPT.md
