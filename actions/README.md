# Actions register

Curated links shown alongside the nowcast: where to **get help**, **donate**,
or **volunteer** in each county. Served by `GET /v1/actions?geo_unit_id=...`.

This does NOT soften commitment #3 (*we inform, we do not allocate*):

- **No money flows through Groundwork.** Links go directly to independent
  organizations. We never process, hold, or route a donation.
- **Listing is not a ranking.** Resources are returned in stable file order,
  not ordered by need, effectiveness, or any score. Groundwork's nowcast says
  where need appears to be moving; it does not say which organization
  "deserves" support.
- **Curation is a PR.** Adding/removing a resource is a pull request against
  `actions.v1.json` with a rationale. Criteria: the organization serves the
  listed counties, is an established food-security or referral service
  (food bank, 211, government benefits portal), and the URL is live.
  Corrections welcome — especially from people with local knowledge.
- **Get-help links come first.** The register exists for people in need
  first, donors second.

## The "systemic" tier

`kind: "systemic"` entries are the big levers — interventions with published
evidence of changing a need category at scale (Housing First / Built for Zero
for homelessness, SNAP and the expanded Child Tax Credit for hunger and child
poverty, GiveWell-vetted programs globally). Criteria are stricter than the
other tiers: the entry's `note` must state the evidence claim, and the claim
must be attributable to published research or program data. Groundwork still
allocates nothing and ranks nothing — "eradicating" a need is policy and
sustained local work; these links point at the organizations doing that work
measurably.

URLs verified live 2026-06-11 (endhomelessness.org blocks bots; verified in
a browser).
