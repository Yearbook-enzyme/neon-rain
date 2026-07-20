# Neon Rain deployment

## Current release target

The first public target is a NixOS-first Linux alpha.

Once this branch is published, users should be able to launch it with:

```bash
nix run github:Yearbook-enzyme/neon-rain
```

Run directly from the local checkout with:

```bash
nix run .
```

Enter the reproducible development shell with:

```bash
nix develop
```

Validate the complete flake with:

```bash
nix flake check -L
```

## Release ladder

1. NixOS alpha through the repository flake.
2. Test additional Linux GPUs and desktop environments.
3. Generic Linux/AppImage.
4. Arch Linux and the AUR.
5. Flatpak.
6. Debian/Ubuntu packaging.
7. Windows.
8. macOS.
9. Explore mobile platforms after the desktop architecture stabilizes.

## Before announcing the repository

- Choose and add a project license.
- Add screenshots or a short demonstration video.
- Verify startup behavior on a fresh user account.
- Document optional PipeWire, media-player, moodbar, lyric, and media-directory integration.
- Tag the first tested alpha release.
