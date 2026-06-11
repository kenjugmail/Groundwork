# Demo script (10 minutes)

Audience: 211 operator staff or a food-security org. Run on the public URL
(or `cargo run -p api` locally). Goal: show the transparency loop, not wow
with features.

1. **The map (1 min).** Open the app. "Each dot is a census tract. Color =
   how far the food-insecurity nowcast sits above its annual baseline. Faded
   dots mean low *coverage* — we show when we can't see, we never let silence
   look like low need."

2. **Click an elevated tract (3 min).** Pick one with signals. Walk the
   panel top to bottom: nowcast vs baseline, uncertainty, and then the
   decomposition — "every point of elevation is attributed: this much from a
   WARN layoff filing, this much from SNAP enrollment movement. Click any
   line —" (click) "— and you get the verbatim excerpt and a link to the
   government filing or article it came from. If we can't show why, we don't
   flag it."

3. **The alert feed (2 min).** Open `/v1/alerts.atom` in a feed reader.
   "You don't have to watch the map. Subscribe and you get two kinds of
   pings: a tract is widening fast, or we've gone blind somewhere that's
   historically high-need. It's a feed, not a platform — it works in
   whatever you already use."

4. **What's missing (2 min — the ask).** Point at the coverage score.
   "The biggest hole is the thing only you have: real-time request volume.
   211 calls are the proven leading indicator — our other signals lag them.
   With nightly ZIP-level aggregates, this map gets weeks of early warning
   it can't have today, and you get the fused picture back."

5. **The ground-truth offer (2 min).** "We don't want you to trust this —
   we want to test it. Four weeks: you keep doing what you do; we log what
   the map said. Then we sit down with GROUND_TRUTH_PROTOCOL.md and see if
   it surfaced anything earlier or truer than your current process. If it
   didn't, that's a finding too, and we rethink the model before scaling."
