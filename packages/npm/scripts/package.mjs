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
  const cargo = readFileSync(join(ROOT, 'Cargo.toml'), 'utf8');
  const section = cargo.match(/\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/);
  const version = section && section[1].match(/^version\s*=\s*"([^"]+)"/m);
  if (!version) {
    fail('Cargo workspace version is missing');
  }
  return version[1];
}

function archiveName(name, version) {
  return `${name.replace(/^@/, '').replace('/', '-')}-${version}.tgz`;
}

function runPack(manager, directory, dist, manifest) {
  const archive = join(dist, archiveName(manifest.name, manifest.version));
  rmSync(archive, { force: true });
  const command = manager === 'npm' ? 'npm' : 'bun';
  const args = manager === 'npm'
    ? ['pack', '--ignore-scripts', '--pack-destination', dist]
    : ['pm', 'pack', '--ignore-scripts', '--quiet', '--destination', dist];
  const result = spawnSync(command, args, {
    cwd: directory,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe']
  });
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

function packageLaunchers(manager, dist, version) {
  for (const directory of ['runtime', 'cli', 'bun']) {
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
  packageLaunchers(manager, dist, version);
}
