'use strict';

const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join, resolve } = require('node:path');
const test = require('node:test');
const { PACKAGE_BY_PLATFORM } = require('../runtime/lib/platform');

const root = resolve(__dirname, '../../..');
const npmRoot = join(root, 'packages', 'npm');

function json(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function manifest(name) {
  return json(join(npmRoot, name, 'package.json'));
}

function cargoVersion() {
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8');
  const section = cargo.match(/\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/);
  const version = section && section[1].match(/^version\s*=\s*"([^"]+)"/m);
  assert.ok(version, 'workspace package version must exist');
  return version[1];
}

test('target inventory is complete and matches the runtime map', () => {
  const targets = json(join(npmRoot, 'targets.json')).targets;
  assert.equal(targets.length, 8);
  assert.equal(new Set(targets.map((target) => target.rustTarget)).size, 8);
  assert.equal(new Set(targets.map((target) => target.package)).size, 8);

  const mappedPackages = Object.values(PACKAGE_BY_PLATFORM).sort();
  assert.deepEqual(targets.map((target) => target.package).sort(), mappedPackages);
  assert.deepEqual(
    new Set(targets.map((target) => target.platform)),
    new Set(['darwin', 'linux', 'win32'])
  );
  for (const arch of ['arm64', 'x64']) {
    assert.ok(targets.some((target) => target.platform === 'darwin' && target.arch === arch));
    assert.ok(targets.some((target) => target.platform === 'win32' && target.arch === arch));
    for (const libc of ['glibc', 'musl']) {
      assert.ok(targets.some((target) => (
        target.platform === 'linux' && target.arch === arch && target.libc === libc
      )));
    }
  }
});

test('all publishable package versions and dependencies match Cargo', () => {
  const version = cargoVersion();
  const rootManifest = json(join(root, 'package.json'));
  const runtime = manifest('runtime');
  const cli = manifest('cli');
  const bun = manifest('bun');
  for (const packageManifest of [rootManifest, runtime, cli, bun]) {
    assert.equal(packageManifest.version, version);
    assert.equal(packageManifest.scripts && packageManifest.scripts.postinstall, undefined);
  }
  const targets = json(join(npmRoot, 'targets.json')).targets;
  assert.deepEqual(
    runtime.optionalDependencies,
    Object.fromEntries(targets.map((target) => [target.package, version]))
  );
  assert.deepEqual(cli.dependencies, { '@rsproxy/runtime': version });
  assert.deepEqual(bun.dependencies, { '@rsproxy/runtime': version });
});

test('Cargo packages cannot be published through crates.io', () => {
  const workspace = readFileSync(join(root, 'Cargo.toml'), 'utf8');
  const workspacePackage = workspace.match(/\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/);
  assert.ok(workspacePackage);
  assert.match(workspacePackage[1], /^publish\s*=\s*false$/m);
  for (const crate of ['rsproxy-cli', 'rsproxy-rules', 'rsproxy-trace']) {
    const cargo = readFileSync(join(root, 'crates', crate, 'Cargo.toml'), 'utf8');
    assert.match(cargo, /^publish\.workspace\s*=\s*true$/m);
  }
  assert.match(readFileSync(join(root, 'fuzz', 'Cargo.toml'), 'utf8'), /^publish\s*=\s*false$/m);
});

test('launcher licenses are exact copies of the repository license', () => {
  const license = readFileSync(join(root, 'LICENSE'), 'utf8');
  for (const packageName of ['runtime', 'cli', 'bun']) {
    assert.equal(readFileSync(join(npmRoot, packageName, 'LICENSE'), 'utf8'), license);
  }
});

test('npm and Bun expose only the rsproxy command through dedicated launchers', () => {
  const cli = manifest('cli');
  const bun = manifest('bun');
  assert.deepEqual(cli.bin, { rsproxy: 'bin/rsproxy.js' });
  assert.deepEqual(bun.bin, { rsproxy: 'bin/rsproxy.js' });
  assert.match(readFileSync(join(npmRoot, 'cli/bin/rsproxy.js'), 'utf8'), /^#!\/usr\/bin\/env node/);
  assert.match(readFileSync(join(npmRoot, 'bun/bin/rsproxy.js'), 'utf8'), /^#!\/usr\/bin\/env bun/);
});
