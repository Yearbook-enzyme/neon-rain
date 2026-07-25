# Debian and Ubuntu package

Neon Rain provides a vendor-built `.deb` for current 64-bit Debian and Ubuntu desktops. The package is built on Ubuntu 22.04 from the locked Rust source tree and is intended for direct distribution through GitHub releases while the project remains in alpha.

## Install

Download the `.deb` and matching checksum from the release or workflow artifact, then run:

```bash
sha256sum -c neon-rain_*.deb.sha256
sudo apt install ./neon-rain_*.deb
```

Launch from the desktop menu or run:

```bash
neon-rain
```

Remove it with:

```bash
sudo apt remove neon-rain
```

User configuration and remembered state are intentionally preserved under the normal XDG directories after package removal.

## Runtime integration

The package depends on the shared libraries detected from the built executable and on Fontconfig. It recommends:

- a Vulkan or OpenGL userspace loader
- PipeWire command-line tools for music analysis
- `playerctl` for MPRIS player information
- Noto CJK fonts for the intended Katakana glyph set

The base Matrix world remains usable when optional music and metadata integrations are unavailable.

## Build the package

The supported packaging environment is Ubuntu 22.04, matching the GitHub Actions workflow:

```bash
cargo build --release --locked
scripts/package-deb.sh
```

The resulting package and checksum are written under `dist/`.

This GitHub-release package is not yet a Debian archive submission. Official Debian inclusion would require a separate source-package review and Debian Rust dependency work.
