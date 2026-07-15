#!/usr/bin/env node
/**
 * Regression tests for stable Commander source-trail expansion (#834).
 * Run with: node rust/src/dashboard/static/tests/cockpit-commander.test.js
 */

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const componentPath = path.join(__dirname, '..', 'components', 'cockpit-commander.js');
const source = fs.readFileSync(componentPath, 'utf8').replace(
  'export { CockpitCommander };',
  'globalThis.CockpitCommander = CockpitCommander;'
);

class HTMLElement {}
const context = {
  console,
  customElements: { define() {} },
  document: { addEventListener() {}, querySelector() { return null; } },
  HTMLElement,
  window: {},
};
context.globalThis = context;
vm.runInNewContext(source, context, { filename: componentPath });

const commander = new context.CockpitCommander();
commander._powerMode = true;
commander._data = {
  items: [
    {
      path: '/workspace/a.rs',
      mode: 'full',
      tokens_sent: 10,
      eviction_score: 0.1,
      source_trail: [{ type: 'read', detail: 'trail-a' }],
    },
    {
      path: '/workspace/b.rs',
      mode: 'map',
      tokens_sent: 20,
      eviction_score: 0.2,
      source_trail: [{ type: 'read', detail: 'trail-b' }],
    },
  ],
};
commander._expandedTrails.add('/workspace/b.rs');

function assertStableExpansion(sortDir) {
  commander._sortKey = 'path';
  commander._sortDir = sortDir;
  const html = commander._renderPressureTable();
  const encodedPath = encodeURIComponent('/workspace/b.rs');

  if (!html.includes(`data-trail-path="${encodedPath}"`)) {
    throw new Error(`${sortDir}: trail toggle is not keyed by the file path`);
  }
  if (!html.includes('trail-b')) {
    throw new Error(`${sortDir}: expanded file lost its source trail`);
  }
  if (html.includes('trail-a')) {
    throw new Error(`${sortDir}: expansion moved to a different file`);
  }
}

assertStableExpansion('asc');
assertStableExpansion('desc');
commander._modeFilter = 'map';
assertStableExpansion('asc');
console.log('PASS: source-trail expansion remains keyed by path after sorting');
