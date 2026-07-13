# @rsproxy/runtime

Internal runtime package for the shared rsproxy npm/Bun launcher. It resolves
the matching native optional dependency and forwards arguments, standard
streams, signals, and exit status to the Rust executable.

Install `@rsproxy/cli` with npm or Bun instead of depending on this package
directly.
