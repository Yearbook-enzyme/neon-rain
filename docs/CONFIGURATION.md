# Configuration

Neon Rain now uses XDG paths for editable configuration and remembered session preferences.

## Paths

Editable configuration:

```text
$XDG_CONFIG_HOME/neon-rain/config.conf
```

When `XDG_CONFIG_HOME` is unset, this becomes:

```text
~/.config/neon-rain/config.conf
```

Remembered session state:

```text
$XDG_STATE_HOME/neon-rain/session.conf
```

When `XDG_STATE_HOME` is unset, this becomes:

```text
~/.local/state/neon-rain/session.conf
```

The configuration file supplies intentional defaults. A normal application exit remembers the current theme, palette, fullscreen state, window size, auto-flight mode, cinematic-director state, and media path in the session file.

Command-line arguments override both files.

## Create a configuration

```bash
neon-rain --write-default-config
```

This refuses to overwrite an existing configuration.

Inspect the effective configuration:

```bash
neon-rain --print-config
```

Clear remembered session choices while preserving the editable configuration:

```bash
neon-rain --reset-session
```

Disable session loading and saving for one launch:

```bash
neon-rain --no-remember
```

## Themes

List themes:

```bash
neon-rain --list-themes
```

Available theme names:

- `quiet`
- `classic`
- `surge`
- `dream`
- `amber`
- `red-alert`
- `ultraviolet`
- `ghost`
- `monochrome`

Select a theme:

```bash
neon-rain --theme dream
```

Themes control motion, density, glow, bloom, exposure, and their native colors.

## Palettes

List palettes:

```bash
neon-rain --list-palettes
```

Available palette names:

- `theme` — use the selected theme's original colors
- `cyberpunk` — cyan, magenta, and deep electric blue
- `vaporwave` — pink, violet, and pale cyan
- `ice` — cool blue-white
- `ember` — orange, red, and warm white
- `rainbow` — green, cyan, magenta, and violet

Palettes are independent from motion themes:

```bash
neon-rain --theme surge --palette cyberpunk
neon-rain --theme dream --palette vaporwave
```

Press `F3` while Neon Rain is running to cycle palettes.

## Window and motion

```bash
neon-rain --windowed --size 1600x900
neon-rain --fullscreen
neon-rain --auto-flight weave
neon-rain --no-cinematic
```

Auto-flight values are `off`, `forward`, `weave`, `orbit`, and `tunnel`.

## Media

```bash
neon-rain --media-dir "/path/to/images"
neon-rain --image "/path/to/image.png"
neon-rain --no-media
```

A positional path is also accepted:

```bash
neon-rain "/path/to/images"
```

## Example configuration

```ini
theme = dream
palette = vaporwave
fullscreen = true
window_width = 1920
window_height = 1080
auto_flight = weave
cinematic = true
media_enabled = true
media_path = /home/user/Pictures/neon-rain
remember = true
```
