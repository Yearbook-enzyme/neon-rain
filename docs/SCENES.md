# Scene presets

Scenes coordinate a theme, palette, automatic camera flight, field of view, and cinematic-director state. They provide intentional visual identities rather than requiring users to assemble several settings individually.

List scenes:

```bash
neon-rain --list-scenes
```

Start one directly:

```bash
neon-rain --scene lucid-dream
```

Press `F12` while Neon Rain is running to cycle scenes.

## Included scenes

| Scene | Character |
|---|---|
| `classic-matrix` | The normal green Matrix presentation with forward travel |
| `lucid-dream` | Slow cyan dream motion recolored with a vaporwave palette |
| `cyber-tunnel` | High-energy surge mode, cyberpunk colors, and tunnel flight |
| `aurora-drift` | Sparse ghost rain, rainbow color, and orbiting drift |
| `ember-terminal` | Warm amber terminal styling with restrained straight movement |
| `redline` | Aggressive red/ember presentation with narrow, fast tunnel motion |

## Custom combinations

Selecting an individual theme, palette, flight mode, FOV, or cinematic setting changes the active scene label to `custom`.

```bash
neon-rain \
  --theme dream \
  --palette ice \
  --auto-flight orbit \
  --fov 72
```

## Live controls

- `F12`: next complete scene
- `F3`: next palette, making the scene custom
- `[` / `]`: previous or next theme, making the scene custom
- `Home`: reload the XDG configuration file
- `End`: write current choices to remembered session state
- `Insert`: toggle on-screen status toasts

The editable configuration remains separate from remembered session state. Reloading with `Home` applies the configuration file directly; it does not merge in the remembered session file.
