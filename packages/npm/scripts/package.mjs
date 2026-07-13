#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const NPM_ROOT = resolve(SCRIPT_DIR, '..');
const ROOT = resolve(NPM_ROOT, '../..');

function fail(message) {
  process.stderr.write(`npm package: ${message}\n`);
  process.exit(1);
}

function parseOptions(values) {
  const options = {};
  for (let index = 0; index < values.length; index += 1) {
    const name = values[index];
    if (!name.startsWith('--') || index + 1 >= values.length) {
      fail(`invalid option list near ${name}`);
    }
    options[name.slice(2)] = values[index + 1];
    index += 1;
  }
  return options;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function workspaceVersion() {
  const result = spawnSync(
    'cargo',
    ['metadata', '--format-version', '1', '--no-deps'],
    {
      cwd: ROOT,
      encoding: 'utf8',
      maxBuffer: 16 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true
    }
  );
  if (result.error) {
    fail(`could not run cargo metadata: ${result.error.message}`);
  }
  if (result.status !== 0) {
    process.stderr.write(result.stdout || '');
    process.stderr.write(result.stderr || '');
    fail(`cargo metadata failed with status ${result.status ?? 'unknown'}`);
  }

  let metadata;
  try {
    metadata = JSON.parse(result.stdout);
  } catch (error) {
    fail(`cargo metadata returned invalid JSON: ${error.message}`);
  }
  if (!metadata || typeof metadata !== 'object') {
    fail('cargo metadata did not return a JSON object');
  }
  if (typeof metadata.workspace_root !== 'string' || resolve(metadata.workspace_root) !== ROOT) {
    fail(
      `cargo metadata returned unexpected workspace root ${metadata.workspace_root || '<missing>'}`
    );
  }
  if (!Array.isArray(metadata.packages) || !Array.isArray(metadata.workspace_members)) {
    fail('cargo metadata did not return a workspace package inventory');
  }
  if (metadata.workspace_members.length === 0) {
    fail('Cargo workspace has no members');
  }
  if (new Set(metadata.workspace_members).size !== metadata.workspace_members.length) {
    fail('cargo metadata returned duplicate workspace members');
  }
  if (metadata.packages.some((packageMetadata) => (
    !packageMetadata
      || typeof packageMetadata !== 'object'
      || typeof packageMetadata.id !== 'string'
      || packageMetadata.id.length === 0
  ))) {
    fail('cargo metadata returned a package without an id');
  }

  const packagesById = new Map(metadata.packages.map((packageMetadata) => [
    packageMetadata.id,
    packageMetadata
  ]));
  if (packagesById.size !== metadata.packages.length) {
    fail('cargo metadata returned duplicate package ids');
  }
  const members = metadata.workspace_members.map((id) => packagesById.get(id));
  const missingMembers = metadata.workspace_members.filter((_, index) => !members[index]);
  if (missingMembers.length > 0) {
    fail(`cargo metadata omitted workspace members: ${missingMembers.join(', ')}`);
  }

  const invalidMembers = members.filter((packageMetadata) => (
    typeof packageMetadata.name !== 'string'
      || packageMetadata.name.length === 0
      || typeof packageMetadata.version !== 'string'
      || packageMetadata.version.length === 0
  ));
  if (invalidMembers.length > 0) {
    fail('cargo metadata returned a workspace member without a name or version');
  }

  const versions = new Map();
  for (const packageMetadata of members) {
    const packageNames = versions.get(packageMetadata.version) || [];
    packageNames.push(packageMetadata.name);
    versions.set(packageMetadata.version, packageNames);
  }
  if (versions.size !== 1) {
    const inventory = [...versions]
      .map(([version, packageNames]) => `${version}: ${packageNames.sort().join(', ')}`)
      .sort()
      .join('; ');
    fail(`Cargo workspace package versions are inconsistent (${inventory})`);
  }
  return versions.keys().next().value;
}

function archiveName(name, version) {
  return `${name.replace(/^@/, '').replace('/', '-')}-${version}.tgz`;
}

function runPack(manager, directory, dist, manifest) {
  const archive = join(dist, archiveName(manifest.name, manifest.version));
  rmSync(archive, { force: true });
  // On Windows npm resolves to npm.cmd, which spawnSync can only execute
  // through a shell; shell mode does not quote arguments, so quote them here.
  const windows = process.platform === 'win32';
  const command = manager === 'npm' ? 'npm' : 'bun';
  const args = manager === 'npm'
    ? ['pack', '--ignore-scripts', '--pack-destination', dist]
    : ['pm', 'pack', '--ignore-scripts', '--quiet', '--destination', dist];
  const result = spawnSync(
    windows ? `${command}.cmd` : command,
    windows ? args.map((arg) => (/\s/.test(arg) ? `"${arg}"` : arg)) : args,
    {
      cwd: directory,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      shell: windows
    }
  );
  if (result.status !== 0) {
    process.stderr.write(result.stdout || '');
    process.stderr.write(result.stderr || '');
    fail(`${manager} failed to pack ${manifest.name}`);
  }
  if (!existsSync(archive)) {
    fail(`${manager} did not create ${archive}`);
  }
  process.stdout.write(`${archive}\n`);
}

function assertManager(manager) {
  if (manager !== 'npm' && manager !== 'bun') {
    fail(`unsupported package manager ${manager}; expected npm or bun`);
  }
}

function assertVersion(manifest, version) {
  if (manifest.version !== version) {
    fail(`${manifest.name} version ${manifest.version} does not match Cargo ${version}`);
  }
}

function packageNative(options, manager, dist, version) {
  const targetName = options.target;
  if (!targetName) {
    fail('native requires --target <rust-target>');
  }
  const targets = readJson(join(NPM_ROOT, 'targets.json')).targets;
  const target = targets.find((item) => item.rustTarget === targetName);
  if (!target) {
    fail(`unsupported Rust target ${targetName}`);
  }
  const binary = resolve(
    options.binary || join(ROOT, 'target', target.rustTarget, 'release', target.executable)
  );
  if (!existsSync(binary)) {
    fail(`native binary is missing: ${binary}`);
  }

  const stagingRoot = mkdtempSync(join(tmpdir(), 'rsproxy-npm-native-'));
  const staging = join(stagingRoot, 'package');
  try {
    mkdirSync(join(staging, 'bin'), { recursive: true });
    const packagedBinary = join(staging, 'bin', target.executable);
    copyFileSync(binary, packagedBinary);
    chmodSync(packagedBinary, 0o755);
    copyFileSync(join(ROOT, 'LICENSE'), join(staging, 'LICENSE'));
    writeFileSync(
      join(staging, 'README.md'),
      `# ${target.package}\n\nNative rsproxy executable for ${target.rustTarget}.\n\n`
      + 'This package is installed automatically by @rsproxy/runtime.\n'
    );
    const manifest = {
      name: target.package,
      version,
      description: `Native rsproxy executable for ${target.rustTarget}`,
      license: 'MIT',
      os: [target.platform],
      cpu: [target.arch],
      files: ['bin', 'README.md', 'LICENSE'],
      publishConfig: { access: 'public' },
      rsproxy: { rustTarget: target.rustTarget }
    };
    if (target.libc) {
      manifest.libc = [target.libc];
    }
    writeFileSync(join(staging, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
    runPack(manager, staging, dist, manifest);
  } finally {
    rmSync(stagingRoot, { recursive: true, force: true });
  }
}

function packageRuntimeAndLauncher(manager, dist, version) {
  for (const directory of ['runtime', 'cli']) {
    const packageRoot = join(NPM_ROOT, directory);
    const manifest = readJson(join(packageRoot, 'package.json'));
    assertVersion(manifest, version);
    runPack(manager, packageRoot, dist, manifest);
  }
}

const [command, ...rawOptions] = process.argv.slice(2);
if (command !== 'native' && command !== 'launchers') {
  fail('usage: package.mjs <native|launchers> [--target TARGET] [--binary PATH] [--manager npm|bun] [--dist PATH]');
}
const options = parseOptions(rawOptions);
const manager = options.manager || 'npm';
assertManager(manager);
const dist = resolve(options.dist || join(ROOT, 'dist', 'npm'));
mkdirSync(dist, { recursive: true });
const version = workspaceVersion();

if (command === 'native') {
  packageNative(options, manager, dist, version);
} else {
  packageRuntimeAndLauncher(manager, dist, version);
}
