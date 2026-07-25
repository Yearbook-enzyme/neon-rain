# Flatpak prototype

Neon Rain has an experimental local Flatpak build using the Freedesktop 25.08 runtime. This is currently a project-hosted prototype rather than a Flathub submission.

## NixOS host setup

Enable Flatpak in the NixOS configuration:

```nix
services.flatpak.enable = true;
```

Apply the configuration:

```bash
sudo nixos-rebuild switch
```

Open a new terminal after rebuilding. The local build also needs `flatpak-builder`; it can be used without permanently installing it:

```bash
nix shell nixpkgs#flatpak nixpkgs#flatpak-builder nixpkgs#appstream
```

## Build, install, and run

From the repository root:

```bash
nix shell nixpkgs#flatpak nixpkgs#flatpak-builder nixpkgs#appstream -c scripts/build-flatpak.sh
flatpak run io.github.yearbook_enzyme.neon_rain
```

The build script installs the Freedesktop runtime, SDK, and stable Rust SDK extension for the current user. It builds entirely from the locked Cargo dependency sources, installs the application for the current user, and produces a single-file bundle under `dist/`.

Remove the locally installed app with:

```bash
flatpak remove --user io.github.yearbook_enzyme.neon_rain
```

## Permissions under evaluation

The prototype currently requests:

- Wayland and fallback X11 display access
- DRI/GPU access
- PulseAudio-compatible audio access
- the native PipeWire runtime socket
- read-only Pictures and Music access
- MPRIS communication with Strawberry

The core visualizer should run with GPU and display permissions alone. Music response depends on whether the selected runtime supplies a compatible `pw-record` command and whether native PipeWire monitor capture works from the sandbox.

Inspect the runtime tools with:

```bash
flatpak run --command=sh io.github.yearbook_enzyme.neon_rain -c '
  printf "pw-record: "; command -v pw-record || true
  printf "playerctl: "; command -v playerctl || true
  printf "busctl: "; command -v busctl || true
'
```

Audit D-Bus access while launching the app with:

```bash
flatpak run --log-session-bus io.github.yearbook_enzyme.neon_rain
```

## Known prototype boundary

The application currently discovers music and metadata through Linux helper programs such as `pw-record`, `playerctl`, `busctl`, and optional Python helpers. Flatpak may require those helpers to be bundled or the integrations to be moved into native Rust libraries. The first milestone is a reliable sandboxed renderer; full music parity is the next Flatpak-specific milestone.

## Distribution status

Do not submit this manifest to Flathub yet. The local manifest and bundle can be tested and distributed directly from the Neon Rain project while sandbox behavior is being validated.
