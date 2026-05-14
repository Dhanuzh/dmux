# Install dmux

## Option 1: prebuilt release binary (recommended)

Use the installer script:

```bash
DMUX_REPO=<owner>/<repo> sh scripts/install.sh
```

Current prebuilt targets:
- Linux: `x86_64-unknown-linux-musl`
- macOS: `x86_64-apple-darwin`
- Windows: `x86_64-pc-windows-msvc` (manual download from release page)

Pin a specific version:

```bash
DMUX_REPO=<owner>/<repo> DMUX_VERSION=v0.2.0 sh scripts/install.sh
```

Custom install directory:

```bash
DMUX_REPO=<owner>/<repo> DMUX_INSTALL_DIR="$HOME/bin" sh scripts/install.sh
```

## Option 2: install from git source

```bash
cargo install --locked --git https://github.com/<owner>/<repo>.git dmux
```

## Option 3: local development build

```bash
cargo run -p dmux -- --help
```
