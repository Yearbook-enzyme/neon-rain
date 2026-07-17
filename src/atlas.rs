use std::{fs, process::Command};

use fontdue::{Font, FontSettings};

pub const ATLAS_COLUMNS: u32 = 8;
pub const ATLAS_ROWS: u32 = 8;

pub const GLYPH_WIDTH: u32 = 32;
pub const GLYPH_HEIGHT: u32 = 40;

pub const ATLAS_WIDTH: u32 = ATLAS_COLUMNS * GLYPH_WIDTH;

pub const ATLAS_HEIGHT: u32 = ATLAS_ROWS * GLYPH_HEIGHT;

const GLYPHS: [char; 64] = [
    // Digits
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', // Latin letters
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z', // Katakana
    'ア', 'イ', 'ウ', 'エ', 'オ', 'カ', 'キ', 'ク', 'ケ', 'コ', 'サ', 'シ', 'ス', 'セ', 'ソ', 'タ',
    'チ', 'ツ', 'テ', 'ト', 'ナ', 'ニ', 'ヌ', 'ネ', 'ノ', 'ハ', // Symbols
    '+', '-',
];

pub fn create_glyph_atlas() -> Vec<u8> {
    let font = load_font();

    let mut atlas = vec![0u8; (ATLAS_WIDTH * ATLAS_HEIGHT) as usize];

    for (glyph_index, character) in GLYPHS.iter().copied().enumerate() {
        let (metrics, bitmap) = font.rasterize(character, 30.0);

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

    atlas
}

fn load_font() -> Font {
    let candidates = [
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
