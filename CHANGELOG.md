# Changelog

All notable public-facing changes will be recorded here.

The project follows a pre-1.0 alpha development model. Interfaces, controls, configuration, and packaging may change between releases.

## Unreleased

No unreleased changes yet.

## [0.1.0-alpha.3] - 2026-07-20

### Added

- XDG configuration and remembered session state
- Six complete scene presets combining theme, palette, camera flight, FOV, and cinematic behavior
- Independent cyberpunk, vaporwave, ice, ember, and rainbow palettes
- Named theme, palette, scene, FOV, window, motion, media, and persistence command-line options
- Live configuration reload, manual session save, scene cycling, and status-toast controls
- On-screen status overlay for scene, palette, configuration, and persistence changes
- Improved optional-helper discovery through explicit environment and adjacent package paths
- Per-user uninstaller, initial configuration installation, desktop artwork, and capture helper
- Configuration, scene, and screenshot/video capture documentation
- Fast development checks and a reusable comprehensive release-check script

### Changed

- The default configuration is scene-first: optional component overrides are commented so changing only the scene works
- Runtime status output now identifies both the active scene and its theme/palette

### Verified

- All Rust targets, unit tests, formatting, Clippy, and optimized release compilation
- Reproducible Nix flake evaluation and package build
- Native Ubuntu CI outside Nix
- Generic Linux archive contents, portable checksum, CLI, and interactive Vulkan launch


## [0.1.0-alpha.2] - 2026-07-20

### Added

- Native Ubuntu Cargo build, tests, Clippy, and release-build checks outside Nix
- Generic `x86_64` Linux tarball workflow built on Ubuntu 22.04
- `--help` and `--version` command-line output
- Per-user installer, desktop entry, runtime launcher, and Linux bundle documentation
- SHA-256 checksum distributed beside the generic Linux archive

### Fixed

- Linux bundle checksums now record a portable filename rather than a GitHub runner path

### Verified

- Clean-profile launch with an empty temporary home directory
- Native Ubuntu compilation and tests without the Nix development shell
- Generic archive checksum, safe extraction, expected files, and CLI entry points
- Interactive Vulkan launch, music response, help overlay, and signal inspector from the generic bundle


## [0.1.0-alpha.1] - 2026-07-20

### Added

- NixOS-first reproducible package and development shell
- GitHub Actions checks for the Nix package, formatting, tests, and Clippy
- MIT licensing and public repository documentation
- Initial generated repository and social-preview artwork

### Verified

- Packaged fullscreen launch on NixOS
- Vulkan rendering on an AMD Radeon RX 580
- PipeWire, MPRIS, media helper, font, and image-field runtime discovery
