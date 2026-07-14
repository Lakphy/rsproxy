#!/usr/bin/env node
'use strict';

const { run } = require('@rsproxy/runtime');

run().then((code) => {
  process.exitCode = code;
});
