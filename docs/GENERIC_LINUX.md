# Generic Linux bundle

The generic Linux archive is built directly with Cargo on Ubuntu 22.04. It is intended for current 64-bit glibc-based Linux desktops and does not require Nix.

## Contents

The archive contains:

- The optimized `neon-rain` executable
- A dependency-checking launcher
- A desktop entry
- A per-user installation helper
- The README, license, and Linux runtime notes
- A SHA-256 checksum beside the archive

## Run in place

Extract the archive and run:

```bash
tar -xzf neon-rain-*-x86_64-linux.tar.gz
cd neon-rain-*-x86_64-linux
./neon-rain
```

Press `Escape` to exit.

## Install for one user

From the extracted directory:

```bash
./install-user.sh
```

This copies the bundle under `~/.local/share/neon-rain`, creates a launcher at `~/.local/bin/neon-rain`, and installs a desktop entry under the user's local applications directory.

## Runtime requirements

The host system must provide:

- A 64-bit glibc-based Linux userspace compatible with Ubuntu 22.04 or newer
- A working Wayland or X11 desktop session
- Vulkan, OpenGL, or another graphics backend supported by `wgpu`
- The host's graphics drivers and loader libraries
- Fontconfig, including the `fc-match` command
- A readable monospace font; a Japanese-capable font gives the intended Katakana glyph set

Useful fonts include Migu 1M, Noto Sans Mono CJK JP, Noto Sans CJK JP, Noto Sans Mono, or DejaVu Sans Mono. When the selected font lacks Matrix glyphs, Neon Rain substitutes safe fallback characters.

## Optional integrations

The core visualizer works without these integrations:

- PipeWire command-line tools for live audio analysis
- `playerctl` for MPRIS media-player awareness
- The Neon Rain lyric, moodbar, and track-profile helper commands
- A local image or media directory

Launch without local media:

```bash
./neon-rain --no-media
```

Launch with an image directory:

```bash
./neon-rain --media-dir "/path/to/images"
```

## Compatibility scope

This bundle intentionally does not package Mesa, proprietary GPU drivers, Vulkan ICDs, PipeWire daemons, or desktop-session libraries. Those components must match the host system and are safer to use from the distribution.

The initial bundle is a portability alpha. Reports should include the distribution and version, desktop/session, GPU, graphics backend, and sanitized terminal output.
