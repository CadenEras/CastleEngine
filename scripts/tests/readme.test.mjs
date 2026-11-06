// README version badges must match the versions actually pinned in the repository.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const read = file => readFileSync(path.join(root, file), 'utf8');

const readme = read('README.md');
const pkg = JSON.parse(read('apps/desktop/package.json'));

function badge(label) {
  const match = readme.match(new RegExp(`img\\.shields\\.io/badge/${label}-([^-?]+)-`));
  assert.ok(match, `README badge ${label} not found`);
  return match[1];
}

function pinned(text, pattern, what) {
  const match = text.match(pattern);
  assert.ok(match, `${what} not found`);
  return match[1];
}

test('Rust badge matches rust-toolchain.toml', () => {
  assert.equal(badge('Rust'),
    pinned(read('rust-toolchain.toml'), /^channel = "([^"]+)"/m, 'toolchain channel'));
});

test('Node.js badge matches the CI Node version', () => {
  assert.equal(badge('Node.js'),
    pinned(read('.github/workflows/desktop.yml'), /NODE_VERSION: "([^"]+)"/, 'NODE_VERSION'));
});

test('frontend badges match package.json', () => {
  assert.equal(badge('React'), pkg.dependencies.react);
  assert.equal(badge('TypeScript'), pkg.devDependencies.typescript);
  assert.equal(badge('Vite'), pkg.devDependencies.vite);
});

test('Tauri badge matches the major.minor of the tauri crate and CLI', () => {
  const crate = pinned(read('apps/desktop/src-tauri/Cargo.toml'),
    /^tauri = \{ version = "([^"]+)"/m, 'tauri crate version');
  for (const version of [crate, pkg.devDependencies['@tauri-apps/cli']]) {
    assert.equal(badge('Tauri'), version.split('.').slice(0, 2).join('.'));
  }
});
