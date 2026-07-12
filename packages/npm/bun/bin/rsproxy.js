#!/usr/bin/env bun
'use strict';

const { run } = require('@rsproxy/runtime');

process.exitCode = run();
