# dmux release guide

This follows the same broad model used by projects like zellij:
- tag-based GitHub releases
- prebuilt binaries per OS/target
- optional source install via `cargo`

## 1. Pre-release checklist

1. Ensure formatting and build are green:
   ```bash
   cargo fmt --check
   cargo check --all-targets
   ```
1. Update `README.md` if commands/behavior changed.
1. Confirm the workspace version in [`Cargo.toml`](/mnt/d/d-projects/rust-projects/dmux/dmux/Cargo.toml) under `[workspace.package]`.

## 2. Cut a release tag

Use semantic version tags (`vX.Y.Z`), for example:

```bash
git tag v0.2.0
git push origin v0.2.0
```

Pushing the tag triggers:
- [`.github/workflows/release.yml`](/mnt/d/d-projects/rust-projects/dmux/dmux/.github/workflows/release.yml)

It builds and uploads release assets:
- `dmux-x86_64-unknown-linux-musl.tar.gz`
- `dmux-x86_64-apple-darwin.tar.gz`
- `dmux-x86_64-pc-windows-msvc.zip`
- matching `*.sha256sum` files

## 3. Verify release

1. Open the GitHub release page and confirm all expected assets exist.
1. Test installer against the new tag:
   ```bash
   DMUX_REPO=<owner>/<repo> DMUX_VERSION=v0.2.0 sh scripts/install.sh
   dmux --help
   ```

## 4. Optional: publish to crates.io

If you want `cargo install dmux` from crates.io (not just GitHub source/releases),
you will need a dedicated publish strategy for the workspace crates. Right now,
the quickest user path is GitHub Releases + installer script.
