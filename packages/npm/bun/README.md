# @rsproxy/bun

Bun-native launcher for rsproxy, a programmable HTTP and HTTPS debugging proxy.

```sh
bun add --global @rsproxy/bun
rsproxy --version
```

The package installs one native optional dependency for the current operating
system, architecture, and Linux libc. Installation never compiles Rust and does
not run lifecycle scripts. Node/npm users should install `@rsproxy/cli`.
