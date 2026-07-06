# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository overview

Strand-Braid is a Rust Cargo workspace (`resolver = "3"`) housing two related
applications plus the many crates they're built from:

- **Strand Camera** (`strand-cam/`) — single-camera acquisition, realtime
  feature detection, and recording.
- **Braid** (`braid/braid-run`, binary `braid-run`) — multi-camera realtime 3D
  tracking. At startup it spawns one `strand-cam` child process per configured
  camera and coordinates them over HTTP.

Top-level directories (`braid/`, `braidz/`, `camera/`, `geometry/`,
`im-proc/`, `led-box/`, `media-utils/`, `strand-cam/`, `utils/`, `web/`) each
group related crates and each crate has its own `README.md`. A per-crate
description table is auto-generated into
`docs/developer-docs/repository-organization.md` (included in the root
`README.md`) from `.workspace-docs.toml` — regenerate it with
`cargo run -p workspace-docs` (see below) whenever workspace members change.

## Common commands

```sh
cargo check --workspace --all-targets        # compile check, all crates
cargo test --release --workspace --all-targets   # run tests (use --release; some crates are slow otherwise)
cargo test -p <crate-name> [<test_name>]     # run a single crate's tests / a single test
cargo clippy --all-targets --workspace -- -D warnings
cargo fmt                                    # cargo fmt --check is the CI gate
cargo run -p workspace-docs -- --check       # verify generated repo-organization docs are up to date
cargo run -p license-check                   # verify SPDX headers on .rs files (add --fix to insert missing ones)
```

Building the two main binaries requires [`trunk`](https://trunk-rs.github.io/trunk/)
and the `wasm32-unknown-unknown` target, because their browser UIs
(`strand-cam/yew_frontend`, `braid/braid-run/braid_frontend`) are Yew/WASM
apps compiled by `trunk` and embedded into the native binary by `build.rs`:

```sh
cd strand-cam && cargo build --release --bin strand-cam
cd braid/braid-run && cargo build --release
```

Vendor camera drivers (Basler Pylon, Allied Vision Vimba) are proprietary and
loaded dynamically at runtime (`--camera-backend pylon|vimba`); they are not
needed to build. A `--camera-backend webcam` backend and a `ci2-sim` backend
(synthetic images) work with no vendor SDK at all — see
`docs/developer-docs/testing-with-emulated-cameras.md` and
`smoke-tests/braid-camemu.sh` for end-to-end smoke testing without camera
hardware, and `braid/braid-sim` for a fully synthetic simulation harness used
to test Braid's 3D tracking in isolation.

### Before declaring a task done

CI enforces all of the following; run them locally before finishing (per
`AGENTS.md`, which also has more detail on release/license-bundling
requirements):

1. `cargo fmt`
2. `cargo check --workspace`
3. `cargo clippy --all-targets --workspace -- -D warnings`
4. `cargo run -p workspace-docs -- --check`
5. `cargo run -p license-check`

## Architecture

**Data flow.** `braid-run`'s "mainbrain" (`braid/braid-run/src/mainbrain.rs`)
runs an axum HTTP server. On startup (`braid/braid-run/src/main.rs`) it spawns
one `strand-cam` subprocess per camera listed in the config file, passing
`--braid-url` so each `strand-cam` connects back to mainbrain for remote
control and streams its detected 2D points to it. `flydra2`
(`braid/flydra2`) is the tracking engine: it fuses the per-camera 2D point
streams into 3D trajectories via Kalman filtering (models in `braid/tracking`)
and multi-view geometry (`geometry/braid-mvg`, `geometry/flydra-mvg`), then
writes results as `.braidz` files through `braid/braidz-writer`. `.braidz` is
the canonical output format; its schema is documented in
`braid/braid-types/braidz-schema.md`.

**Camera abstraction.** `camera/ci2` defines the `CameraModule`/`Camera` trait
abstraction that every vendor backend implements (`ci2-pylon`, `ci2-vimba`,
`ci2-webcam`, `ci2-sim`); `camera/ci2-async` wraps it for async use inside
`strand-cam`. `ci2-sim` renders synthetic images of simulated insects and,
paired with `braid-sim`, enables fully offline end-to-end testing of the
tracking pipeline.

**`.braidz` tooling.** Everything under `braidz/` operates on the format after
the fact: `braidz-parser` reads it, `braidz-cli`/`braidz-viewer` inspect it,
`braidz-rerun`/`braidz-export-rrd`/`rerun-braidz-viewer` visualize it via
[rerun](https://rerun.io), and `flytrax-csv-to-braidz` imports legacy Flytrax
CSV data into the format.

**Geometry/calibration.** `geometry/braid-mvg` holds the core camera geometry
and multi-view-geometry types used throughout. Calibration is produced by
`geometry/braid-april-cal` (AprilTag-based, SQPnP), `geometry/checkerboard-calibrate`
/ `geometry/camcal` (checkerboard-based), and `geometry/mcsc-native` /
`geometry/bundle-adj` (multi-camera self-calibration / bundle adjustment).

**Feature flags matter.** `strand-cam` and several other crates gate
significant functionality behind Cargo features (e.g. `flydra_feat_detect`,
`fiducial`, `checkercal`, `flydratrax`, `bundle_files`, `with-rerun`) — check
the relevant `Cargo.toml` `[features]` section before assuming a code path is
compiled in; CI builds several different feature combinations (see
`.gitlab-ci.yml`).

**Shared dependency versions.** The root `Cargo.toml`'s
`[workspace.dependencies]` is the single source of truth for both third-party
crate versions and in-repo path dependencies; leaf crates should reference
`foo.workspace = true` rather than pinning their own versions. The workspace
release version also lives there (`[workspace.package] version`), inherited
via `version.workspace = true`.

## Style conventions (see `AGENTS.md` for full detail)

- Use `tracing`, not `log`, for instrumentation.
- Prefer captured identifiers in format strings: `println!("{variable}")` over
  `println!("{}", variable)`.
- When removing functionality, delete it outright rather than commenting it
  out.
- Every `.rs` file needs an SPDX header (checked by `license-check`).
- New crates need a `README.md`; any crate scripts must be documented.
- Keep in-repo crate references separated by a blank line from crates.io
  dependencies, both in `Cargo.toml` and in `use` statements.
