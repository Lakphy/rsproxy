#!/usr/bin/env node
'use strict';

const { run } = require('@rsproxy/runtime');

process.exitCode = run();
