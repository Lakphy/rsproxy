'use strict';

const childProcess = require('node:child_process');
const { constants } = require('node:os');
const { resolveBinary } = require('./platform');

const FORWARDED_SIGNALS = ['SIGTERM', 'SIGINT', 'SIGHUP', 'SIGQUIT'];

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
      + '  bun add --global @rsproxy/cli\n'
    );
    return Promise.resolve(1);
  }

  const spawn = options.spawn || childProcess.spawn;
  // Injectable for tests so signal wiring never touches the real process object.
  const supervisor = options.process || process;

  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(native.path, argv, {
        stdio: 'inherit',
        env: { ...supervisor.env, RSPROXY_SUPERVISOR_PID: String(supervisor.pid) }
      });
    } catch (error) {
      process.stderr.write(
        `rsproxy could not start ${native.packageName}: ${error.message}\n`
      );
      resolve(1);
      return;
    }

    // Async spawn keeps the event loop running, so — unlike spawnSync — the shim
    // can relay signals to the native child instead of dying and orphaning it.
    const forwarders = FORWARDED_SIGNALS.map((signal) => {
      const handler = () => {
        try {
          child.kill(signal);
        } catch {
          // Child already gone; nothing to forward.
        }
      };
      supervisor.on(signal, handler);
      return [signal, handler];
    });
    const cleanup = () => {
      for (const [signal, handler] of forwarders) {
        supervisor.removeListener(signal, handler);
      }
    };

    child.on('error', (error) => {
      cleanup();
      process.stderr.write(
        `rsproxy could not start ${native.packageName}: ${error.message}\n`
      );
      resolve(1);
    });
    child.on('exit', (code, signal) => {
      cleanup();
      if (signal) {
        resolve(signalExitCode(signal));
        return;
      }
      resolve(typeof code === 'number' ? code : 1);
    });
  });
}

module.exports = { run, signalExitCode };
