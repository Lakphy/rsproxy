# @rsproxy/cli

Shared npm/Bun launcher for rsproxy, a programmable HTTP and HTTPS debugging
proxy.

```sh
npm install --global @rsproxy/cli
# or
bun add --global @rsproxy/cli
rsproxy --version
```

The package installs one native optional dependency for the current operating
system, architecture, and Linux libc. Installation never compiles Rust and does
not run lifecycle scripts. The installed command uses Node 18+; Bun-only users
can execute the same package with `bunx --bun @rsproxy/cli`.
