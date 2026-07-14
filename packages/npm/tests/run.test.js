'use strict';

const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');
const { constants } = require('node:os');
const test = require('node:test');
const { run, signalExitCode } = require('../runtime/lib/run');

function fakeChild() {
  const child = new EventEmitter();
  child.signals = [];
  child.kill = (signal) => {
    child.signals.push(signal);
    return true;
  };
  return child;
}

function fakeSupervisor() {
  const supervisor = new EventEmitter();
  supervisor.pid = 4242;
  supervisor.env = { EXISTING: 'kept' };
  return supervisor;
}

function baseOptions(overrides = {}) {
  return {
    platform: 'darwin',
    arch: 'arm64',
    resolver: () => '/fake/@rsproxy/darwin-arm64/bin/rsproxy',
    ...overrides
  };
}

test('forwards signals to the child and reports the signalled exit code', async () => {
  const child = fakeChild();
  const supervisor = fakeSupervisor();
  let spawnOptions;
  const options = baseOptions({
    process: supervisor,
    spawn: (path, argv, opts) => {
      spawnOptions = opts;
      return child;
    }
  });

  const pending = run(['run'], options);

  // The supervisor PID and inherited env reach the native child.
  assert.equal(spawnOptions.env.RSPROXY_SUPERVISOR_PID, '4242');
  assert.equal(spawnOptions.env.EXISTING, 'kept');
  assert.equal(spawnOptions.stdio, 'inherit');

  // A signal delivered to the shim alone is relayed to the child.
  supervisor.emit('SIGTERM');
  assert.deepEqual(child.signals, ['SIGTERM']);

  child.emit('exit', null, 'SIGTERM');
  assert.equal(await pending, signalExitCode('SIGTERM'));
  assert.equal(await pending, 128 + constants.signals.SIGTERM);

  // Handlers are removed after exit so no further forwarding happens.
  supervisor.emit('SIGINT');
  assert.deepEqual(child.signals, ['SIGTERM']);
  assert.equal(supervisor.listenerCount('SIGTERM'), 0);
});

test('propagates a normal child exit status', async () => {
  const child = fakeChild();
  const options = baseOptions({
    process: fakeSupervisor(),
    spawn: () => child
  });
  const pending = run(['status'], options);
  child.emit('exit', 7, null);
  assert.equal(await pending, 7);
});

test('reports failure to spawn the native child', async () => {
  const child = fakeChild();
  const options = baseOptions({
    process: fakeSupervisor(),
    spawn: () => child
  });
  const pending = run(['run'], options);
  child.emit('error', new Error('boom'));
  assert.equal(await pending, 1);
});

test('returns 1 when the platform is unsupported', async () => {
  const code = await run(['run'], baseOptions({ platform: 'freebsd', arch: 'x64' }));
  assert.equal(code, 1);
});

test('returns 1 when spawn throws synchronously', async () => {
  const options = baseOptions({
    process: fakeSupervisor(),
    spawn: () => {
      throw new Error('spawn failed');
    }
  });
  assert.equal(await run(['run'], options), 1);
});
