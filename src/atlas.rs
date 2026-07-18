use std::{fs, process::Command};

use fontdue::{Font, FontSettings};

pub const ATLAS_COLUMNS: u32 = 8;
pub const ATLAS_ROWS: u32 = 8;

pub const GLYPH_WIDTH: u32 = 32;
pub const GLYPH_HEIGHT: u32 = 40;

pub const ATLAS_WIDTH: u32 = ATLAS_COLUMNS * GLYPH_WIDTH;

pub const ATLAS_HEIGHT: u32 = ATLAS_ROWS * GLYPH_HEIGHT;

// Matrix-inspired mixture of Katakana, digits, angular Latin
// forms, and technical punctuation.
const GLYPHS: [char; 64] = [
    // Digits
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', // Selected Latin forms
    'A', 'C', 'E', 'H', 'K', 'M', 'N', 'R', // Full-width Katakana
    'ア', 'イ', 'ウ', 'エ', 'オ', 'カ', 'キ', 'ク', 'ケ', 'コ', 'サ', 'シ', 'ス', 'セ', 'ソ', 'タ',
    'チ', 'ツ', 'テ', 'ト', 'ナ', 'ニ', 'ヌ', 'ネ', // Half-width Katakana
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ', 'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ', 'ﾀ',
    // Symbols
    '+', '-', '*', '=', ':', '/',
];

// Used only when the selected font unexpectedly lacks a requested
// character. Every atlas cell therefore remains usable.
const FALLBACK_GLYPHS: [char; 64] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I',
    'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b',
    'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u',
    'v', 'w', 'x', 'y', 'z', '+', '-',
];

pub fn create_glyph_atlas() -> Vec<u8> {
    let font = load_font();

    let mut atlas = vec![0u8; (ATLAS_WIDTH * ATLAS_HEIGHT) as usize];

    let mut substitutions = Vec::new();

    for (glyph_index, character) in GLYPHS.iter().copied().enumerate() {
        let render_character = if font.lookup_glyph_index(character) == 0 {
            let fallback = FALLBACK_GLYPHS[glyph_index];

            substitutions.push((character, fallback));

            fallback
        } else {
            character
        };

        let (metrics, bitmap) = font.rasterize(render_character, 30.0);

        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }

        let glyph_index = glyph_index as u32;

        let atlas_column = glyph_index % ATLAS_COLUMNS;

        let atlas_row = glyph_index / ATLAS_COLUMNS;

        let cell_origin_x = atlas_column * GLYPH_WIDTH;

        let cell_origin_y = atlas_row * GLYPH_HEIGHT;

        let available_width = GLYPH_WIDTH.saturating_sub(4);

        let available_height = GLYPH_HEIGHT.saturating_sub(4);

        let copy_width = (metrics.width as u32).min(available_width);

        let copy_height = (metrics.height as u32).min(available_height);

        let destination_x = cell_origin_x + (GLYPH_WIDTH - copy_width) / 2;

        let destination_y = cell_origin_y + (GLYPH_HEIGHT - copy_height) / 2;

        let source_x = (metrics.width as u32 - copy_width) / 2;

        let source_y = (metrics.height as u32 - copy_height) / 2;

        for y in 0..copy_height {
            for x in 0..copy_width {
                let source_index = ((source_y + y) * metrics.width as u32 + source_x + x) as usize;

                let destination_index =
                    ((destination_y + y) * ATLAS_WIDTH + destination_x + x) as usize;

                atlas[destination_index] = bitmap[source_index];
            }
        }
    }

    let atlas_energy: usize = atlas.iter().map(|value| *value as usize).sum();

    assert!(
        atlas_energy > 0,
        "Glyph atlas rasterized empty.          The selected font is not producing usable bitmaps."
    );

    if substitutions.is_empty() {
        println!(
            "All {} requested Matrix glyphs are supported.",
            GLYPHS.len(),
        );
    } else {
        eprintln!(
            "{} requested glyphs used safe fallbacks:",
            substitutions.len(),
        );

        for (missing, fallback) in substitutions {
            eprintln!("  {missing:?} -> {fallback:?}");
        }
    }

    atlas
}

fn load_font() -> Font {
    let candidates = [
        "Migu 1M",
        "Noto Sans Mono CJK JP",
        "Noto Sans CJK JP",
        "Noto Sans Mono",
        "DejaVu Sans Mono",
        "monospace",
    ];

    let mut errors = Vec::new();

    for candidate in candidates {
        let Some(path) = find_font_path(candidate) else {
            errors.push(format!("{candidate}: fc-match returned no path"));

            continue;
        };

        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,

            Err(error) => {
                errors.push(format!(
                    "{candidate}: could not read \
                     {path}: {error}"
                ));

                continue;
            }
        };

        match Font::from_bytes(bytes, FontSettings::default()) {
            Ok(font) => {
                println!(
                    "Using glyph font: \
                     {candidate} ({path})"
                );

                return font;
            }

            Err(error) => {
                errors.push(format!(
                    "{candidate}: could not parse \
                     {path}: {error}"
                ));
            }
        }
    }

    panic!("Could not load a usable font.\n{}", errors.join("\n"),);
}

fn find_font_path(pattern: &str) -> Option<String> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}\n", pattern])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8(output.stdout).ok()?;

    let path = path.lines().next()?.trim();

    if path.is_empty() {
        return None;
    }

    Some(path.to_owned())
}
