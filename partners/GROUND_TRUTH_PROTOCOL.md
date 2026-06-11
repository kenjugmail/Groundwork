# Ground-truth protocol (the M4 test)

Pre-registered before the trial starts, so success can't be redefined
afterward. Per the project spec: if the sensing layer doesn't beat the
status quo for one category in one place, we rethink fusion **before**
generalizing.

## Setup

- One partner org (food bank, pantry network, or 211 operator) in NYC or
  Westchester. One named contact.
- Duration: 4 consecutive weeks.
- Before week 1: snapshot the current nowcast + alert thresholds
  (`orchestrator export`), commit the hash. No weight changes during the
  trial (drift-gate quarantines excepted — those are logged).

## During each week

- Groundwork logs: every alert fired (timestamp, tract, type, evidence
  chain), plus a weekly top-10 widening-tracts list, delivered by the feed.
- Partner logs (their normal process, no extra burden): notable demand
  shifts they observed — where, when noticed, how they learned of it.

## At the end: three questions

1. **Earlier?** For each demand shift the partner observed, did Groundwork
   alert on the corresponding area *before* the partner knew by their own
   channels? Count weeks/days of lead time.
2. **Truer?** Of Groundwork's alerts, how many does the partner judge real
   (need actually moved) vs noise? Precision matters as much as lead time —
   an alert feed nobody trusts is worse than none.
3. **Actionable?** Did any alert change a decision (stocking, staffing,
   outreach, grant language)? One genuine yes is the bar.

## Success / failure

- **Success:** at least one verified earlier-or-truer surfacing AND
  precision the partner calls usable. → Write up, keep the partner, pursue
  the 211 DSA, consider a second category/metro.
- **Failure:** nothing earlier, or precision too low to trust. → Publish
  the negative result in the repo (the honesty layer applies to us too),
  revisit fusion weights/sources, re-run before any generalization.

## Notes

- The partner sees this document before agreeing. Their time cost should be
  ≤ 30 minutes/week.
- All shared partner observations stay private unless they approve
  publication.
