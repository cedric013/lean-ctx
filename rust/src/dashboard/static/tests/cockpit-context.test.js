#!/usr/bin/env node
/**
 * Regression tests for context telemetry scope labels.
 * Run with: node rust/src/dashboard/static/tests/cockpit-context.test.js
 */

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const componentPath = path.join(__dirname, '..', 'components', 'cockpit-context.js');
const source = fs.readFileSync(componentPath, 'utf8').replace(
  'export { CockpitContext };',
  'globalThis.CockpitContext = CockpitContext; globalThis.contextScopeLabel = contextScopeLabel; globalThis.contextUsage = contextUsage;'
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

const label = context.contextScopeLabel;
const cases = [
  [true, undefined, 'measured latest request'],
  [true, 0, 'measured recent request \u00b7 no active editor'],
  [true, 4, 'measured latest request \u00b7 4 editor sessions'],
  [false, undefined, 'shared estimate'],
  [false, 0, 'no active editor session'],
  [false, 1, '1 editor session \u00b7 shared estimate'],
  [false, 3, '3 editor sessions \u00b7 shared estimate'],
];

for (const [measured, count, expected] of cases) {
  const actual = label(measured, count);
  if (actual !== expected) {
    throw new Error(`scope label mismatch: expected "${expected}", got "${actual}"`);
  }
}

const estimated = context.contextUsage(false, 190000, 12000, 128000);
if (estimated.total !== 12000 || estimated.utilization !== 0.09375) {
  throw new Error('shared estimate did not use visible telemetry');
}

const measured = context.contextUsage(true, 64000, 12000, 128000);
if (measured.total !== 64000 || measured.utilization !== 0.5) {
  throw new Error('measured request did not use proxy telemetry');
}

console.log('PASS: Context telemetry distinguishes measured requests and shared estimates');
