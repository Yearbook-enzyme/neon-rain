# Neon Rain

A GPU-rendered Matrix rain environment written in Rust and WGSL.

The renderer places persistent streams in a continuous, repeating world-space
volume. The camera can move through the volume while themes, HDR bloom,
temporal persistence, and an image-driven media field remain active.

## Run with Logan's visualizer wallpapers

The program automatically checks this folder when no media argument is given:

```text
/home/logan/Documents/pictures/vizualizer wallpapers
```

Run normally:

```bash
cargo run --release
```

Or provide another image or folder explicitly:

```bash
cargo run --release -- --image "/path/to/image.png"
cargo run --release -- --media-dir "/path/to/image folder"
cargo run --release -- --no-media
```

PNG, JPEG, and WebP files are supported. Image folders are scanned recursively.

Press `F1` or `?` while Neon Rain is running to open the complete in-app keybinding panel.

## Media-field controls

| Key | Action |
|---|---|
| `M` | Cycle off, silhouette, source-color, and ghost modes |
| `,` / `.` | Previous / next image |
| `I` | Rescan and reload the image folder |
| `O` / `P` | Reduce / increase image influence |
| `K` / `L` | Reduce / increase image contrast |
| `Z` / `X` | Reduce / increase image scale |

The image is not drawn as a flat overlay. Instead, its luminance changes glyph
visibility and its colors can tint the existing rain. This keeps the image
inside the animated stream structure and world-space depth effect.

## Theme controls

| Key | Theme |
|---|---|
| `1` | Quiet green |
| `2` | Classic Matrix |
| `3` | Green Surge |
| `4` | Dream Cyan |
| `5` | Amber Terminal |
| `6` | Red Alert |
| `7` | Ultraviolet |
| `8` | Ghost |
| `9` | Monochrome |
| `[` / `]` | Previous / next theme |

## World-space camera

| Key | Action |
|---|---|
| `W` / `S` | Move forward / backward relative to the view |
| `A` / `D` | Strafe left / right relative to the view |
| `Q` / `E` | Move down / up |
| `Shift` | Three-times movement boost |
| `Ctrl` | Precision movement |
| `Page Up` / `Page Down` | Increase / decrease base camera speed |
| `Tab` | Capture/release the mouse for smooth mouse-look |
| Mouse | Look around while captured |
| Mouse wheel | Zoom by changing field of view |
| `C` | Cycle off, forward, weave, orbit, and tunnel auto-flight |
| `H` | Toggle the mouse-look reticle |
| `R` | Reset position, orientation, and zoom |
| `G` | Regenerate persistent streams |

Camera acceleration and deceleration are smoothed. Near- and far-volume fades
hide stream recycling as the camera travels through the repeating rain field.
The look range is intentionally limited to the populated forward corridor.

## Other controls

| Key | Action |
|---|---|
| `Space` | Pause or resume every visual clock and camera movement |
| `F11` | Toggle fullscreen |
| `Up` / `Down` | Adjust target rain speed |
| `Left` / `Right` | Adjust target glow |
| `-` / `=` | Adjust target exposure |
| `Esc` | Exit |

## Architecture

- `src/simulation.rs` — motion, weather, personalities, mutations, ecology,
  and cascades.
- `src/main.rs` — glyph instances, camera navigation, image loading and
  sampling, world projection, controls, themes, and GPU submission.
- `src/shader.wgsl` — instanced glyph rendering and continuous depth optics.
- `src/bloom.rs` — HDR and bloom render-pass orchestration.
- `src/bloom.wgsl` — extraction, blur, history, themed grading, and tone mapping.
- `src/atlas.rs` — static 64-glyph Matrix atlas generation.


## World-space media plane controls

- `M` cycles media appearance: off, silhouette, color, ghost.
- `,` and `.` switch images.
- `I` rescans the media folder.
- `O` and `P` decrease/increase influence.
- `K` and `L` decrease/increase contrast.
- `Z` and `X` decrease/increase image scale.
- `J` and `N` move the media plane nearer/farther.
- Arrow keys move the media plane vertically/horizontally.
- `V` cycles flat, portal, extruded, and volume spatial modes.
- `B` toggles whether the plane is locked to the camera or fixed in world space.


## Spatial editing aids

The media plane now displays a projected guide made from a center marker,
corner markers, edge markers, and a short facing-direction marker. The guide
color identifies the spatial mode: cyan for flat, amber for portal, magenta
for extruded, and blue for volume.

- `F` turns the camera to face the current media plane.
- `Y` toggles the spatial guide.
- `U` resets media depth, offsets, and scale.
- The window title and terminal status continue to show depth, offsets,
  world/camera lock, and spatial mode.


## Visual media preview aids

- `T` cycles media preview: `off`, `image`, `matrix`.
- `image` shows the media plane more directly in world space.
- `matrix` shows the plane as an embedded glyph sheet so it reads as part of the rainfall.
- `F` faces the active media plane.
- `Y` toggles the spatial guide.
- `U` resets depth, offset, and scale.


## Rain/media coupling

- `;` cycles coupling: `influence`, `rain-formed`, `diagnostic`.
- `influence` preserves the earlier subtle behavior.
- `rain-formed` widens the affected depth corridor, adds more real rain streams, brightens image regions, and dims unrelated rain so the picture is formed by falling glyphs.
- `diagnostic` dims unaffected rain and marks genuinely affected rain glyphs in bright orange.
- The title and periodic performance line show `affected/current rain glyphs` so the coupling is measurable.
- `T` remains the separate optional visual preview control.


## Strong media defaults

Media now starts in a deliberately obvious test configuration:

- color media mode
- rain-formed coupling
- volume spatial mode
- opacity 1.0
- contrast 2.0
- scale 1.25
- separate preview off

Press `0` at any time to restore these strong media defaults. A contrast of 2.0 preserves more image detail than the previous maximum of 4.0 while the stronger coupling supplies the visibility.


## Direct image preview in Matrix space

The media preview modes now serve two distinct purposes:

- `preview:image` shows a denser direct-image billboard in world space.
  It uses a solid block glyph so the source image reads much more like an actual picture.
- `preview:matrix` shows the image as a Matrix-style glyph mosaic embedded into the rain.

Press `T` to cycle preview modes:

- `off`
- `image`
- `matrix`

A direct image preview is intended for visual placement and recognition.
A matrix preview is intended for stylistic integration with the rainfall.


## Falling block-image preview

`T` now cycles four preview modes:

- `off`
- `image`
- `matrix`
- `rain`

`preview:rain` turns the blocky image into a falling, randomized block-sheet that moves downward like rain while still preserving the source image structure.
It is intended as a bridge between a literal image and Matrix-style motion.


## Ambient falling wallpaper apparitions

A separate ambient system can spawn occasional whole-image wallpaper apparitions in the world.
These are subtle translucent billboards that drift downward through space like rare Matrix ghosts.

Controls:

- `\` toggle ambient apparitions on or off.
- `` ` `` increase apparition frequency.
- `Backspace` decrease apparition frequency.
- `'` increase apparition opacity.
- `/` decrease apparition opacity.

These apparitions are independent from the manually positioned media plane.
They choose random images from the active media folder and float through the scene as ambient events.


## Integrated media pass

This pass improves three areas at once:

- smoother, filtered image-to-rain coupling for less flicker
- more rain-native ambient wallpaper apparitions with reduced moire
- automatic random media cycling for continually changing image-driven rain

Controls:

- `F7` toggle media auto-cycle on or off
- `F8` slower media auto-cycle
- `F9` faster media auto-cycle

The coupling field now uses a filtered low-frequency image version internally,
so rain adopts image structure and color more coherently instead of flickering on raw per-pixel detail.
Ambient apparitions also render as looser rain-ghost sheets instead of rigid block billboards.


## Media stability and ghost-edge polish

This pass targets visual stability rather than adding another media mode:

- filtered coupling images are preloaded once, avoiding image-decode stalls during auto-cycle
- rain coupling crossfades over roughly three seconds instead of changing abruptly
- auto-cycle now defaults to a less frequent 16-second interval
- bilinear low-frequency sampling reduces character-color shimmer and flicker
- apparition dropout is stable per image cell instead of being randomized every frame
- apparition borders receive an edge and radial vignette so rectangular image edges dissolve away
- apparition glyphs now retain much more of each source image's original color, with a randomized source/theme mixture

Controls remain:

- `F7` toggle automatic coupling-image cycling
- `F8` make cycling slower
- `F9` make cycling faster


## Camera-anchored coupling and apparition pacing

The image field that colors and shapes rainfall is now separate from the manually positioned media plane.
It follows the camera automatically, covers the active viewport, and remains centered through movement,
zoom, resizing, and fullscreen changes. The manual media plane can still stay in either world or camera mode.

The title now reports the automatic cycle timer and crossfade percentage, making it easy to confirm that
cycling is still active. Random coupling selection advances circularly through the valid preloaded images
and does not stop at the end of the directory.

Ambient wallpaper apparitions now default to a lower frequency and a maximum of two at once. Their spawn
accumulator is capped while the scene is full, preventing a backlog from causing rapid bursts later.


## Startup defaults

Neon Rain now starts in forward auto-flight so the camera immediately moves inward through the rain.
The media spatial guide starts hidden and remains hidden after applying strong media defaults with `0`.

- Press `C` to cycle forward → weave → orbit → tunnel → off.
- Press `Y` to show or hide the media spatial guide manually.
- Press `R` to reset the camera back to the default forward-flight state.


## Persistent cache, cinematic randomization, and session autostart

The filtered coupling-image set is now cached under the user's XDG cache directory
(`~/.cache/neon-rain/coupling-v3` by default). Cache entries persist across reboots and
are rebuilt only when their source image changes.

The cinematic director starts enabled and periodically chooses a new forward, weave,
tunnel, or orbit movement together with a smoothly changing field of view.

- `F6` toggles the cinematic movement/zoom director.
- Mouse-wheel zoom still works and delays the next automatic zoom change.
- `--warm-cache` prepares the persistent cache without opening a GPU window.

The installer also builds the release binary and installs a KDE session-autostart entry,
so Neon Rain opens after graphical login without compiling or rebuilding its image cache.


## Smooth cinematic lateral motion

Cinematic weave, orbit, and tunnel changes now use a six-second lateral ease-in.
Sideways targets are smaller and lateral acceleration is intentionally slower than forward motion,
so occasional left/right slides remain visible without feeling like sudden camera shoves.


## PipeWire music-reactive mode

Neon Rain can now listen to the default PipeWire output monitor and react to
music from Strawberry or any other application. The analyzer separates broad
bass, midrange, treble, loudness, stereo balance, and transient/beat energy,
then applies deliberately smoothed modulation to rain speed, bloom, stream
heads, cascades, image coupling, apparition timing, exposure, camera drift,
and subtle inward FOV pulses.

Controls:

- `F5` toggles music reactivity
- `Shift+F5` cycles subtle / balanced / intense response
- `F10` cycles system-audio / Strawberry-gated response

In Strawberry mode, MPRIS playback status gates the reaction. When `playerctl`
is installed, the current artist/title is also shown; otherwise Neon Rain uses
`busctl` for playback status. Cinematic changes can wait briefly for a detected
musical accent so major movement changes land more naturally.

The audio capture uses `pw-record` with `stream.capture.sink=true`, 48 kHz stereo
float samples, and a low-latency raw stream. No audio is saved to disk.


## Music color-balance pass

The music reactor now spends most of its energy on color and light rather than camera bumps.

- beat-driven field-of-view pulses and stereo camera drift are greatly reduced
- cinematic flight changes happen less often and no longer wait to fire directly on beats
- a slowly evolving music palette responds to bass, mids, treble, beats, and track changes
- wallpaper colors and the music palette can be used independently or blended together
- hybrid color mode is the new default

Controls:

- `F4` cycles music color mode: `wallpaper -> palette -> hybrid`
- `F5` toggles music reactivity
- `Shift+F5` cycles subtle, balanced, and intense response
- `F10` cycles system-audio and Strawberry-gated capture

Color modes:

- `wallpaper`: music strengthens the current wallpaper-derived coloring without adding a separate hue palette
- `palette`: music supplies a smooth animated palette while wallpaper structure still shapes the rainfall
- `hybrid`: wallpaper colors remain prominent while a slower music-driven palette washes through them


## Music-driven rain activity pass

This pass makes the Matrix code itself react more visibly to music, not only the camera and color grading.

What changes:

- denser rain during stronger midrange/overall energy
- brighter and more active stream bodies during musical surges
- more frequent white heads during treble and beat activity
- subtle glyph-identity variation/glitching tied to treble and beats
- rhythmic within-stream pulsing so the code feels more alive musically

Controls are unchanged from the existing music-reactive pass. Use `F4` for wallpaper/palette/hybrid color modes and `F5` / `Shift+F5` for music response.

## Full-scene adaptive music reactivity

The music visualizer now normalizes itself to each track's own quiet-to-loud range and applies a coordinated performance envelope across the whole scene.

The default showcase profile is:

- Strawberry-gated music source
- intense response
- hybrid wallpaper + generated music palette
- cinematic movement enabled
- music-driven rain, cascades, heads, glyph variation, glow, exposure, speed, coupling, apparitions, image-cycle tempo, color, and gradual FOV response

The camera still avoids sharp beat bumps: camera depth and FOV follow slower section energy, while beats primarily affect rain, cascades, glyphs, glow, and color.


## Instrumented spatial visualizer pass

This pass separates the music response into multiple independently moving systems so the result reads as a music visualizer rather than a global color filter.

- Hybrid mode now preserves much more of each wallpaper's local sampled colors.
- Wallpaper colors receive restrained per-channel musical lighting instead of being replaced by the generated palette.
- Generated palette color remains visible in the ordinary rain and as a smaller accent inside image-formed rain.
- Ambient apparition clouds retain their source image colors, with music changing brightness and channel lighting rather than replacing their identity.
- Bass controls forward/depth drive and gradual field-of-view breathing.
- Midrange controls horizontal spatial waves, coupling motion, density bands, and lateral camera coordinates.
- Treble controls vertical coordinates, head activity, glyph mutation, and fine glow.
- Stereo balance provides a smooth horizontal bias.
- Sustained section energy controls scale, density, movement, and scene intensity.
- Transients travel through the rain as spatially structured waves instead of only flashing the whole frame.

The camera remains smoothed, but now has separate X, Y, Z, yaw, pitch, and FOV music channels.

## Smart song profiles and calmer camera phrasing

This pass separates fast musical detail from slower visual structure.

Live PipeWire analysis now estimates tempo, beat confidence, busyness, beats, bars, and section energy. Busy songs still produce active glyphs, cascades, density bands, and fine glow, but camera motion follows the slower bar/section layer instead of every transient.

Strawberry supplies the current artist, title, album, genre, and file URL through MPRIS. On track changes, the installed `neon-rain-track-profile` helper:

- uses Strawberry metadata immediately
- optionally maps the recording through ListenBrainz/MusicBrainz
- fetches public recording/artist/release tags when available
- caches the resulting profile under `~/.cache/neon-rain/music-profiles-v1`
- selects an atmospheric, kinetic, dense, bass-led, dynamic, organic, or balanced visual profile

Metadata changes the *visual strategy* for a song. Live audio remains responsible for exact timing.

The title reports `profile`, estimated `bpm`, rhythm `conf`, and `busy` level.

### Apparition sizing

Wallpaper-character apparitions now have shorter lifetimes, smaller starting scales, camera-relative retirement, and a projected-screen-size fade. They should disappear before becoming oversized on large monitors.

## Moodbar-guided image matching and rain continuity

This pass uses Strawberry's per-track moodbar as a time-context layer while keeping live PipeWire audio as the timing authority.

For local tracks, Neon Rain looks for Strawberry moodbars in this order:

1. hidden or normal `.mood` files next to the song
2. Strawberry's `QNetworkDiskCache` moodbar entry
3. no timeline, with the existing live-audio analysis used by itself

The 1000 moodbar RGB samples represent low-, mid-, and high-frequency energy through the track. Neon Rain follows Strawberry's playback position and uses the current moodbar region for slower context rather than pretending metadata tags know exactly what is happening at a particular second.

Moodbar context now affects image selection:

- bright musical regions prefer brighter source images
- darker regions prefer darker source images
- spectral balance influences warmer/cooler image choices
- rapidly changing regions prefer more contrast and texture
- image-character apparitions and the rain-coupling field use the same target, with randomized scoring retained for variety

Hybrid coloring remains genuinely hybrid. The local image hue stays visible, a restrained moodbar channel balance lights it, and the generated palette remains a separate accent layer.

Rain-formed image coupling now has a protected visibility floor. Dark images, sparse masks, or carved image regions can shape and color the rainfall, but they cannot blank the entire music visualization. Turning media mode off also restores unmodified rain visibility.

The title reports moodbar state in this form:

`mood:<energy>/<change>@<track-percent>:<source>`

Typical sources are `strawberry-sidecar`, `strawberry-cache`, or `none`.


## Phase 31 — Hierarchical reactivity, track memory, and signature moments

This pass adds five higher-level presentation features:

- **Musical hierarchy**: fast musical detail now primarily drives glyph activity, cascades, and fine glow, while slower section energy drives camera phrasing and broader scene changes.
- **Per-track visual memory**: the selected wallpaper/image coupling for each Strawberry track is remembered in `~/.cache/neon-rain/track-visual-memory-v1.tsv` and restored when the track returns.
- **Longer image handoffs**: image transitions use a slightly longer crossfade so swaps read more like a dissolve than a hard change.
- **Call-and-response spatial behavior**: left and right regions of the rain field alternate emphasis over the musical phrase, and apparition spawns are softly biased toward the active side.
- **Signature climax event**: every so often, when section energy, rhythmic confidence, and change all align, Neon Rain emits a brief convergence pulse that intensifies visibility, cascades, and glow across the field.

## Phase 32 — Live lyric semantic conductor

Neon Rain can now use lyrics as a live high-level guidance layer while Strawberry is playing.

Source priority:

1. timestamped `.lrc` beside the local audio file
2. `.lyrics` or `.txt` sidecars
3. Strawberry/MPRIS `xesam:asText` metadata when supplied
4. embedded unsynced lyric tags through `ffprobe` when available

Timestamped lyrics are sampled with the preceding and upcoming lines included at lower weights. This smooths semantic changes and gives each line a little narrative context instead of switching the entire visual state on a single word.

The semantic channels are:

- warmth and coolness
- light and darkness
- intimacy
- motion
- tension
- release
- transcendence
- synthetic and organic imagery

These channels remain subordinate to the live audio and moodbar:

- live PipeWire audio retains authority over exact beats, transients, and instantaneous energy
- the moodbar retains authority over broader spectral/time structure
- timed lyrics guide color temperature, brightness tendency, camera openness, image matching, apparition behavior, and climax meaning
- untimed lyrics are deliberately weakened and act only as a whole-track thematic prior

The rain visibility floor also receives lyric-aware protection. Energetic, tense, liberating, or transcendent passages cannot be accidentally erased by a dark or sparse image mask.

The runtime helper is installed as:

```text
~/.local/bin/neon-rain-lyric-runtime
```

The status line reports the current semantic label and several active channels, for example:

```text
lyric:celestial:timed:sidecar warm:0.12 dark:0.00 move:0.36 tense:0.05
```

Press `F2` to open Signal Inspector. Use Left/Right to isolate one of 50 live audio, adaptive, structural, color, or mapped-output channels.

## Deployment resilience

Neon Rain treats player metadata, lyrics, and moodbar timelines as optional enrichment.
The universal fallback is a rolling live-audio timeline that estimates section energy,
change, novelty, trend, and a broad color character from the previous moments of audio.

The runtime reports one of these operating states:

- `autonomous`: no usable capture; the base world continues on its own.
- `listening`: audio has just appeared.
- `calibrating`: Neon Rain is learning the current level and rhythmic baseline.
- `performing`: live audio alone is driving the full conductor.
- `enriched`: player or precomputed information is adding confidence.
- `silence`: intentional settling during sustained quiet.
- `recovering`: graceful transition after capture or playback disappears.

`player-aware` source mode now falls back to live system audio when no compatible player
metadata source is available, so a missing player integration does not disable reactivity.

## Learned track timelines

When player metadata provides a stable track identity and playback position, Neon Rain
learns one structural sample per second while the song plays. The timeline is cached under
`$XDG_CACHE_HOME/neon-rain/analysis/` or `~/.cache/neon-rain/analysis/`.

The first play still uses the universal rolling live analysis. Later plays can reuse the
learned timeline for stronger structural continuity and limited lookahead, even when no
moodbar file exists. The cache contains derived numeric features only, not audio.

## User-level deployment

After a release build, run `./deploy/doctor.sh` and then `./deploy/install-user.sh`.
The deployment scripts treat `playerctl`, moodbars, lyrics, and profile helpers as optional.
Only PipeWire capture is required for live audio; the base Matrix world runs without it.
