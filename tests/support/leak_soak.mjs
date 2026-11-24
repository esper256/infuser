// Test-only CDP measurements. No browser-side memory estimate or clock control.
import assert from 'node:assert/strict';
import {mkdir, mkdtemp, writeFile} from 'node:fs/promises';
import {join, resolve} from 'node:path';

const metrics = ['JSHeapUsedSize', 'Nodes', 'JSEventListeners', 'Documents', 'Frames'];
const resources = metrics.slice(1);
export const heapAllowance = 320 * 1024;
const trendStep = 8 * 1024;

// Four non-overlapping 100-cycle low-water marks. Small downward steps within
// a block do not hide persistent growth, and a single allocation step is not a trend.
export function heapTrend(rows) {
  const measured = rows.filter(row => /^cycles-\d+$/.test(row.checkpoint));
  if (measured.length < 8) return {sustained: false, floors: []};
  const tail = measured.slice(-8), floors = [];
  for (let i = 0; i < tail.length; i += 2) floors.push(Math.min(tail[i].JSHeapUsedSize, tail[i + 1].JSHeapUsedSize));
  return {sustained: floors.slice(1).every((value, i) => value - floors[i] >= trendStep), floors};
}

export function checkHeap(rows) {
  const baseline = rows[0], row = rows.at(-1);
  for (const key of resources) assert.equal(row[key], baseline[key], `${row.checkpoint}: ${key} changed from baseline`);
  assert(Math.abs(row.JSHeapUsedSize - baseline.JSHeapUsedSize) <= heapAllowance,
    `${row.checkpoint}: post-GC heap moved more than 320 KiB from baseline`);
  const trend = heapTrend(rows);
  assert(!trend.sustained, `${row.checkpoint}: sustained post-GC heap growth: four 100-cycle low-water marks rose by at least 8 KiB each (${trend.floors.join(', ')})`);
}

function graph(snapshot) {
  const {node_fields: nf, edge_fields: ef, node_types: nt, edge_types: et} = snapshot.snapshot.meta;
  const field = name => nf.indexOf(name), edgeField = name => ef.indexOf(name);
  const width = nf.length, edgeWidth = ef.length;
  const nodes = snapshot.nodes, edges = snapshot.edges, strings = snapshot.strings;
  const count = nodes.length / width, starts = new Uint32Array(count + 1);
  let cursor = 0;
  for (let i = 0; i < count; i++) {
    starts[i] = cursor;
    cursor += nodes[i * width + field('edge_count')] * edgeWidth;
  }
  starts[count] = cursor;
  const node = i => ({
    id: nodes[i * width + field('id')],
    type: nt[field('type')][nodes[i * width + field('type')]],
    name: strings[nodes[i * width + field('name')]],
    size: nodes[i * width + field('self_size')],
  });
  const edge = offset => {
    const type = et[edgeField('type')][edges[offset + edgeField('type')]];
    const name = edges[offset + edgeField('name_or_index')];
    return {type, name: ['element', 'hidden'].includes(type) ? String(name) : strings[name], to: edges[offset + edgeField('to_node')] / width};
  };
  return {node, edge, count, starts, edgeWidth};
}

function inventory(g) {
  const ids = new Map(), constructors = new Map();
  for (let i = 0; i < g.count; i++) {
    const n = g.node(i);
    ids.set(n.id, n.size);
    // V8 names object/native nodes by constructor; retain storage/closure kinds too.
    if (!['object', 'native', 'closure', 'array'].includes(n.type)) continue;
    const key = `${n.type}: ${n.name}`;
    const value = constructors.get(key) || {count: 0, bytes: 0};
    value.count++; value.bytes += n.size;
    constructors.set(key, value);
  }
  return {ids, constructors};
}

// Keep inspector-only retainers separate, without subtracting their memory from
// measurements. An alternate application path takes precedence over an inspector path.
const inspector = /blink::(?:\w*DevTools\w*|Inspector\w*|NetworkResourcesData)|v8_inspector::/;
const instrumentationEdge = name => name === '__leakSoak' || /DevTools console|v8_inspector::/.test(name);
function paths(g, excludeInstrumentation) {
  const unseen = 0xffffffff, parent = new Uint32Array(g.count).fill(unseen);
  const parentEdge = new Uint32Array(g.count), queue = new Uint32Array(g.count);
  let read = 0, end = 1; queue[0] = 0; parent[0] = 0;
  while (read < end) {
    const from = queue[read++];
    for (let offset = g.starts[from]; offset < g.starts[from + 1]; offset += g.edgeWidth) {
      const e = g.edge(offset);
      if (e.type === 'weak' || parent[e.to] !== unseen) continue;
      if (excludeInstrumentation && (inspector.test(g.node(e.to).name) || instrumentationEdge(e.name))) continue;
      parent[e.to] = from; parentEdge[e.to] = offset; queue[end++] = e.to;
    }
  }
  return {parent, parentEdge, unseen};
}
function retainer(g, tree, target) {
  const describe = i => {const n = g.node(i); return `${n.type} ${n.name.slice(0, 160) || '(root)'} @${n.id} (${n.size} bytes)`;};
  if (tree.parent[target] === tree.unseen) return [`No strong root path found for ${describe(target)}`];
  const result = [];
  for (let at = target; at !== 0; at = tree.parent[at]) {
    const e = g.edge(tree.parentEdge[at]);
    result.push(`  --${e.type} ${e.name}--> ${describe(at)}`);
  }
  return [describe(0), ...result.reverse()];
}
export function heapInventory(snapshot) {return inventory(graph(snapshot));}
export function heapGrowth(baseline, current) {
  const g = graph(current), counts = inventory(g).constructors;
  const constructors = [...counts].map(([constructor, value]) => ({
    constructor, count: value.count, grew: value.count - (baseline.constructors.get(constructor)?.count || 0),
    bytes: value.bytes - (baseline.constructors.get(constructor)?.bytes || 0),
  })).filter(row => row.grew > 0 || row.bytes > 0).sort((a, b) => b.bytes - a.bytes || b.grew - a.grew);
  const growing = new Set(constructors.map(row => row.constructor));
  const appPaths = paths(g, true), allPaths = paths(g, false), groups = new Map();
  for (let i = 0; i < g.count; i++) {
    const n = g.node(i), constructor = `${n.type}: ${n.name}`;
    const delta = n.size - (baseline.ids.get(n.id) || 0);
    if (delta <= 0 || !growing.has(constructor)) continue;
    const category = appPaths.parent[i] !== appPaths.unseen ? 'candidates'
      : allPaths.parent[i] !== allPaths.unseen ? 'instrumentation' : 'unreachable';
    const key = `${category}: ${constructor}`;
    const group = groups.get(key) || {category, constructor, newOrExpandedBytes: 0, largestGrowth: 0, target: i};
    group.newOrExpandedBytes += delta;
    if (delta > group.largestGrowth) {group.largestGrowth = delta; group.target = i;}
    groups.set(key, group);
  }
  const result = {constructors};
  for (const category of ['candidates', 'instrumentation', 'unreachable']) {
    result[category] = [...groups.values()].filter(group => group.category === category)
      .sort((a, b) => b.newOrExpandedBytes - a.newOrExpandedBytes).slice(0, 8)
      .map(({target, ...group}) => ({...group, retainer: retainer(g, category === 'candidates' ? appPaths : allPaths, target)}));
  }
  return result;
}

export async function leakSoak(cdp, {directory = '.local/leak-soak', settle = async () => {}} = {}) {
  await mkdir(directory, {recursive: true});
  const output = await mkdtemp(join(resolve(directory), 'run-'));
  await cdp.send('Performance.enable');
  await cdp.send('HeapProfiler.enable');
  const rows = [];
  let baseline, baselineHeap;
  async function snapshot(label) {
    const chunks = [], receive = ({chunk}) => chunks.push(chunk);
    cdp.on('HeapProfiler.addHeapSnapshotChunk', receive);
    try {await cdp.send('HeapProfiler.takeHeapSnapshot', {reportProgress: false});}
    finally {cdp.off('HeapProfiler.addHeapSnapshotChunk', receive);}
    const json = chunks.join(''), file = join(output, `${label}.heapsnapshot`);
    await writeFile(file, json);
    return {file, heap: JSON.parse(json)};
  }
  async function measure(label) {
    await settle();
    for (let i = 0; i < 3; i++) await cdp.send('HeapProfiler.collectGarbage');
    const response = await cdp.send('Performance.getMetrics');
    const available = new Map(response.metrics.map(m => [m.name, m.value]));
    const row = {checkpoint: label};
    for (const name of metrics) {
      assert(Number.isFinite(available.get(name)), `CDP did not report ${name}`);
      row[name] = available.get(name);
    }
    rows.push(row);
    const shot = await snapshot(label);
    return {row, ...shot};
  }
  async function checkpoint(label) {
    assert(/^[a-z0-9-]+$/.test(label), 'Use a filename-safe checkpoint label');
    const {row, heap} = await measure(label);
    if (!baseline) {baseline = row; baselineHeap = heapInventory(heap);}
    else checkHeap(rows);
    console.table([row]);
  }
  async function finish() {
    assert(rows.length >= 9, 'Need a baseline and at least eight checkpoints for the trend gate');
    const last = rows.slice(-3).map(row => row.JSHeapUsedSize);
    assert(!(last[0] < last[1] && last[1] < last[2]), 'Post-GC heap is strictly increasing across the last three checkpoints');
    await writeFile(join(output, 'results.json'), JSON.stringify({heapAllowance, trend: heapTrend(rows), checkpoints: rows}, null, 2));
    console.table(rows);
    console.log(`Heap snapshots: ${output}`);
  }
  async function failure(error) {
    const {file, heap} = await measure('failure');
    const detail = heapGrowth(baselineHeap || {ids: new Map(), constructors: new Map()}, heap);
    console.error(`Leak soak failed: ${error.message}\nFailure snapshot: ${file}`);
    console.table(rows);
    console.error('Constructor count or self-size growth:');
    for (const row of detail.constructors) console.error(`${row.constructor}: ${row.count} (${row.grew >= 0 ? '+' : ''}${row.grew}); self-size delta ${row.bytes} bytes`);
    console.error('Growth candidates are evidence to investigate, not proof of the leak source. New/expanded bytes can include replacement allocations.');
    for (const [category, heading] of Object.entries({
      candidates: 'Non-instrumentation growth candidates',
      instrumentation: 'DevTools / test instrumentation retention (still included in all metric gates)',
      unreachable: 'No strong root path found',
    })) {
      console.error(`${heading}:`);
      if (!detail[category].length) console.error('  None identified.');
      for (const group of detail[category]) console.error(`${group.constructor}: ${group.newOrExpandedBytes} new/expanded bytes; largest node +${group.largestGrowth} bytes\n${group.retainer.join('\n')}`);
    }
    await writeFile(join(output, 'failure.json'), JSON.stringify({error: error.message, heapAllowance, trend: heapTrend(rows), checkpoints: rows, ...detail}, null, 2));
  }
  return {checkpoint, finish, failure, output};
}
