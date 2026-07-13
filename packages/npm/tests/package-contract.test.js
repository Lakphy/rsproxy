'use strict';

const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { existsSync, readFileSync } = require('node:fs');
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

function cargoMetadata(manifestPath) {
  const args = ['metadata', '--format-version', '1', '--no-deps'];
  if (manifestPath) {
    args.push('--manifest-path', manifestPath);
  }
  const result = spawnSync('cargo', args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true
  });
  assert.ifError(result.error);
  assert.equal(
    result.status,
    0,
    `cargo metadata failed with status ${result.status ?? 'unknown'}:\n${result.stderr || result.stdout}`
  );
  return JSON.parse(result.stdout);
}

function workspacePackages(metadata) {
  assert.ok(Array.isArray(metadata.packages), 'cargo metadata packages must be an array');
  assert.ok(
    Array.isArray(metadata.workspace_members),
    'cargo metadata workspace_members must be an array'
  );
  assert.ok(metadata.workspace_members.length > 0, 'Cargo workspace must have members');
  const packagesById = new Map(metadata.packages.map((packageMetadata) => [
    packageMetadata.id,
    packageMetadata
  ]));
  return metadata.workspace_members.map((id) => {
    const packageMetadata = packagesById.get(id);
    assert.ok(packageMetadata, `cargo metadata omitted workspace member ${id}`);
    return packageMetadata;
  });
}

function cargoVersion() {
  const packages = workspacePackages(cargoMetadata());
  const versions = new Set(packages.map((packageMetadata) => packageMetadata.version));
  assert.equal(
    versions.size,
    1,
    `Cargo workspace package versions must agree: ${packages
      .map((packageMetadata) => `${packageMetadata.name}@${packageMetadata.version}`)
      .join(', ')}`
  );
  return versions.values().next().value;
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
  for (const packageManifest of [rootManifest, runtime, cli]) {
    assert.equal(packageManifest.version, version);
    assert.equal(packageManifest.scripts && packageManifest.scripts.postinstall, undefined);
  }
  const targets = json(join(npmRoot, 'targets.json')).targets;
  assert.deepEqual(
    runtime.optionalDependencies,
    Object.fromEntries(targets.map((target) => [target.package, version]))
  );
  assert.deepEqual(cli.dependencies, { '@rsproxy/runtime': version });
});

test('Cargo packages cannot be published through crates.io', () => {
  const workspace = workspacePackages(cargoMetadata());
  for (const packageMetadata of workspace) {
    assert.deepEqual(
      packageMetadata.publish,
      [],
      `${packageMetadata.name} must set publish = false`
    );
  }
  const fuzz = workspacePackages(cargoMetadata(join(root, 'fuzz', 'Cargo.toml')));
  assert.equal(fuzz.length, 1);
  assert.equal(fuzz[0].name, 'rsproxy-rules-fuzz');
  assert.deepEqual(fuzz[0].publish, [], 'rsproxy-rules-fuzz must set publish = false');
});

test('every published manifest declares the provenance repository', () => {
  // npm provenance rejects packages whose repository.url does not match the
  // publishing workflow's repository (E422 at publish time).
  const expected = {
    type: 'git',
    url: 'git+https://github.com/Lakphy/rsproxy.git'
  };
  for (const packageName of ['runtime', 'cli']) {
    assert.deepEqual(manifest(packageName).repository, expected);
  }
  const generator = readFileSync(join(npmRoot, 'scripts', 'package.mjs'), 'utf8');
  assert.ok(
    generator.includes(expected.url),
    'native package generator must stamp the provenance repository URL'
  );
});

test('launcher licenses are exact copies of the repository license', () => {
  const license = readFileSync(join(root, 'LICENSE'), 'utf8');
  for (const packageName of ['runtime', 'cli']) {
    assert.equal(readFileSync(join(npmRoot, packageName, 'LICENSE'), 'utf8'), license);
  }
});

test('npm and Bun share the same rsproxy launcher package', () => {
  const cli = manifest('cli');
  assert.deepEqual(cli.bin, { rsproxy: 'bin/rsproxy.js' });
  assert.match(readFileSync(join(npmRoot, 'cli/bin/rsproxy.js'), 'utf8'), /^#!\/usr\/bin\/env node/);
  assert.equal(existsSync(join(npmRoot, 'bun/package.json')), false);
});
