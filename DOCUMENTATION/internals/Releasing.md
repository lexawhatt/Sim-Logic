# Preparing a release

[Documentation](../README.md) / Internals

The current development version is **0.2.0-dev.1**, using local
**sim-engine = 0.5.0-dev.1**. The target final versions are **Logic 0.2.0** and
**Engine 0.5.0**. Neither the development version string nor a successful local
build establishes registry publication. Publication remains a separate approved
step; qualification commands below do not upload or tag a release.

## Engine and consumer release gates

The Engine handoff reports the layered-rendering and exact text-cluster geometry
APIs implemented and checked. Its last text-contract review and full release run
on the final Engine commit remain pending, followed by Engine publication.
There is no confirmed Engine defect in the desktop integration request.

Logic's standard desktop layered bridge is already implemented: assignment,
resolution/effects, target lifecycle, input/UI preservation and diagnostics.
Chaos;Chess acceptance must exercise that bridge; consumer acceptance is still
open. Local Logic/Engine checkouts are the supported development route meanwhile.
Consumers pinned to `=0.1.0` must explicitly adopt `=0.2.0-dev.1` and refresh
their lockfiles. This preparation does not edit consumer manifests.

After Engine 0.5.0 is published:

1. Replace Logic's Engine dependency with `version = "=0.5.0"` and remove `path`.
   Resolve and inspect the lockfile against the registry artifact. Check direct
   consumer Engine dependencies for the same type/source identity.
2. Set Logic to 0.2.0 when its final scope and acceptance are complete; update
   the changelog and installation examples. Keep unimplemented mutable screen
   textures and live diagnostic history outside the advertised release scope.
3. Run the full local gate, MSRV and applicable native/consumer acceptance on
   the final candidate. Recheck the registry-backed package and clean-commit
   `cargo publish --dry-run --locked --all-features`.
4. Publish/tag only after explicit release approval. Re-run affected checks if
   the final dependency artifact or source changes after qualification.

While Engine is local and unpublished, package/archive and publish dry-run gates
remain open. `scripts/check-release.sh` includes registry-backed packaging and
is not expected to qualify this development dependency graph end to end. Use
individual source checks for development and report their limited scope.

## Package and documentation

`Cargo.toml` has an explicit `include` list: library sources, example/test
sources and their licensed assets, benchmarks, public documentation, release
scripts, manifest/lockfile, README, changelog and root licenses.
Cargo adds its normal generated package metadata. Inspect the actual list;
an allowlist is not a substitute for reviewing new files under those paths.

Private working notes, IDE settings, saved games and `target` output must not
enter the archive. Do not delete user saves or local IDE settings as cleanup.
Old compiled probe binaries are rebuildable; their source, measurements and
screenshots are evidence and should be kept separately.

Before publishing, check that `Cargo.toml`, the lockfile, the dated changelog
entry and the installation examples all refer to the intended release.
Verify the crate name is available to this account; a network failure is not
proof of availability. Keep the MIT/Apache licenses and all bundled font/Ferris
notices in the package.

## Repeatable local checks

From the repository root, with Python 3, Rustfmt and Clippy installed:

```bash
bash scripts/check-release.sh
```

The script checks public Markdown links, the exact README/getting-started
programs, formatting, tests, lints, feature boundaries, rustdoc and the unpacked
package, including an archive check for required licenses/assets and private
paths. It uses the current toolchain, not an implicit MSRV claim. Network is
allowed for Cargo dependencies; set `CARGO_NET_OFFLINE=true` only when the
required registry sources are already cached. All-feature checks require the
Linux audio build prerequisites described in [Audio output](../guides/Audio-Output.md).

Run the minimum supported Rust version separately:

```bash
rustup toolchain install 1.95.0 --profile minimal
cargo +1.95.0 check --locked --all-features --all-targets
```

Do not call a Rust 1.96 result an MSRV check. The docs.rs configuration enables
all features and builds the Linux target. Local warning-free rustdoc checks
the API reference; an actual docs.rs build is only confirmed after publication.

Check [build storage](Code-Layout.md#build-storage) before a large matrix.
Avoid concurrent Cargo matrices and separate duplicated target directories.
The script never deletes artifacts or private evidence automatically.

## Native acceptance is separate

Ordinary test runs skip tests requiring a real window/Vulkan device. A passing
headless/package check does not mean these passed. On an isolated Linux display,
run the ignored resource tests one at a time to avoid competing event loops:

```bash
cargo test --locked --all-features --test engine_dev4_gpu -- \
  --ignored --nocapture --test-threads=1
cargo test --locked --all-features --lib managed_mesh_texture_revisions_and_recovery -- \
  --ignored --nocapture --test-threads=1
cargo test --locked --all-features --lib layered_bridge_capture_resize_and_recovery -- \
  --ignored --nocapture --test-threads=1
cargo test --locked --all-features --lib retained_screen_gpu_resources -- \
  --ignored --nocapture --test-threads=1
cargo test --locked --all-features --lib retained_glyph_batch_gpu_resources -- \
  --ignored --nocapture --test-threads=1
cargo test --locked --all-features --lib native_budget_recovery_keeps_running_and_presents_again -- \
  --ignored --nocapture --test-threads=1
cargo test --locked --all-features --lib native_budget_failure_remains_fatal_without_opt_in -- \
  --ignored --nocapture --test-threads=1
```

Record exact source revision, adapter/driver and feature flags. Recheck moving
ball input/exit, mixed screen text/clipping, resize, cursor capture and the
voxel example's free camera. Historical screenshots and measurements describe
their recorded revision; they do not certify an untested release. Windows is
not currently a Sim;Logic release-qualified platform.

## Final package inspection and publication

```bash
git status --short
cargo package --list --locked
cargo publish --dry-run --locked --all-features
```

Use a clean, committed tree for this final check. Review unexpected files and
warnings. `--dry-run` validates packaging without uploading; the local check
script uses `cargo package --allow-dirty` only so an in-progress edit can be
tested before committing. That result is not final clean-source qualification.

After explicit release approval, use `cargo publish --locked --all-features`, confirm registry
and docs.rs results, then create/push the version tag on that tested commit.
A crates.io version cannot be overwritten: fix a bad release with a new version,
not by rewriting a published commit. See the official
[Cargo publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html)
and [docs.rs metadata reference](https://docs.rs/about/metadata).
