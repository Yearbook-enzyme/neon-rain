use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliAction {
    Run,
    Help,
    Version,
    ListThemes,
    ListPalettes,
    PrintConfig,
    WriteDefaultConfig,
    ResetSession,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub theme: String,
    pub palette: String,
    pub fullscreen: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub auto_flight: String,
    pub cinematic: bool,
    pub media_enabled: bool,
    pub media_path: Option<PathBuf>,
    pub remember: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "classic".to_owned(),
            palette: "theme".to_owned(),
            fullscreen: true,
            window_width: 1280,
            window_height: 720,
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

pub fn parse_launch_options() -> Result<LaunchOptions, String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let config_path = explicit_config_path(&arguments).unwrap_or_else(default_config_path);
    let state_path = default_state_path();

    let mut preferences = load_preferences(&config_path, Preferences::default())
        .map_err(|error| format!("Could not read {}: {error}", config_path.display()))?;

    let no_remember_requested = arguments.iter().any(|argument| argument == "--no-remember");
    if preferences.remember && !no_remember_requested {
        preferences = load_preferences(&state_path, preferences)
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
            "--list-themes" => action = CliAction::ListThemes,
            "--list-palettes" => action = CliAction::ListPalettes,
            "--print-config" => action = CliAction::PrintConfig,
            "--write-default-config" => action = CliAction::WriteDefaultConfig,
            "--reset-session" => action = CliAction::ResetSession,
            "--warm-cache" => warm_cache = true,
            "--fullscreen" => preferences.fullscreen = true,
            "--windowed" => preferences.fullscreen = false,
            "--cinematic" => preferences.cinematic = true,
            "--no-cinematic" => preferences.cinematic = false,
            "--media" => preferences.media_enabled = true,
            "--no-media" => {
                preferences.media_enabled = false;
                preferences.media_path = None;
            }
            "--remember" => preferences.remember = true,
            "--no-remember" => preferences.remember = false,
            "--theme" => {
                index += 1;
                preferences.theme = required_value(&arguments, index, "--theme")?.to_owned();
            }
            "--palette" => {
                index += 1;
                preferences.palette = required_value(&arguments, index, "--palette")?.to_owned();
            }
            "--auto-flight" => {
                index += 1;
                preferences.auto_flight =
                    required_value(&arguments, index, "--auto-flight")?.to_owned();
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

    preferences.window_width = preferences.window_width.clamp(320, 16384);
    preferences.window_height = preferences.window_height.clamp(240, 16384);

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
         theme = {}\n\
         palette = {}\n\
         fullscreen = {}\n\
         window_width = {}\n\
         window_height = {}\n\
         auto_flight = {}\n\
         cinematic = {}\n\
         media_enabled = {}\n\
         media_path = {}\n\
         remember = {}\n",
        preferences.theme,
        preferences.palette,
        preferences.fullscreen,
        preferences.window_width,
        preferences.window_height,
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

pub fn default_config_text() -> &'static str {
    r#"# Neon Rain configuration
#
# Command-line options override this file. Normal exits remember the current
# theme, palette, window state, flight mode, cinematic director, and media path
# under XDG_STATE_HOME unless remember is false.

theme = classic
palette = theme
fullscreen = true
window_width = 1280
window_height = 720
auto_flight = forward
cinematic = true
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

fn load_preferences(path: &Path, mut preferences: Preferences) -> io::Result<Preferences> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(preferences),
        Err(error) => return Err(error),
    };

    apply_config_text(&text, &mut preferences);
    Ok(preferences)
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
    fn config_overrides_defaults() {
        let mut preferences = Preferences::default();
        apply_config_text(
            "theme = dream\npalette = vaporwave\nfullscreen = false\nremember = no\n",
            &mut preferences,
        );

        assert_eq!(preferences.theme, "dream");
        assert_eq!(preferences.palette, "vaporwave");
        assert!(!preferences.fullscreen);
        assert!(!preferences.remember);
    }

    #[test]
    fn render_round_trips_core_values() {
        let preferences = Preferences {
            theme: "surge".to_owned(),
            palette: "cyberpunk".to_owned(),
            fullscreen: false,
            auto_flight: "weave".to_owned(),
            ..Preferences::default()
        };
        let rendered = render_preferences(&preferences);
        let mut reparsed = Preferences::default();
        apply_config_text(&rendered, &mut reparsed);

        assert_eq!(reparsed.theme, "surge");
        assert_eq!(reparsed.palette, "cyberpunk");
        assert!(!reparsed.fullscreen);
        assert_eq!(reparsed.auto_flight, "weave");
    }
}
