# Installer test fixtures

`scripts/tests/installer_test.sh` creates tiny release archives and matching
checksum manifests in a temporary directory at runtime. The generated files use
the same names as GitHub Release assets, but they are never committed and the
test never contacts the network.

The test also replaces `uname` and `curl` through a temporary `PATH`, isolates
`HOME` and the install directory, and verifies that configuration and existing
binaries are preserved on failure.
