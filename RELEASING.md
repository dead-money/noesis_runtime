# Releasing

## CI

The crate links the closed-source Noesis SDK, so only a self-hosted runner with
the SDK can build it with the `shim` feature.

- **`fmt`** and **`doc`** run on hosted runners for every push and PR,
  including forks. `doc` sets `DOCS_RS=1`, which makes `build.rs` skip the
  native build.
- **`build • clippy (no shim)`** also runs on hosted runners. Without the
  `shim` feature the crate needs no SDK.

The SDK jobs run in the private
[`dead-money/noesis-ci`](https://github.com/dead-money/noesis-ci) repo on the
self-hosted runner (label `noesis-sdk`), which takes no jobs from public repos.
Every 15 minutes it polls this repo and runs all-features clippy and
tests on each new `main` commit, and the release for each new `v*` tag. Results
show in that repo's Actions tab, not as checks here, so PRs merge on the hosted
checks alone.

The noesis-ci workflows select and validate the SDK from
`noesis-sdk-version.txt`. Update that pin and install the matching SDK on the
runner when upgrading. Older tags without a pin keep the 3.2.13 default.

## Cutting a release

Requires [cargo-release](https://github.com/crate-ci/cargo-release). With
`main` clean and CI green:

```sh
cargo release minor --dry-run
cargo release minor --execute
```

It bumps the version, stamps `CHANGELOG.md`, commits, tags `vX.Y.Z`, and
pushes. Within 15 minutes noesis-ci notices the tag and runs its `release.yml`
on the SDK runner, which tests and publishes through crates.io Trusted Publishing.
Afterward, check the crate page and the docs.rs build.

Keep `## [Unreleased]` in `CHANGELOG.md` current as PRs land.
