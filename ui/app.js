// Thin demo of the Groundwork API. All logic is in the API; this just renders.
const map = new maplibregl.Map({
  container: 'map',
  style: 'https://basemaps.cartocdn.com/gl/positron-gl-style/style.json', // © CARTO © OpenStreetMap contributors
  center: [-73.85, 40.9],
  zoom: 9,
});

// Actionable next steps for a place: get help first, then donate/volunteer.
// Links go straight to the organizations — Groundwork handles no money.
async function renderActions(geoUnitId, placeName) {
  const el = document.getElementById('actions');
  const res = await fetch(`/v1/actions${geoUnitId ? '?geo_unit_id=' + geoUnitId : ''}`);
  const data = await res.json();
  const order = { get_help: 0, donate: 1, volunteer: 2 };
  const label = { get_help: 'Get help', donate: 'Donate', volunteer: 'Volunteer' };
  const groups = {};
  for (const r of data.resources) (groups[r.kind] = groups[r.kind] || []).push(r);
  let html = `<h2>Take action${placeName ? ' — ' + placeName : ''}</h2>`;
  for (const kind of Object.keys(groups).sort((a, b) => order[a] - order[b])) {
    html += `<div style="font-weight:600;font-size:11px;margin-top:6px">${label[kind] || kind}</div>`;
    for (const r of groups[kind]) {
      html += `<a class="act ${r.kind}" href="${r.url}" target="_blank" rel="noopener">
        ${r.title}<div class="org">${r.org}${r.note ? ' — ' + r.note : ''}</div></a>`;
    }
  }
  html += `<div class="fine">${data.disclaimer}</div>`;
  el.innerHTML = html;
}

renderActions(null, null); // full register until a tract is picked; no need to wait for the map

// If the basemap CDN stalls, fall back to a blank canvas after 10s so the
// nowcast layer (the actual product) still renders.
const basemapFallback = setTimeout(() => {
  if (!map.loaded()) {
    console.warn('basemap stalled; falling back to blank style');
    map.setStyle({ version: 8, sources: {}, layers: [{ id: 'bg', type: 'background', paint: { 'background-color': '#eef0f2' } }] });
    // setStyle does not re-fire 'load', and 'styledata' can fire before the
    // style is usable — poll until addLayer is safe.
    const tryAdd = () => (map.isStyleLoaded() ? addNowcastLayer().catch(console.error) : setTimeout(tryAdd, 250));
    tryAdd();
  }
}, 10000);

map.on('load', () => {
  clearTimeout(basemapFallback);
  addNowcastLayer();
});

async function addNowcastLayer() {
  if (map.getSource('nowcast')) return;
  const res = await fetch('/v1/nowcast?geometry=centroid');
  const fc = await res.json();

  // Render tract centroids as dots (M0 style); polygons stay available in
  // the same payload for a choropleth later.
  const dots = {
    type: 'FeatureCollection',
    features: fc.features
      .filter((f) => f.properties.centroid && f.properties.centroid.type)
      .map((f) => ({
        type: 'Feature',
        geometry: f.properties.centroid,
        properties: {
          geo_unit_id: f.properties.geo_unit_id,
          name: f.properties.name,
          nowcast_gap: f.properties.nowcast_gap,
          baseline_gap: f.properties.baseline_gap,
          delta: f.properties.nowcast_gap - f.properties.baseline_gap,
          uncertainty: f.properties.uncertainty,
          coverage_score: f.properties.coverage_score,
          news_decomposition: JSON.stringify(f.properties.news_decomposition),
          as_of: f.properties.as_of,
        },
      })),
  };

  map.addSource('nowcast', { type: 'geojson', data: dots });
  map.addLayer({
    id: 'nowcast-dots',
    type: 'circle',
    source: 'nowcast',
    paint: {
      'circle-radius': ['interpolate', ['linear'], ['get', 'nowcast_gap'], 0, 2.5, 0.12, 5, 0.3, 9],
      'circle-color': [
        'interpolate', ['linear'], ['get', 'delta'],
        0, '#2c7fb8',
        0.005, '#fd8d3c',
        0.02, '#e31a1c',
      ],
      // Low coverage reads as faded, never as low need.
      'circle-opacity': ['interpolate', ['linear'], ['get', 'coverage_score'], 0, 0.25, 1, 0.85],
      'circle-stroke-width': 0.5,
      'circle-stroke-color': '#fff',
    },
  });

  map.on('click', 'nowcast-dots', async (e) => {
    const p = e.features[0].properties;
    const decomposition = JSON.parse(p.news_decomposition || '[]');
    const detail = document.getElementById('detail');
    const pct = (x) => (100 * x).toFixed(1) + '%';
    let html = `<h3 style="margin:10px 0 2px">${p.name}</h3>
      <div>nowcast <b>${pct(p.nowcast_gap)}</b> vs baseline ${pct(p.baseline_gap)}
      &nbsp;·&nbsp; ±${pct(p.uncertainty)} &nbsp;·&nbsp; coverage ${(p.coverage_score * 100).toFixed(0)}%</div>`;
    if (!decomposition.length) {
      html += `<div class="sig">No active signals — nowcast equals baseline.</div>`;
    }
    for (const d of decomposition) {
      html += `<div class="sig">
        <span class="contrib">${d.contribution >= 0 ? '+' : ''}${(100 * d.contribution).toFixed(2)}pp</span>
        ${d.signal_type} (recency ×${d.recency_factor.toFixed(2)}, gameability ×${d.gameability_discount})
        <div class="excerpt" data-id="${d.signal_id}">loading evidence…</div>
        <a href="/v1/signals/${d.signal_id}" target="_blank">full signal + provenance →</a>
      </div>`;
    }
    detail.innerHTML = html;
    renderActions(p.geo_unit_id, p.name);
    // Click down to the raw evidence: the loop that IS the UX.
    for (const d of decomposition) {
      fetch(`/v1/signals/${d.signal_id}`)
        .then((r) => r.json())
        .then((s) => {
          const el = detail.querySelector(`.excerpt[data-id="${d.signal_id}"]`);
          if (el) el.innerHTML = `“${s.raw_excerpt}” — <a href="${s.provenance_url}" target="_blank">source</a>`;
        })
        .catch(() => {});
    }
  });
  map.on('mouseenter', 'nowcast-dots', () => (map.getCanvas().style.cursor = 'pointer'));
  map.on('mouseleave', 'nowcast-dots', () => (map.getCanvas().style.cursor = ''));
}
