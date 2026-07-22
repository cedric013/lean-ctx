#!/usr/bin/env node
/**
 * Regression tests for honest savings-ledger status on the Overview bridge.
 * Run with: node rust/src/dashboard/static/tests/cockpit-overview.test.js
 */

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const componentPath = path.join(__dirname, '..', 'components', 'cockpit-overview.js');
const source = fs.readFileSync(componentPath, 'utf8').replace(
  'export { CockpitOverview };',
  'globalThis.CockpitOverview = CockpitOverview;'
);

class HTMLElement {}
const context = {
  console,
  customElements: { define() {} },
  document: { querySelector() { return null; } },
  HTMLElement,
  window: {},
};
context.globalThis = context;
vm.runInNewContext(source, context, { filename: componentPath });

const overview = new context.CockpitOverview();
const format = (value) => String(value);

function renderBridge(overrides) {
  overview._data = {
    roi: {
      roi: {
        total_events: 1,
        net_saved_tokens: 100,
        saved_usd: 0.25,
        chain_valid: true,
        signed: true,
        ...overrides,
      },
      trend: [],
    },
  };
  return overview._verifiedBridge(format, format, format);
}

const verified = renderBridge({});
if (!verified.includes('<span class="tag tg">verified</span>')) {
  throw new Error('valid signed ledger is not labelled verified');
}
if (!verified.includes('net tokens saved')) {
  throw new Error('ledger total is not identified as net savings');
}
if (!verified.includes('separate measured ledger')) {
  throw new Error('ledger scope is not distinguished from the estimate');
}

const broken = renderBridge({ chain_valid: false });
if (!broken.includes('<span class="tag td">chain BROKEN</span>')) {
  throw new Error('broken ledger chain is still presented as verified');
}

const unsigned = renderBridge({ signed: false });
if (!unsigned.includes('<span class="tag ty">unsigned</span>')) {
  throw new Error('unsigned ledger is still presented as verified');
}

console.log('PASS: Overview bridge reflects ledger verification state');
