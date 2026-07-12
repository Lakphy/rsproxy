#!/bin/sh
exec "$(dirname -- "$0")/targets.sh" coverage "$@"
