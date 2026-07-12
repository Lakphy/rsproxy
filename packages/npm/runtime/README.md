# @rsproxy/runtime

Internal runtime package for the rsproxy npm and Bun launchers. It resolves the
matching native optional dependency and forwards arguments, standard streams,
signals, and exit status to the Rust executable.

Install `@rsproxy/cli` with npm or `@rsproxy/bun` with Bun instead of depending
on this package directly.
