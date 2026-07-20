# Screenshots and demo capture

The repository includes a small desktop-aware capture helper:

```bash
scripts/capture-neon-rain.sh screenshot
scripts/capture-neon-rain.sh video
```

The generic Linux bundle includes the same helper under its `tools` directory, and the Nix package exposes it as:

```bash
neon-rain-capture screenshot
neon-rain-capture video
```

## Screenshot backends

The helper tries these tools in order:

1. KDE Spectacle
2. `grim` on Wayland
3. GNOME Screenshot
4. ImageMagick `import` on X11

## Video backends

- Wayland: `wf-recorder`
- X11: FFmpeg with `x11grab`

Video capture continues until `Ctrl+C`.

## Suggested demo preparation

1. Pick a visually distinct theme and palette.
2. Start music and confirm the intended player is selected.
3. Let the rain settle for several seconds.
4. Hide diagnostic overlays for beauty footage, or deliberately open `F2` for an instrumentation demonstration.
5. Capture a 15–30 second clip containing one cinematic transition.
