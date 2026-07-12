'use strict';

const { spawnSync } = require('node:child_process');

const PACKAGE_BY_PLATFORM = Object.freeze({
  'darwin:arm64': '@rsproxy/darwin-arm64',
  'darwin:x64': '@rsproxy/darwin-x64',
  'linux:arm64:gnu': '@rsproxy/linux-arm64-gnu',
  'linux:arm64:musl': '@rsproxy/linux-arm64-musl',
  'linux:x64:gnu': '@rsproxy/linux-x64-gnu',
  'linux:x64:musl': '@rsproxy/linux-x64-musl',
  'win32:arm64': '@rsproxy/win32-arm64-msvc',
  'win32:x64': '@rsproxy/win32-x64-msvc'
});

function normalizeLibc(value) {
  if (value === 'gnu' || value === 'glibc') {
    return 'gnu';
  }
  if (value === 'musl') {
    return 'musl';
  }
  return undefined;
}

function detectLinuxLibc(options = {}) {
  const env = options.env || process.env;
  const override = env.RSPROXY_LIBC;
  if (override) {
    const normalized = normalizeLibc(override);
    if (!normalized) {
      throw new Error('RSPROXY_LIBC must be one of gnu, glibc, or musl');
    }
    return normalized;
  }

  const versions = options.versions || process.versions || {};
  if (versions.musl) {
    return 'musl';
  }

  const report = options.report === undefined ? process.report : options.report;
  try {
    const snapshot = report && typeof report.getReport === 'function'
      ? report.getReport()
      : undefined;
    if (snapshot && snapshot.header && snapshot.header.glibcVersionRuntime) {
      return 'gnu';
    }
    const sharedObjects = snapshot && Array.isArray(snapshot.sharedObjects)
      ? snapshot.sharedObjects.join(' ').toLowerCase()
      : '';
    if (sharedObjects.includes('musl')) {
      return 'musl';
    }
  } catch (_error) {
    // Runtime reports are optional in Bun and in restricted Node environments.
  }

  const spawn = options.spawn || spawnSync;
  try {
    const probe = spawn('ldd', ['--version'], { encoding: 'utf8' });
    const output = `${probe.stdout || ''}\n${probe.stderr || ''}`.toLowerCase();
    if (output.includes('musl')) {
      return 'musl';
    }
    if (output.includes('glibc') || output.includes('gnu libc')) {
      return 'gnu';
    }
  } catch (_error) {
    // GNU is the conservative default when no libc signal is available.
  }
  return 'gnu';
}

function packageFor(options = {}) {
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const libc = platform === 'linux'
    ? normalizeLibc(options.libc) || detectLinuxLibc(options)
    : undefined;
  const key = libc ? `${platform}:${arch}:${libc}` : `${platform}:${arch}`;
  const packageName = PACKAGE_BY_PLATFORM[key];
  if (!packageName) {
    throw new Error(
      `Unsupported rsproxy platform: platform=${platform}, arch=${arch}`
      + (libc ? `, libc=${libc}` : '')
    );
  }
  return packageName;
}

function resolveBinary(options = {}) {
  const platform = options.platform || process.platform;
  const packageName = packageFor(options);
  const executable = platform === 'win32' ? 'rsproxy.exe' : 'rsproxy';
  const resolver = options.resolver || require.resolve;
  return {
    packageName,
    path: resolver(`${packageName}/bin/${executable}`)
  };
}

module.exports = {
  PACKAGE_BY_PLATFORM,
  detectLinuxLibc,
  normalizeLibc,
  packageFor,
  resolveBinary
};
