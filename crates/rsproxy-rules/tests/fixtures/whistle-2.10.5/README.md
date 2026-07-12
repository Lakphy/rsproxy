# Whistle 2.10.5 evidence fixture

This directory is a minimal, immutable source-evidence snapshot used only by
the rsproxy rule migration and option-classification contract tests. It is not
an executable Whistle checkout and is not part of the rsproxy runtime.

The files were copied without modification from Whistle v2.10.5 at commit
`0b4c4bdb78ff5c53ffcb5a823ca9b53d7e6269c4`. `SHA256SUMS` covers the 75
evidence files and the upstream MIT `LICENSE` file.

When the pinned comparison version changes, regenerate the snapshot from a
separate upstream checkout, update the contract matrices, metadata, hashes,
and the pinned benchmark package together.
