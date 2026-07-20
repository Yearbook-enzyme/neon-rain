use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliAction {
    Run,
    Help,
    Version,
    ListScenes,
    ListThemes,
    ListPalettes,
    PrintConfig,
    WriteDefaultConfig,
    ResetSession,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenePreset {
    pub slug: &'static str,
    pub label: &'static str,
    pub theme: &'static str,
    pub palette: &'static str,
    pub auto_flight: &'static str,
    pub cinematic: bool,
    pub field_of_view: u32,
}

pub const SCENE_PRESETS: [ScenePreset; 6] = [
    ScenePreset {
        slug: "classic-matrix",
        label: "Classic Matrix",
        theme: "classic",
        palette: "theme",
        auto_flight: "forward",
        cinematic: true,
        field_of_view: 60,
    },
    ScenePreset {
        slug: "lucid-dream",
        label: "Lucid Dream",
        theme: "dream",
        palette: "vaporwave",
        auto_flight: "weave",
        cinematic: true,
        field_of_view: 66,
    },
    ScenePreset {
        slug: "cyber-tunnel",
        label: "Cyber Tunnel",
        theme: "surge",
        palette: "cyberpunk",
        auto_flight: "tunnel",
        cinematic: true,
        field_of_view: 50,
    },
    ScenePreset {
        slug: "aurora-drift",
        label: "Aurora Drift",
        theme: "ghost",
        palette: "rainbow",
        auto_flight: "orbit",
        cinematic: true,
        field_of_view: 70,
    },
    ScenePreset {
        slug: "ember-terminal",
        label: "Ember Terminal",
        theme: "amber",
        palette: "ember",
        auto_flight: "forward",
        cinematic: false,
        field_of_view: 58,
    },
    ScenePreset {
        slug: "redline",
        label: "Redline",
        theme: "red-alert",
        palette: "ember",
        auto_flight: "tunnel",
        cinematic: true,
        field_of_view: 46,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub scene: String,
    pub theme: String,
    pub palette: String,
    pub fullscreen: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub field_of_view: u32,
    pub auto_flight: String,
    pub cinematic: bool,
    pub media_enabled: bool,
    pub media_path: Option<PathBuf>,
    pub remember: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            scene: "classic-matrix".to_owned(),
            theme: "classic".to_owned(),
            palette: "theme".to_owned(),
            fullscreen: true,
            window_width: 1280,
            window_height: 720,
            field_of_view: 60,
            auto_flight: "forward".to_owned(),
            cinematic: true,
            media_enabled: true,
            media_path: None,
            remember: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LaunchOptions {
    pub action: CliAction,
    pub preferences: Preferences,
    pub config_path: PathBuf,
    pub state_path: PathBuf,
    pub warm_cache: bool,
}

impl LaunchOptions {
    pub fn media_path(&self) -> Option<PathBuf> {
        self.preferences
            .media_enabled
            .then(|| self.preferences.media_path.clone())
            .flatten()
    }
}

pub fn scene_presets() -> &'static [ScenePreset] {
    &SCENE_PRESETS
}

pub fn scene_preset(name: &str) -> Option<&'static ScenePreset> {
    let normalized = normalize_name(name);

    SCENE_PRESETS.iter().find(|scene| {
        scene.slug == normalized
            || (scene.slug == "classic-matrix"
                && matches!(normalized.as_str(), "classic" | "matrix"))
            || (scene.slug == "lucid-dream" && matches!(normalized.as_str(), "lucid" | "dream"))
            || (scene.slug == "cyber-tunnel" && matches!(normalized.as_str(), "cyber" | "tunnel"))
            || (scene.slug == "aurora-drift" && matches!(normalized.as_str(), "aurora" | "drift"))
            || (scene.slug == "ember-terminal"
                && matches!(normalized.as_str(), "ember" | "terminal"))
    })
}

pub fn next_scene_preset(current: &str) -> &'static ScenePreset {
    let index = scene_preset(current)
        .and_then(|selected| {
            SCENE_PRESETS
                .iter()
                .position(|candidate| candidate.slug == selected.slug)
        })
        .unwrap_or(0);

    &SCENE_PRESETS[(index + 1) % SCENE_PRESETS.len()]
}

pub fn apply_scene(preferences: &mut Preferences, name: &str) -> Result<(), String> {
    if normalize_name(name) == "custom" {
        preferences.scene = "custom".to_owned();
        return Ok(());
    }

    let Some(scene) = scene_preset(name) else {
        return Err(format!("Unknown scene: {name}"));
    };

    preferences.scene = scene.slug.to_owned();
    preferences.theme = scene.theme.to_owned();
    preferences.palette = scene.palette.to_owned();
    preferences.auto_flight = scene.auto_flight.to_owned();
    preferences.cinematic = scene.cinematic;
    preferences.field_of_view = scene.field_of_view;
    Ok(())
}

pub fn parse_launch_options() -> Result<LaunchOptions, String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let config_path = explicit_config_path(&arguments).unwrap_or_else(default_config_path);
    let state_path = default_state_path();

    let mut preferences = load_config(&config_path, Preferences::default())
        .map_err(|error| format!("Could not read {}: {error}", config_path.display()))?;

    let no_remember_requested = arguments.iter().any(|argument| argument == "--no-remember");
    if preferences.remember && !no_remember_requested {
        preferences = load_config(&state_path, preferences)
            .map_err(|error| format!("Could not read {}: {error}", state_path.display()))?;
    }

    let mut action = CliAction::Run;
    let mut warm_cache = false;
    let mut positional_media = None;
    let mut index = 0usize;

    while index < arguments.len() {
        let argument = arguments[index].as_str();

        match argument {
            "-h" | "--help" => action = CliAction::Help,
            "-V" | "--version" => action = CliAction::Version,
            "--list-scenes" => action = CliAction::ListScenes,
            "--list-themes" => action = CliAction::ListThemes,
            "--list-palettes" => action = CliAction::ListPalettes,
            "--print-config" => action = CliAction::PrintConfig,
            "--write-default-config" => action = CliAction::WriteDefaultConfig,
            "--reset-session" => action = CliAction::ResetSession,
            "--warm-cache" => warm_cache = true,
            "--fullscreen" => preferences.fullscreen = true,
            "--windowed" => preferences.fullscreen = false,
            "--cinematic" => {
                preferences.cinematic = true;
                preferences.scene = "custom".to_owned();
            }
            "--no-cinematic" => {
                preferences.cinematic = false;
                preferences.scene = "custom".to_owned();
            }
            "--media" => preferences.media_enabled = true,
            "--no-media" => {
                preferences.media_enabled = false;
                preferences.media_path = None;
            }
            "--remember" => preferences.remember = true,
            "--no-remember" => preferences.remember = false,
            "--scene" => {
                index += 1;
                apply_scene(
                    &mut preferences,
                    required_value(&arguments, index, "--scene")?,
                )?;
            }
            "--theme" => {
                index += 1;
                preferences.theme = required_value(&arguments, index, "--theme")?.to_owned();
                preferences.scene = "custom".to_owned();
            }
            "--palette" => {
                index += 1;
                preferences.palette = required_value(&arguments, index, "--palette")?.to_owned();
                preferences.scene = "custom".to_owned();
            }
            "--auto-flight" => {
                index += 1;
                preferences.auto_flight =
                    required_value(&arguments, index, "--auto-flight")?.to_owned();
                preferences.scene = "custom".to_owned();
            }
            "--fov" => {
                index += 1;
                preferences.field_of_view = required_value(&arguments, index, "--fov")?
                    .parse::<u32>()
                    .map_err(|_| "--fov requires an integer from 32 through 88".to_owned())?;
                preferences.scene = "custom".to_owned();
            }
            "--size" => {
                index += 1;
                let value = required_value(&arguments, index, "--size")?;
                let (width, height) = parse_size(value)?;
                preferences.window_width = width;
                preferences.window_height = height;
            }
            "--image" | "--media-dir" => {
                index += 1;
                preferences.media_path =
                    Some(PathBuf::from(required_value(&arguments, index, argument)?));
                preferences.media_enabled = true;
            }
            "--config" => {
                index += 1;
                let _ = required_value(&arguments, index, "--config")?;
            }
            "--" => {
                if let Some(value) = arguments.get(index + 1) {
                    positional_media = Some(PathBuf::from(value));
                }
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("Unknown option: {value}"));
            }
            value => {
                if positional_media.is_some() {
                    return Err(format!("Unexpected extra path argument: {value}"));
                }
                positional_media = Some(PathBuf::from(value));
            }
        }

        index += 1;
    }

    if let Some(path) = positional_media {
        preferences.media_enabled = true;
        preferences.media_path = Some(path);
    }

    clamp_preferences(&mut preferences);

    Ok(LaunchOptions {
        action,
        preferences,
        config_path,
        state_path,
        warm_cache,
    })
}

pub fn render_preferences(preferences: &Preferences) -> String {
    let media_path = preferences
        .media_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    format!(
        "# Neon Rain effective settings\n\
         scene = {}\n\
         theme = {}\n\
         palette = {}\n\
         fullscreen = {}\n\
         window_width = {}\n\
         window_height = {}\n\
         field_of_view = {}\n\
         auto_flight = {}\n\
         cinematic = {}\n\
         media_enabled = {}\n\
         media_path = {}\n\
         remember = {}\n",
        preferences.scene,
        preferences.theme,
        preferences.palette,
        preferences.fullscreen,
        preferences.window_width,
        preferences.window_height,
        preferences.field_of_view,
        preferences.auto_flight,
        preferences.cinematic,
        preferences.media_enabled,
        media_path,
        preferences.remember,
    )
}

pub fn write_default_config(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, default_config_text())
}

pub fn save_session(path: &Path, preferences: &Preferences) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temporary = path.with_extension("tmp");
    fs::write(&temporary, render_preferences(preferences))?;
    fs::rename(temporary, path)
}

pub fn reset_session(path: &Path) -> io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    fs::remove_file(path)?;
    Ok(true)
}

pub fn load_config(path: &Path, mut preferences: Preferences) -> io::Result<Preferences> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(preferences),
        Err(error) => return Err(error),
    };

    apply_config_text(&text, &mut preferences);
    clamp_preferences(&mut preferences);
    Ok(preferences)
}

pub fn default_config_text() -> &'static str {
    r#"# Neon Rain configuration
#
# A scene coordinates the theme, palette, automatic flight, field of view,
# and cinematic behavior. Change the scene line for the simplest setup.
#
# Uncomment individual visual values only when deliberately overriding part
# of the selected scene. Command-line options override this file.
#
# Home reloads this file while Neon Rain is running. End saves the current
# session under XDG_STATE_HOME. Normal exits also save when remember is true.

scene = classic-matrix

# Optional scene overrides:
# theme = classic
# palette = theme
# field_of_view = 60
# auto_flight = forward
# cinematic = true

fullscreen = true
window_width = 1280
window_height = 720
media_enabled = true
media_path =
remember = true
"#
}

fn explicit_config_path(arguments: &[String]) -> Option<PathBuf> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == "--config")
        .map(|pair| PathBuf::from(&pair[1]))
}

fn default_config_path() -> PathBuf {
    xdg_path("XDG_CONFIG_HOME", ".config")
        .join("neon-rain")
        .join("config.conf")
}

fn default_state_path() -> PathBuf {
    xdg_path("XDG_STATE_HOME", ".local/state")
        .join("neon-rain")
        .join("session.conf")
}

fn xdg_path(variable: &str, home_suffix: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|home| {
                let mut path = PathBuf::from(home);
                for component in home_suffix.split('/') {
                    path.push(component);
                }
                path
            })
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

fn required_value<'a>(
    arguments: &'a [String],
    index: usize,
    option: &str,
) -> Result<&'a str, String> {
    arguments
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("{option} requires a value"))
}

fn parse_size(value: &str) -> Result<(u32, u32), String> {
    let Some((width, height)) = value.split_once('x').or_else(|| value.split_once('X')) else {
        return Err(format!("Invalid size {value:?}; expected WIDTHxHEIGHT"));
    };

    let width = width
        .parse::<u32>()
        .map_err(|_| format!("Invalid width in {value:?}"))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| format!("Invalid height in {value:?}"))?;

    if width < 320 || height < 240 {
        return Err("Window size must be at least 320x240".to_owned());
    }

    Ok((width, height))
}

fn apply_config_text(text: &str, preferences: &mut Preferences) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim();
        let value = value.trim().trim_matches('"');

        match key {
            "scene" if !value.is_empty() => {
                if let Err(error) = apply_scene(preferences, value) {
                    eprintln!("Ignoring {error} in configuration");
                }
            }
            "theme" if !value.is_empty() => preferences.theme = value.to_owned(),
            "palette" if !value.is_empty() => preferences.palette = value.to_owned(),
            "fullscreen" => {
                if let Some(parsed) = parse_bool(value) {
                    preferences.fullscreen = parsed;
                }
            }
            "window_width" => {
                if let Ok(parsed) = value.parse::<u32>() {
                    preferences.window_width = parsed;
                }
            }
            "window_height" => {
                if let Ok(parsed) = value.parse::<u32>() {
                    preferences.window_height = parsed;
                }
            }
            "field_of_view" | "fov" => {
                if let Ok(parsed) = value.parse::<u32>() {
                    preferences.field_of_view = parsed;
                }
            }
            "auto_flight" if !value.is_empty() => preferences.auto_flight = value.to_owned(),
            "cinematic" => {
                if let Some(parsed) = parse_bool(value) {
                    preferences.cinematic = parsed;
                }
            }
            "media_enabled" => {
                if let Some(parsed) = parse_bool(value) {
                    preferences.media_enabled = parsed;
                }
            }
            "media_path" => {
                preferences.media_path = (!value.is_empty()).then(|| PathBuf::from(value));
            }
            "remember" => {
                if let Some(parsed) = parse_bool(value) {
                    preferences.remember = parsed;
                }
            }
            _ => {}
        }
    }
}

fn clamp_preferences(preferences: &mut Preferences) {
    preferences.window_width = preferences.window_width.clamp(320, 16384);
    preferences.window_height = preferences.window_height.clamp(240, 16384);
    preferences.field_of_view = preferences.field_of_view.clamp(32, 88);
}

fn normalize_name(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .replace(&['_', ' '][..], "-")
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_size() {
        assert_eq!(parse_size("1920x1080"), Ok((1920, 1080)));
        assert!(parse_size("small").is_err());
        assert!(parse_size("100x100").is_err());
    }

    #[test]
    fn scene_applies_coordinated_values() {
        let mut preferences = Preferences::default();
        apply_scene(&mut preferences, "lucid").unwrap();

        assert_eq!(preferences.scene, "lucid-dream");
        assert_eq!(preferences.theme, "dream");
        assert_eq!(preferences.palette, "vaporwave");
        assert_eq!(preferences.auto_flight, "weave");
        assert_eq!(preferences.field_of_view, 66);
    }

    #[test]
    fn later_config_values_override_scene() {
        let mut preferences = Preferences::default();
        apply_config_text(
            "scene = cyber-tunnel\npalette = rainbow\nfield_of_view = 72\n",
            &mut preferences,
        );

        assert_eq!(preferences.scene, "cyber-tunnel");
        assert_eq!(preferences.theme, "surge");
        assert_eq!(preferences.palette, "rainbow");
        assert_eq!(preferences.field_of_view, 72);
    }

    #[test]
    fn render_round_trips_core_values() {
        let preferences = Preferences {
            scene: "custom".to_owned(),
            theme: "surge".to_owned(),
            palette: "cyberpunk".to_owned(),
            fullscreen: false,
            field_of_view: 54,
            auto_flight: "weave".to_owned(),
            ..Preferences::default()
        };
        let rendered = render_preferences(&preferences);
        let mut reparsed = Preferences::default();
        apply_config_text(&rendered, &mut reparsed);

        assert_eq!(reparsed.scene, "custom");
        assert_eq!(reparsed.theme, "surge");
        assert_eq!(reparsed.palette, "cyberpunk");
        assert!(!reparsed.fullscreen);
        assert_eq!(reparsed.field_of_view, 54);
        assert_eq!(reparsed.auto_flight, "weave");
    }
}
