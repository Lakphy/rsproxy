'use strict';

const { spawnSync } = require('node:child_process');
const { constants } = require('node:os');
const { resolveBinary } = require('./platform');

function signalExitCode(signal) {
  const number = constants.signals[signal];
  return typeof number === 'number' ? 128 + number : 1;
}

function run(argv = process.argv.slice(2), options = {}) {
  let native;
  try {
    native = resolveBinary(options);
  } catch (error) {
    const detail = error && error.message ? error.message : String(error);
    process.stderr.write(
      `rsproxy could not resolve its native executable: ${detail}\n`
      + 'Reinstall with optional dependencies enabled using either:\n'
      + '  npm install --global @rsproxy/cli\n'
      + '  bun add --global @rsproxy/bun\n'
    );
    return 1;
  }

  const spawn = options.spawn || spawnSync;
  const result = spawn(native.path, argv, { stdio: 'inherit' });
  if (result.error) {
    process.stderr.write(
      `rsproxy could not start ${native.packageName}: ${result.error.message}\n`
    );
    return 1;
  }
  if (result.signal) {
    return signalExitCode(result.signal);
  }
  return typeof result.status === 'number' ? result.status : 1;
}

module.exports = { run, signalExitCode };
