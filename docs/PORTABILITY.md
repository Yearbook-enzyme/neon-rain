# Portability status

Neon Rain is NixOS-first, but the project is being structured so the renderer and core behavior are not tied to one personal machine or one Linux distribution.

## Runtime checks

The published `v0.1.0-alpha.1` Nix release was launched with a completely empty temporary home directory while preserving only the active graphics, D-Bus, and PipeWire session.

The test confirmed:

- Successful Vulkan launch on an AMD Radeon RX 580
- Packaged Migu glyph-font discovery
- All requested Matrix glyphs available
- PipeWire and MPRIS discovery
- Graceful operation without lyric, moodbar, or profile helpers
- Stable rendering near the configured 100 FPS target
- Clean exit with status 0
- No dependency on files under the normal user home directory
- No personal home-directory paths in the runtime log

This proves first-run resilience for the packaged Nix release on the tested machine. It does not yet prove runtime compatibility on every Linux distribution or GPU.

## Native Linux CI

GitHub Actions also builds Neon Rain directly with Cargo on Ubuntu, outside the Nix development shell. That job runs formatting, unit tests, Clippy, and a locked release build using ordinary Ubuntu development libraries.

The native job is a compile-and-test check. GitHub's headless CI runner does not provide a representative desktop, GPU, audio session, or media player, so interactive runtime testing remains separate.

## Optional runtime integrations

The core visualizer should continue operating when enrichment tools are unavailable. Depending on the desired feature set, a Linux runtime may additionally provide:

- PipeWire command-line tools for live audio analysis
- `playerctl` for MPRIS media-player discovery
- Python for optional helper scripts
- Suitable Japanese/monospace fonts when using a non-Nix package

## Next portability targets

1. Test the native build on an actual Ubuntu desktop.
2. Create a relocatable generic Linux bundle.
3. Add an Arch Linux package recipe.
4. Evaluate Flatpak sandbox permissions for PipeWire, MPRIS, local media, and GPU access.
5. Add additional GPU and desktop-session reports from testers.
