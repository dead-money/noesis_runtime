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
tests on each new `main` commit, and validates each new `v*` tag. Results
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
pushes. The private `dead-money/noesis-ci` poller validates the tag on the SDK runner.
Run its `poll.yml` manually to start immediately. After that run succeeds,
confirm the exact tag SHA and dispatch this repository's `release.yml`:

```sh
tag=vX.Y.Z
sha=$(git rev-parse "$tag^{commit}")
gh workflow run release.yml --repo dead-money/noesis_runtime --ref main \
  -f tag="$tag" -f sha="$sha" \
  -f sdk_run="https://github.com/dead-money/noesis-ci/actions/runs/<successful-run-id>"
```

The release operator must check the private run succeeded for this exact source;
the public workflow records that URL and checks tag identity, but cannot read
private CI results itself. Publishing runs on a hosted runner with `--no-verify`
because the native build and tests already passed on the SDK runner. The trusted
publisher is `dead-money/noesis_runtime`, workflow `release.yml`, no environment.
Afterward, check the crate pages and docs.rs builds.

Keep `## [Unreleased]` in `CHANGELOG.md` current as PRs land.
