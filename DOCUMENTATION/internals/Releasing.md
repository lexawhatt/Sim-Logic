# Preparing a release

[Documentation](../README.md) / Internals

The current candidate is **0.1.0**, using registry **sim-engine = 0.4.2**.
Publication is a separate, deliberate step. None of the checks below uploads
the crate, creates a tag or rewrites Git history.

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

Before publishing, set the final version in `Cargo.toml`, resolve the lockfile,
move the changelog's candidate entry to a dated release entry, and replace
pre-publication checkout instructions in the README and getting-started guide.
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
cargo publish --dry-run --locked
```

Use a clean, committed tree for this final check. Review unexpected files and
warnings. `--dry-run` validates packaging without uploading; the local check
script uses `cargo package --allow-dirty` only so an in-progress edit can be
tested before committing. That result is not final clean-source qualification.

After explicit release approval, use `cargo publish --locked`, confirm registry
and docs.rs results, then create/push the version tag on that tested commit.
A crates.io version cannot be overwritten: fix a bad release with a new version,
not by rewriting a published commit. See the official
[Cargo publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html)
and [docs.rs metadata reference](https://docs.rs/about/metadata).
