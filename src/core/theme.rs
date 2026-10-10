use eframe::egui::Color32;

use super::settings::Theme;

pub struct Palette {
    pub bg: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub border: Color32,
}

pub fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Yellow => Palette {
            bg: Color32::from_rgb(0xFF, 0xF4, 0xB8),
            text: Color32::from_rgb(0x3A, 0x32, 0x14),
            muted: Color32::from_rgb(0x8C, 0x7F, 0x4A),
            border: Color32::from_rgb(0xE6, 0xD3, 0x82),
        },
        Theme::Light => Palette {
            bg: Color32::from_rgb(0xFA, 0xFA, 0xFB),
            text: Color32::from_rgb(0x1C, 0x1C, 0x1E),
            muted: Color32::from_rgb(0x8A, 0x8A, 0x93),
            border: Color32::from_rgb(0xDD, 0xDD, 0xE3),
        },
        Theme::Dark => Palette {
            bg: Color32::from_rgb(0x20, 0x21, 0x24),
            text: Color32::from_rgb(0xEC, 0xEC, 0xF0),
            muted: Color32::from_rgb(0x98, 0x9A, 0xA4),
            border: Color32::from_rgb(0x3C, 0x3E, 0x44),
        },
    }
}

/// `fg` を `bg` に `percent`% の濃さで重ねた色
pub fn blend(fg: Color32, bg: Color32, percent: u32) -> Color32 {
    let mix = |f: u8, b: u8| ((f as u32 * percent + b as u32 * (100 - percent)) / 100) as u8;
    Color32::from_rgb(mix(fg.r(), bg.r()), mix(fg.g(), bg.g()), mix(fg.b(), bg.b()))
}
