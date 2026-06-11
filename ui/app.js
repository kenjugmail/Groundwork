// Groundwork demo UI. All analysis here is presentation of API data — the
// fusion model, weights, and provenance all live server-side.

// ---------- locations ----------
const LOCATIONS = [
  { id: 'all',   name: 'NYC + Westchester (all tracts)', bbox: [-74.27, 40.49, -73.45, 41.37] },
  { id: '36005', name: 'Bronx',          bbox: [-73.94, 40.78, -73.74, 40.92] },
  { id: '36047', name: 'Brooklyn',       bbox: [-74.05, 40.55, -73.83, 40.74] },
  { id: '36061', name: 'Manhattan',      bbox: [-74.05, 40.68, -73.90, 40.88] },
  { id: '36081', name: 'Queens',         bbox: [-73.96, 40.54, -73.70, 40.81] },
  { id: '36085', name: 'Staten Island',  bbox: [-74.26, 40.49, -74.05, 40.65] },
  { id: '36119', name: 'Westchester',    bbox: [-73.99, 40.87, -73.48, 41.37] },
  { id: 'us',    name: '🇺🇸 United States (counties, annual baseline)', bbox: [-125, 24, -66, 50] },
  { id: 'world', name: '🌍 World (annual baseline)', bbox: [-160, -55, 170, 72] },
];

// US county metrics (County Health Rankings). Color stops are in metric units.
const METRICS = {
  chr_food_insecurity_rate:     { label: 'Food insecurity',        fmt: (v) => (100 * v).toFixed(1) + '%', stops: [0.05, '#2c7fb8', 0.12, '#fdae61', 0.20, '#e31a1c', 0.30, '#7f0000'], higherWorse: true },
  chr_child_poverty_rate:       { label: 'Children in poverty',    fmt: (v) => (100 * v).toFixed(1) + '%', stops: [0.05, '#2c7fb8', 0.15, '#fdae61', 0.30, '#e31a1c', 0.45, '#7f0000'], higherWorse: true },
  chr_unemployment_rate:        { label: 'Unemployment',           fmt: (v) => (100 * v).toFixed(1) + '%', stops: [0.02, '#2c7fb8', 0.05, '#fdae61', 0.08, '#e31a1c', 0.12, '#7f0000'], higherWorse: true },
  chr_uninsured_rate:           { label: 'Uninsured',              fmt: (v) => (100 * v).toFixed(1) + '%', stops: [0.05, '#2c7fb8', 0.12, '#fdae61', 0.20, '#e31a1c', 0.30, '#7f0000'], higherWorse: true },
  chr_median_household_income:  { label: 'Median household income', fmt: (v) => '$' + Math.round(v).toLocaleString(), stops: [45000, '#7f0000', 60000, '#e31a1c', 80000, '#fdae61', 110000, '#2c7fb8'], higherWorse: false },
  chr_severe_housing_rate:      { label: 'Severe housing problems', fmt: (v) => (100 * v).toFixed(1) + '%', stops: [0.08, '#2c7fb8', 0.15, '#fdae61', 0.22, '#e31a1c', 0.32, '#7f0000'], higherWorse: true },
};
const ELEVATED_PP = 0.002; // delta above baseline (in rate points) we call "elevated"

let state = { loc: 'all', view: 'dots', metric: 'chr_food_insecurity_rate', nowcastFC: null, worldData: null, usData: null };

const map = new maplibregl.Map({
  container: 'map',
  style: 'https://basemaps.cartocdn.com/gl/positron-gl-style/style.json', // © CARTO © OpenStreetMap contributors
  center: [-73.85, 40.9],
  zoom: 9,
});

// If the basemap CDN stalls, fall back to a blank canvas so the data layers
// (the actual product) still render.
const basemapFallback = setTimeout(() => {
  if (!map.loaded()) {
    console.warn('basemap stalled; falling back to blank style');
    map.setStyle({ version: 8, sources: {}, layers: [{ id: 'bg', type: 'background', paint: { 'background-color': '#dfe6ec' } }] });
    const tryAdd = () => (map.isStyleLoaded() ? init() : setTimeout(tryAdd, 250));
    tryAdd();
  }
}, 10000);
map.on('load', () => { clearTimeout(basemapFallback); init(); });

// ---------- data ----------
async function fetchNowcast() {
  if (!state.nowcastFC) {
    const res = await fetch('/v1/nowcast?geometry=centroid');
    state.nowcastFC = await res.json();
  }
  return state.nowcastFC;
}
async function fetchUs() {
  if (!state.usData) {
    const res = await fetch('/v1/us');
    state.usData = await res.json();
  }
  return state.usData;
}
async function fetchWorld() {
  if (!state.worldData) {
    const [wb, geo] = await Promise.all([
      fetch('/v1/world').then((r) => r.json()),
      fetch('data/countries.geo.json').then((r) => r.json()),
    ]);
    const byIso = {};
    for (const c of wb.countries) byIso[c.iso3] = c;
    for (const f of geo.features) {
      const d = byIso[f.id];
      f.properties.iso3 = f.id;
      f.properties.undernourishment_pct = d ? d.undernourishment_pct : null;
      f.properties.year = d ? d.year : null;
      f.properties.provenance_url = d ? d.provenance_url : null;
    }
    geo.features = geo.features.filter((f) => f.properties.undernourishment_pct !== null);
    state.worldData = { wb, geo };
  }
  return state.worldData;
}

// ---------- map layers ----------
let initialized = false;
async function init() {
  if (initialized) return;
  initialized = true;

  const fc = await fetchNowcast();
  const dots = {
    type: 'FeatureCollection',
    features: fc.features
      .filter((f) => f.properties.centroid && f.properties.centroid.type)
      .map((f) => ({
        type: 'Feature',
        geometry: f.properties.centroid,
        properties: flatProps(f.properties),
      })),
  };
  map.addSource('nowcast', { type: 'geojson', data: dots });
  map.addLayer({
    id: 'nowcast-dots',
    type: 'circle',
    source: 'nowcast',
    paint: {
      'circle-radius': ['interpolate', ['linear'], ['get', 'nowcast_gap'], 0, 2.5, 0.12, 5, 0.3, 9],
      'circle-color': deltaColor('delta'),
      'circle-opacity': ['interpolate', ['linear'], ['get', 'coverage_score'], 0, 0.25, 1, 0.85],
      'circle-stroke-width': 0.5,
      'circle-stroke-color': '#fff',
    },
  });
  map.on('click', 'nowcast-dots', (e) => showTract(e.features[0].properties));
  hoverCursor('nowcast-dots');

  buildControls();
  renderAll();
}

function flatProps(p) {
  return {
    geo_unit_id: p.geo_unit_id,
    name: p.name,
    nowcast_gap: p.nowcast_gap,
    baseline_gap: p.baseline_gap,
    delta: p.nowcast_gap - p.baseline_gap,
    uncertainty: p.uncertainty,
    coverage_score: p.coverage_score,
    news_decomposition: JSON.stringify(p.news_decomposition),
    as_of: p.as_of,
    weights_version: p.weights_version,
  };
}
function deltaColor(prop) {
  return ['interpolate', ['linear'], ['get', prop], 0, '#2c7fb8', 0.005, '#fd8d3c', 0.02, '#e31a1c'];
}
function hoverCursor(layer) {
  map.on('mouseenter', layer, () => (map.getCanvas().style.cursor = 'pointer'));
  map.on('mouseleave', layer, () => (map.getCanvas().style.cursor = ''));
}

async function ensureChoropleth() {
  if (map.getSource('nowcast-poly')) return;
  // Polygons are heavy; fetched once, on first toggle.
  const res = await fetch('/v1/nowcast');
  const fc = await res.json();
  for (const f of fc.features) f.properties = flatProps(f.properties);
  map.addSource('nowcast-poly', { type: 'geojson', data: fc });
  map.addLayer(
    {
      id: 'nowcast-fill',
      type: 'fill',
      source: 'nowcast-poly',
      layout: { visibility: 'none' },
      paint: {
        'fill-color': deltaColor('delta'),
        'fill-opacity': ['interpolate', ['linear'], ['get', 'coverage_score'], 0, 0.2, 1, 0.55],
        'fill-outline-color': '#ffffff',
      },
    },
    'nowcast-dots'
  );
  map.on('click', 'nowcast-fill', (e) => showTract(e.features[0].properties));
  hoverCursor('nowcast-fill');
}

function usColorExpr(metric) {
  const m = METRICS[metric];
  const expr = ['interpolate', ['linear'], ['get', metric, ['get', 'metrics']]];
  for (let i = 0; i < m.stops.length; i += 2) expr.push(m.stops[i], m.stops[i + 1]);
  return expr;
}

async function ensureUsLayer() {
  if (map.getSource('us')) return;
  const us = await fetchUs();
  map.addSource('us', { type: 'geojson', data: us });
  map.addLayer({
    id: 'us-fill',
    type: 'fill',
    source: 'us',
    layout: { visibility: 'none' },
    paint: {
      'fill-color': usColorExpr(state.metric),
      'fill-opacity': 0.7,
      'fill-outline-color': '#ffffff',
    },
  });
  map.on('click', 'us-fill', (e) => showUsCounty(e.features[0].properties));
  hoverCursor('us-fill');
}

async function ensureWorldLayer() {
  if (map.getSource('world')) return;
  const { geo } = await fetchWorld();
  map.addSource('world', { type: 'geojson', data: geo });
  map.addLayer({
    id: 'world-fill',
    type: 'fill',
    source: 'world',
    layout: { visibility: 'none' },
    paint: {
      'fill-color': ['interpolate', ['linear'], ['get', 'undernourishment_pct'],
        2.5, '#2c7fb8', 10, '#fdae61', 25, '#e31a1c', 50, '#7f0000'],
      'fill-opacity': 0.65,
      'fill-outline-color': '#ffffff',
    },
  });
  map.on('click', 'world-fill', (e) => showCountry(e.features[0].properties));
  hoverCursor('world-fill');
}

// ---------- controls ----------
function buildControls() {
  const sel = document.getElementById('loc');
  sel.innerHTML = LOCATIONS.map((l) => `<option value="${l.id}">${l.name}</option>`).join('');
  sel.onchange = () => { state.loc = sel.value; renderAll(); };
  const vt = document.getElementById('viewToggle');
  vt.onclick = async () => {
    state.view = state.view === 'dots' ? 'choropleth' : 'dots';
    vt.classList.toggle('on', state.view === 'choropleth');
    if (state.view === 'choropleth') await ensureChoropleth();
    applyVisibility();
  };
  const ms = document.getElementById('metric');
  ms.innerHTML = Object.entries(METRICS).map(([k, m]) => `<option value="${k}">${m.label}</option>`).join('');
  ms.onchange = () => {
    state.metric = ms.value;
    if (map.getLayer('us-fill')) map.setPaintProperty('us-fill', 'fill-color', usColorExpr(state.metric));
    renderLegend('us');
    renderUsAnalytics();
  };
}

async function applyVisibility() {
  const world = state.loc === 'world';
  const us = state.loc === 'us';
  if (world) await ensureWorldLayer();
  if (us) await ensureUsLayer();
  const setVis = (id, on) => map.getLayer(id) && map.setLayoutProperty(id, 'visibility', on ? 'visible' : 'none');
  setVis('world-fill', world);
  setVis('us-fill', us);
  setVis('nowcast-dots', !world && !us && state.view === 'dots');
  setVis('nowcast-fill', !world && !us && state.view === 'choropleth');
  document.getElementById('metric').style.display = us ? '' : 'none';
  document.getElementById('viewToggle').style.display = world || us ? 'none' : '';
}

async function renderAll() {
  const loc = LOCATIONS.find((l) => l.id === state.loc);
  map.fitBounds([[loc.bbox[0], loc.bbox[1]], [loc.bbox[2], loc.bbox[3]]], { padding: 30, duration: 800 });
  await applyVisibility();
  document.getElementById('detail').innerHTML = '';
  if (state.loc === 'world') {
    renderLegend('world');
    document.getElementById('modeNote').textContent =
      'World view is the SLOW clock only: annual FAO/World Bank undernourishment, country resolution. No nowcast — Groundwork has no real-time sources at world scale and does not pretend to.';
    await renderWorldAnalytics();
    renderActions('world', 'World');
  } else if (state.loc === 'us') {
    renderLegend('us');
    document.getElementById('modeNote').textContent =
      'US view is the SLOW clock: annual County Health Rankings baselines across need categories, county resolution. Tract-level nowcasting runs only inside the sensing metro (NYC + Westchester).';
    await renderUsAnalytics();
    renderActions('us', 'United States');
  } else {
    renderLegend('local');
    document.getElementById('modeNote').textContent =
      'Fast clock: tract-level nowcast, recomputed as signals arrive. A nowcast is a signal, not proof. Faded = low coverage, never low need.';
    await renderLocalAnalytics();
    renderActions(state.loc === 'all' ? null : state.loc + '000000', loc.name);
  }
}

function renderLegend(mode) {
  const el = document.getElementById('legend');
  if (mode === 'world') {
    el.innerHTML = `<span style="background:#2c7fb8"></span>&lt;5% <span style="background:#fdae61"></span>10% <span style="background:#e31a1c"></span>25% <span style="background:#7f0000"></span>50%+ undernourished <span class="worldtag">annual baseline</span>`;
  } else if (mode === 'us') {
    const m = METRICS[state.metric];
    const chips = [];
    for (let i = 0; i < m.stops.length; i += 2)
      chips.push(`<span style="background:${m.stops[i + 1]}"></span>${m.fmt(m.stops[i])}`);
    el.innerHTML = `${chips.join(' ')} — ${m.label} <span class="worldtag">annual baseline</span>`;
  } else {
    el.innerHTML = `<span style="background:#2c7fb8"></span>at baseline <span style="background:#fd8d3c"></span>elevated <span style="background:#e31a1c"></span>widening`;
  }
}

// ---------- analytics ----------
function tractsInScope() {
  const fs = state.nowcastFC.features.map((f) => f.properties);
  return state.loc === 'all' ? fs : fs.filter((p) => p.geo_unit_id.startsWith(state.loc));
}
const pct = (x, d = 1) => (100 * x).toFixed(d) + '%';

async function renderLocalAnalytics() {
  await fetchNowcast();
  const t = tractsInScope();
  const el = document.getElementById('analytics');
  if (!t.length) { el.innerHTML = '<h2>Analysis</h2><div>No tracts in scope.</div>'; return; }

  const deltas = t.map((p) => p.nowcast_gap - p.baseline_gap);
  const mean = (a) => a.reduce((s, x) => s + x, 0) / a.length;
  const elevated = t.filter((p, i) => deltas[i] > ELEVATED_PP);
  const top = t
    .map((p) => ({ ...p, delta: p.nowcast_gap - p.baseline_gap }))
    .sort((a, b) => b.delta - a.delta)
    .slice(0, 5);

  // Signal-type totals across scope (from each tract's decomposition).
  const byType = {};
  for (const p of t)
    for (const d of p.news_decomposition || [])
      byType[d.signal_type] = (byType[d.signal_type] || 0) + Math.abs(d.contribution);
  const typeRows = Object.entries(byType).sort((a, b) => b[1] - a[1]);
  const typeMax = typeRows[0] ? typeRows[0][1] : 1;

  // Histogram of deltas (6 bins).
  const dmax = Math.max(...deltas, 0.001);
  const bins = new Array(6).fill(0);
  for (const d of deltas) bins[Math.min(5, Math.floor((d / dmax) * 5.999))]++;
  const bmax = Math.max(...bins, 1);

  el.innerHTML = `
    <h2>Analysis — ${LOCATIONS.find((l) => l.id === state.loc).name}</h2>
    <div class="statgrid">
      <div class="stat"><div class="v">${t.length}</div><div class="l">tracts</div></div>
      <div class="stat"><div class="v">${pct(mean(t.map((p) => p.nowcast_gap)))}</div><div class="l">mean nowcast gap</div></div>
      <div class="stat"><div class="v">${elevated.length}</div><div class="l">tracts above baseline</div></div>
      <div class="stat"><div class="v">${pct(mean(t.map((p) => p.baseline_gap)))}</div><div class="l">mean baseline</div></div>
      <div class="stat"><div class="v">+${pct(mean(deltas), 2)}</div><div class="l">mean Δ vs baseline</div></div>
      <div class="stat"><div class="v">${pct(mean(t.map((p) => p.coverage_score)), 0)}</div><div class="l">coverage</div></div>
    </div>
    <div class="chart"><div class="t">What's driving the movement (|contribution| by signal type)</div>
      ${typeRows.map(([k, v]) => `
        <div class="bar-row"><span class="lbl plain">${k}</span>
          <div class="bar b2" style="width:${(140 * v) / typeMax}px"></div>
          <span class="val">${(100 * v).toFixed(1)}pp·tracts</span></div>`).join('') || '<div style="font-size:11px;color:#777">No active signals in scope — nowcast equals baseline everywhere here.</div>'}
    </div>
    <div class="chart"><div class="t">Distribution of Δ above baseline (${t.length} tracts)</div>
      ${bins.map((b, i) => `
        <div class="bar-row"><span class="lbl plain">${pct((i * dmax) / 6, 2)}–${pct(((i + 1) * dmax) / 6, 2)}</span>
          <div class="bar b2" style="width:${(140 * b) / bmax}px"></div><span class="val">${b}</span></div>`).join('')}
    </div>
    <div class="chart"><div class="t">Most-widening tracts (click to inspect the evidence)</div>
      ${top.map((p) => `
        <div class="bar-row"><span class="lbl" data-geo="${p.geo_unit_id}">${p.name} (${p.geo_unit_id.slice(0, 5)})</span>
          <div class="bar" style="width:${(140 * p.delta) / (top[0].delta || 1)}px"></div>
          <span class="val">+${pct(p.delta, 2)}</span></div>`).join('')}
    </div>`;

  el.querySelectorAll('.lbl[data-geo]').forEach((n) => {
    n.onclick = () => {
      const p = t.find((x) => x.geo_unit_id === n.dataset.geo);
      const f = state.nowcastFC.features.find((x) => x.properties.geo_unit_id === n.dataset.geo);
      if (f && f.properties.centroid) map.flyTo({ center: f.properties.centroid.coordinates, zoom: 12 });
      showTract(flatProps(p));
    };
  });
}

async function renderUsAnalytics() {
  if (state.loc !== 'us') return;
  const us = await fetchUs();
  const m = METRICS[state.metric];
  const el = document.getElementById('analytics');
  const counties = us.features
    .map((f) => ({ ...f.properties, v: f.properties.metrics[state.metric] }))
    .filter((c) => typeof c.v === 'number');
  const states = us.states
    .map((s) => ({ ...s, v: s.metrics[state.metric] }))
    .filter((s) => typeof s.v === 'number');
  const worst = (a, b) => (m.higherWorse ? b.v - a.v : a.v - b.v);
  const topCounties = [...counties].sort(worst).slice(0, 10);
  const topStates = [...states].sort(worst).slice(0, 8);
  const mean = counties.reduce((s, c) => s + c.v, 0) / counties.length;
  const maxBar = Math.abs(topCounties[0].v) || 1;

  // 6-bin histogram over county values.
  const vals = counties.map((c) => c.v);
  const lo = Math.min(...vals), hi = Math.max(...vals);
  const bins = new Array(6).fill(0);
  for (const v of vals) bins[Math.min(5, Math.floor(((v - lo) / (hi - lo || 1)) * 5.999))]++;
  const bmax = Math.max(...bins, 1);

  el.innerHTML = `
    <h2>Analysis — United States <span class="worldtag">slow clock</span></h2>
    <div class="statgrid">
      <div class="stat"><div class="v">${counties.length}</div><div class="l">counties with data</div></div>
      <div class="stat"><div class="v">${m.fmt(mean)}</div><div class="l">county mean — ${m.label.toLowerCase()}</div></div>
      <div class="stat"><div class="v">${m.fmt(topCounties[0].v)}</div><div class="l">${m.higherWorse ? 'highest' : 'lowest'} (${topCounties[0].name})</div></div>
    </div>
    <div class="chart"><div class="t">Highest-need states (${m.higherWorse ? 'highest' : 'lowest'} ${m.label.toLowerCase()})</div>
      ${topStates.map((s) => `
        <div class="bar-row"><span class="lbl plain">${s.name}</span>
          <div class="bar b2" style="width:${(140 * Math.abs(s.v)) / Math.abs(topStates[0].v || 1)}px"></div>
          <span class="val">${m.fmt(s.v)}</span></div>`).join('')}
    </div>
    <div class="chart"><div class="t">Highest-need counties — ${m.higherWorse ? 'highest' : 'lowest'} ${m.label.toLowerCase()} (click to inspect)</div>
      ${topCounties.map((c) => `
        <div class="bar-row"><span class="lbl" data-geo="${c.geo_unit_id}">${c.name}</span>
          <div class="bar" style="width:${(140 * Math.abs(c.v)) / maxBar}px"></div>
          <span class="val">${m.fmt(c.v)}</span></div>`).join('')}
    </div>
    <div class="chart"><div class="t">Distribution across ${counties.length} counties</div>
      ${bins.map((b, i) => `
        <div class="bar-row"><span class="lbl plain">${m.fmt(lo + ((hi - lo) * i) / 6)}–${m.fmt(lo + ((hi - lo) * (i + 1)) / 6)}</span>
          <div class="bar b2" style="width:${(140 * b) / bmax}px"></div><span class="val">${b}</span></div>`).join('')}
    </div>
    <div class="disclaimer">Source: ${us.source} (<a href="${us.provenance_url}" target="_blank">provenance</a>). State bars are CHR state values; county mean is unweighted.</div>`;

  el.querySelectorAll('.lbl[data-geo]').forEach((n) => {
    n.onclick = () => {
      const f = us.features.find((x) => x.properties.geo_unit_id === n.dataset.geo);
      if (f) {
        showUsCounty(f.properties);
        const c = centroidOf(f.geometry);
        if (c) map.flyTo({ center: c, zoom: 7 });
      }
    };
  });
}

function centroidOf(geom) {
  // crude bbox center — fine for fly-to
  let xs = [], ys = [];
  const walk = (c) => (typeof c[0] === 'number' ? (xs.push(c[0]), ys.push(c[1])) : c.forEach(walk));
  walk(geom.coordinates);
  if (!xs.length) return null;
  return [(Math.min(...xs) + Math.max(...xs)) / 2, (Math.min(...ys) + Math.max(...ys)) / 2];
}

function showUsCounty(p) {
  const metrics = typeof p.metrics === 'string' ? JSON.parse(p.metrics) : p.metrics;
  const cards = Object.entries(METRICS)
    .filter(([k]) => typeof metrics[k] === 'number')
    .map(([k, m]) => `<div class="stat"><div class="v">${m.fmt(metrics[k])}</div><div class="l">${m.label}</div></div>`)
    .join('');
  document.getElementById('detail').innerHTML = `
    <h2>${p.name} <span class="worldtag">annual baseline</span></h2>
    <div class="statgrid">${cards}</div>
    <div class="sig">County Health Rankings & Roadmaps 2025 (University of Wisconsin Population Health
      Institute), compiled from federal sources (ACS, BLS LAUS, Feeding America, SAIPE).
      <br/><a href="https://www.countyhealthrankings.org/health-data" target="_blank">full measures + methodology →</a></div>
    <div class="guidance"><b>Acting on this:</b> these are annual baselines — where need is persistently
      concentrated, not what changed this week. The get-help links below route to services in any US county;
      Feeding America's locator finds the food bank serving this one.</div>`;
  renderActions(p.geo_unit_id, p.name);
}

async function renderWorldAnalytics() {
  const { wb, geo } = await fetchWorld();
  const el = document.getElementById('analytics');
  const joined = geo.features.map((f) => f.properties).sort((a, b) => b.undernourishment_pct - a.undernourishment_pct);
  const top = joined.slice(0, 10);
  const max = top[0] ? top[0].undernourishment_pct : 1;
  const world = wb.countries.find((c) => c.iso3 === 'WLD');
  el.innerHTML = `
    <h2>Analysis — World <span class="worldtag">slow clock</span></h2>
    <div class="statgrid">
      <div class="stat"><div class="v">${joined.length}</div><div class="l">countries with data</div></div>
      <div class="stat"><div class="v">${world ? world.undernourishment_pct.toFixed(1) + '%' : '—'}</div><div class="l">world undernourished (${world ? world.year : ''})</div></div>
      <div class="stat"><div class="v">${top[0] ? top[0].undernourishment_pct.toFixed(0) + '%' : '—'}</div><div class="l">highest (${top[0] ? top[0].name : ''})</div></div>
    </div>
    <div class="chart"><div class="t">Highest prevalence of undernourishment (click to view)</div>
      ${top.map((c) => `
        <div class="bar-row"><span class="lbl" data-iso="${c.iso3}">${c.name} (${c.year})</span>
          <div class="bar" style="width:${(140 * c.undernourishment_pct) / max}px"></div>
          <span class="val">${c.undernourishment_pct.toFixed(1)}%</span></div>`).join('')}
    </div>
    <div class="disclaimer">Source: ${wb.source}. Every value links to its World Bank series. ${wb.license}.</div>`;
  el.querySelectorAll('.lbl[data-iso]').forEach((n) => {
    n.onclick = () => {
      const f = geo.features.find((x) => x.properties.iso3 === n.dataset.iso);
      if (f) showCountry(f.properties);
    };
  });
}

// ---------- detail views ----------
async function showTract(p) {
  const decomposition = typeof p.news_decomposition === 'string' ? JSON.parse(p.news_decomposition || '[]') : (p.news_decomposition || []);
  const delta = p.nowcast_gap - p.baseline_gap;
  const detail = document.getElementById('detail');
  const sev = delta > 0.01 ? 'widening sharply' : delta > ELEVATED_PP ? 'elevated' : 'at baseline';
  let html = `<h2>${p.name} — ${sev}</h2>
    <div class="statgrid">
      <div class="stat"><div class="v">${pct(p.nowcast_gap)}</div><div class="l">nowcast gap</div></div>
      <div class="stat"><div class="v">${pct(p.baseline_gap)}</div><div class="l">baseline (MMG+ACS)</div></div>
      <div class="stat"><div class="v">±${pct(p.uncertainty)}</div><div class="l">uncertainty</div></div>
    </div>
    <div style="font-size:11px;color:#666">coverage ${(p.coverage_score * 100).toFixed(0)}% · as of ${new Date(p.as_of).toLocaleString()} · weights ${p.weights_version}</div>`;

  if (delta > ELEVATED_PP) {
    html += `<div class="guidance"><b>Acting on this:</b> the gap here appears to be widening beyond its baseline.
      Useful next steps — share the evidence below with a local pantry or food bank serving this county,
      direct neighbors to the get-help links, or support the county's food bank (links at the bottom).
      Verify with people who know this place: this is a signal, not proof.</div>`;
  } else {
    html += `<div class="guidance">No movement beyond baseline detected here right now. Baseline need still exists —
      the get-help and donate links below remain relevant year-round.</div>`;
  }

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
  for (const d of decomposition) {
    fetch(`/v1/signals/${d.signal_id}`)
      .then((r) => r.json())
      .then((s) => {
        const el = detail.querySelector(`.excerpt[data-id="${d.signal_id}"]`);
        if (el) el.innerHTML = `“${s.raw_excerpt}” — <a href="${s.provenance_url}" target="_blank">source</a>`;
      })
      .catch(() => {});
  }
  renderActions(p.geo_unit_id, p.name);
}

function showCountry(p) {
  document.getElementById('detail').innerHTML = `
    <h2>${p.name} <span class="worldtag">annual baseline</span></h2>
    <div class="statgrid">
      <div class="stat"><div class="v">${p.undernourishment_pct.toFixed(1)}%</div><div class="l">undernourished (${p.year})</div></div>
    </div>
    <div class="sig">FAO/World Bank prevalence of undernourishment — the share of the population whose
      habitual food consumption is insufficient for a normal, active, healthy life.
      <br/><a href="${p.provenance_url}" target="_blank">full series + methodology at the World Bank →</a></div>
    <div class="guidance"><b>Acting on this:</b> country-level data points to where global hunger is concentrated,
      not what any community needs this week. The organizations below operate at that scale; for local action,
      their country programmes and the FAO's data are the right starting points.</div>`;
  renderActions('world', p.name);
}

// ---------- actions ----------
async function renderActions(geoUnitId, placeName) {
  const el = document.getElementById('actions');
  const res = await fetch(`/v1/actions${geoUnitId ? '?geo_unit_id=' + geoUnitId : ''}`);
  const data = await res.json();
  const order = { get_help: 0, donate: 1, volunteer: 2, systemic: 3 };
  const label = { get_help: 'Get help', donate: 'Donate', volunteer: 'Volunteer', systemic: 'Systemic change — the big levers' };
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

renderActions(null, null);

// ---------- live refresh ----------
// The orchestrator re-ingests sources on their cadences and recomputes the
// nowcast; the UI re-pulls every 5 minutes so the map tracks it unattended.
const REFRESH_MS = 5 * 60 * 1000;
function markLive() {
  const lastAsOf = state.nowcastFC && state.nowcastFC.features.length
    ? new Date(Math.max(...state.nowcastFC.features.map((f) => Date.parse(f.properties.as_of)))) : null;
  document.getElementById('live').innerHTML =
    `<span class="pulse"></span>live · nowcast as of ${lastAsOf ? lastAsOf.toLocaleTimeString() : '…'}`;
}
async function refreshNowcast() {
  try {
    const res = await fetch('/v1/nowcast?geometry=centroid');
    const fc = await res.json();
    state.nowcastFC = fc;
    const src = map.getSource('nowcast');
    if (src) {
      src.setData({
        type: 'FeatureCollection',
        features: fc.features
          .filter((f) => f.properties.centroid && f.properties.centroid.type)
          .map((f) => ({ type: 'Feature', geometry: f.properties.centroid, properties: flatProps(f.properties) })),
      });
    }
    if (!['us', 'world'].includes(state.loc)) renderLocalAnalytics();
    markLive();
  } catch (e) {
    console.warn('live refresh failed; will retry', e);
  }
}
setInterval(refreshNowcast, REFRESH_MS);
// initial badge once first load lands
const liveBadgeWait = setInterval(() => { if (state.nowcastFC) { markLive(); clearInterval(liveBadgeWait); } }, 1000);
