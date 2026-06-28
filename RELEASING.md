# Releasing

How `noesis_runtime` gets published to crates.io.

## The constraint that shapes everything

The crate links the closed-source Noesis SDK at build time, so it can only be
built where the SDK is present. We run a **self-hosted GitHub Actions runner**
(label `noesis-sdk`) that has the SDK installed at `/opt/noesis-sdk`
(`NOESIS_SDK_DIR` is set in the runner's `.env`). That runner does the real
build, clippy, test, and the verified publish. GitHub-hosted runners only run
`cargo fmt` (no SDK needed), and never run fork-PR code against the SDK box.

## CI

- **`fmt`** (hosted) runs on every push and PR, including forks.
- **`build • clippy • test`** (self-hosted) runs on pushes to `main`, version
  tags, and same-repo PRs. Fork PRs are skipped so untrusted code never touches
  the SDK runner.

`cargo test --all-features` exercises the full suite, including the
`test-utils` render-device test (it drives a `MockDevice`, so no GPU is needed).

## One-time setup

1. **Install cargo-release** locally: `cargo install cargo-release`.
2. **Configure crates.io Trusted Publishing.** On crates.io → the crate →
   Settings → Trusted Publishing, add a GitHub publisher:
   - Repository: `dead-money/noesis_runtime`
   - Workflow filename: `release.yml`
   - Environment: leave blank.

   Trusted Publishing uses GitHub's OIDC identity, so there is **no API token to
   store**. The `id-token: write` permission in `release.yml` lets the runner
   mint a short-lived token at publish time.
3. **First publish claims the name.** Trusted Publishing can be configured before
   the crate exists; the first release can go straight through CI, or you can run
   one manual `cargo publish` from a machine with the SDK to claim the name.

## Cutting a release

With `main` clean and CI green:

> **First release (0.9.0):** the version is already set in `Cargo.toml` and the
> `0.9.0` section of `CHANGELOG.md` is already filled, so do NOT run
> `cargo release` for it — that would re-bump the version and duplicate the
> changelog heading. Just tag the current commit and push the tag:
>
> ```sh
> git tag v0.9.0 && git push origin v0.9.0
> ```

Use `cargo release` for **subsequent** releases:

```sh
cargo release 1.0.0        # or: patch | minor | major
```

`cargo release` (config in `release.toml`) bumps the version in `Cargo.toml`,
stamps `CHANGELOG.md` (move notes out of `[Unreleased]`), commits, tags
`v0.9.0`, and pushes. The pushed tag triggers `.github/workflows/release.yml` on
the self-hosted runner, which runs the test suite, authenticates via Trusted
Publishing, and runs `cargo publish` — a full verification build against the SDK,
then upload.

Do a dry run first to see exactly what it will do:

```sh
cargo release 0.9.0 --dry-run
```

After it lands, confirm the crate on crates.io and that docs.rs built (it builds
without the SDK because `build.rs` short-circuits on the `DOCS_RS` env var).

## The self-hosted runner

Provisioned on an Ubuntu droplet:

- Runs as the unprivileged `runner` user (the Actions runner refuses root).
- SDK at `/opt/noesis-sdk`; `NOESIS_SDK_DIR` and a cargo-bin `PATH` are set in
  `~/actions-runner/.env`.
- Installed as a systemd service
  (`actions.runner.dead-money-noesis_runtime.noesis-ci-droplet`), so it survives
  reboots.
- `target/` is kept between runs (`clean: false` on checkout) for fast
  incremental builds.

To re-register after a token/repo change, on the droplet as `runner`:
`cd ~/actions-runner && ./config.sh remove --token <token>` then re-run
`./config.sh` with a fresh registration token.
