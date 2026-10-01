# Releasing

## CI

The crate links the closed-source Noesis SDK, so only a self-hosted runner with
the SDK (label `noesis-sdk`) can build it.

- **`fmt`** and **`doc`** run on hosted runners for every push and PR,
  including forks. `doc` sets `DOCS_RS=1`, which makes `build.rs` skip the
  native build.
- **`build • clippy (no shim)`** also runs on hosted runners. Without the
  `shim` feature the crate needs no SDK.
- **`build • clippy • test`** runs on the SDK runner for pushes to `main`,
  tags, and same-repo PRs. Fork PRs are skipped.

`ci.yml` and `release.yml` both check the runner's SDK version against the
pinned one (`3.2.13 (r17073)`). Update both pins when upgrading the SDK.

## Cutting a release

Requires [cargo-release](https://github.com/crate-ci/cargo-release). With
`main` clean and CI green:

```sh
cargo release minor --dry-run
cargo release minor --execute
```

It bumps the version, stamps `CHANGELOG.md`, commits, tags `vX.Y.Z`, and
pushes. The tag triggers `release.yml` on the SDK runner, which tests and
publishes through crates.io Trusted Publishing.
Afterward, check the crate page and the docs.rs build.

Keep `## [Unreleased]` in `CHANGELOG.md` current as PRs land.
