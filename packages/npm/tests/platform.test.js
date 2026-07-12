'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const {
  detectLinuxLibc,
  packageFor,
  resolveBinary
} = require('../runtime/lib/platform');

const mappings = [
  ['darwin', 'arm64', undefined, '@rsproxy/darwin-arm64'],
  ['darwin', 'x64', undefined, '@rsproxy/darwin-x64'],
  ['linux', 'arm64', 'gnu', '@rsproxy/linux-arm64-gnu'],
  ['linux', 'arm64', 'musl', '@rsproxy/linux-arm64-musl'],
  ['linux', 'x64', 'gnu', '@rsproxy/linux-x64-gnu'],
  ['linux', 'x64', 'musl', '@rsproxy/linux-x64-musl'],
  ['win32', 'arm64', undefined, '@rsproxy/win32-arm64-msvc'],
  ['win32', 'x64', undefined, '@rsproxy/win32-x64-msvc']
];

test('maps every supported platform to one native package', () => {
  for (const [platform, arch, libc, expected] of mappings) {
    assert.equal(packageFor({ platform, arch, libc }), expected);
  }
});

test('rejects unsupported operating systems and architectures', () => {
  assert.throws(
    () => packageFor({ platform: 'freebsd', arch: 'x64' }),
    /Unsupported rsproxy platform/
  );
  assert.throws(
    () => packageFor({ platform: 'darwin', arch: 'ia32' }),
    /Unsupported rsproxy platform/
  );
});

test('detects Linux libc from override, runtime report, and ldd', () => {
  assert.equal(detectLinuxLibc({ env: { RSPROXY_LIBC: 'glibc' } }), 'gnu');
  assert.equal(detectLinuxLibc({ env: { RSPROXY_LIBC: 'musl' } }), 'musl');
  assert.throws(
    () => detectLinuxLibc({ env: { RSPROXY_LIBC: 'other' } }),
    /RSPROXY_LIBC/
  );
  assert.equal(detectLinuxLibc({
    env: {},
    versions: {},
    report: { getReport: () => ({ header: { glibcVersionRuntime: '2.39' } }) }
  }), 'gnu');
  assert.equal(detectLinuxLibc({
    env: {},
    versions: {},
    report: undefined,
    spawn: () => ({ stdout: '', stderr: 'musl libc' })
  }), 'musl');
});

test('resolves the platform-specific executable name', () => {
  const unix = resolveBinary({
    platform: 'darwin',
    arch: 'arm64',
    resolver: (specifier) => `/resolved/${specifier}`
  });
  assert.deepEqual(unix, {
    packageName: '@rsproxy/darwin-arm64',
    path: '/resolved/@rsproxy/darwin-arm64/bin/rsproxy'
  });
  const windows = resolveBinary({
    platform: 'win32',
    arch: 'x64',
    resolver: (specifier) => `/resolved/${specifier}`
  });
  assert.equal(windows.path, '/resolved/@rsproxy/win32-x64-msvc/bin/rsproxy.exe');
});
