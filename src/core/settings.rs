use std::path::PathBuf;

pub const DEFAULT_OPACITY: u32 = 96;
pub const OPACITY_CHOICES: [u32; 7] = [100, 96, 90, 80, 70, 60, 50];
pub const DEFAULT_FONT_SIZE: i32 = 15; // 本文のサイズ(論理ピクセル)
pub const FONT_SIZE_RANGE: (i32, i32) = (9, 48);
pub const DEFAULT_SIZE: (f32, f32) = (380.0, 220.0);
pub const MIN_SIZE: (f32, f32) = (240.0, 120.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Yellow,
    Light,
    Dark,
}

/// (値, 設定ファイル上の名前, メニューの表示名)
pub const THEMES: [(Theme, &str, &str); 3] = [
    (Theme::Yellow, "yellow", "イエロー"),
    (Theme::Light, "light", "ライト"),
    (Theme::Dark, "dark", "ダーク"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Normal,
    Controls,
    Codes,
}

pub const VIEWS: [(View, &str, &str); 3] = [
    (View::Normal, "normal", "通常"),
    (View::Controls, "controls", "制御文字を表示"),
    (View::Codes, "codes", "文字コード一覧"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub pos: Option<(f32, f32)>,
    pub size: Option<(f32, f32)>,
    pub opacity: u32,
    pub theme: Theme,
    pub view: View,
    pub background: Option<String>,
    /// フォント名(None なら既定)
    pub font: Option<String>,
    pub font_size: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            pos: None,
            size: None,
            opacity: DEFAULT_OPACITY,
            theme: Theme::Yellow,
            view: View::Normal,
            background: None,
            font: None,
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

impl Settings {
    /// `key=value` の行形式。未知のキーや壊れた行は無視する
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let (mut left, mut top, mut width, mut height) = (None, None, None, None);
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "left" => left = value.parse::<f32>().ok(),
                "top" => top = value.parse::<f32>().ok(),
                "width" => width = value.parse::<f32>().ok(),
                "height" => height = value.parse::<f32>().ok(),
                "opacity" => {
                    if let Ok(v) = value.parse::<u32>() {
                        s.opacity = v.clamp(10, 100);
                    }
                }
                "theme" => {
                    if let Some(&(t, _, _)) = THEMES.iter().find(|(_, id, _)| *id == value) {
                        s.theme = t;
                    }
                }
                "view" => {
                    if let Some(&(v, _, _)) = VIEWS.iter().find(|(_, id, _)| *id == value) {
                        s.view = v;
                    }
                }
                "background" if !value.is_empty() => s.background = Some(value.to_string()),
                "font" if !value.is_empty() => s.font = Some(value.to_string()),
                "font_size" => {
                    if let Ok(v) = value.parse::<i32>() {
                        s.font_size = v.clamp(FONT_SIZE_RANGE.0, FONT_SIZE_RANGE.1);
                    }
                }
                _ => {}
            }
        }
        s.pos = left.zip(top);
        s.size = width.zip(height).filter(|&(w, h)| w > 0.0 && h > 0.0);
        s
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        if let Some((l, t)) = self.pos {
            out.push_str(&format!("left={l:.1}\ntop={t:.1}\n"));
        }
        if let Some((w, h)) = self.size {
            out.push_str(&format!("width={w:.1}\nheight={h:.1}\n"));
        }
        let theme = THEMES.iter().find(|(t, _, _)| *t == self.theme).map_or("yellow", |t| t.1);
        let view = VIEWS.iter().find(|(v, _, _)| *v == self.view).map_or("normal", |v| v.1);
        out.push_str(&format!("opacity={}\ntheme={theme}\nview={view}\n", self.opacity));
        if let Some(bg) = &self.background {
            out.push_str(&format!("background={bg}\n"));
        }
        if let Some(font) = &self.font {
            out.push_str(&format!("font={font}\nfont_size={}\n", self.font_size));
        }
        out
    }

    pub fn load() -> Settings {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Settings::parse(&t))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = settings_path() {
            let _ = std::fs::write(path, self.serialize());
        }
    }
}

/// 設定フォルダ(なければ作る)。Windows: `%APPDATA%\ClipSticky`、macOS: `~/Library/Application Support/ClipSticky`
fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("APPDATA")?);
    #[cfg(not(windows))]
    let base = PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support");
    let dir = base.join("ClipSticky");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn settings_path() -> Option<PathBuf> {
    Some(config_dir()?.join("settings.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Settings {
            pos: Some((100.0, 50.0)),
            size: Some((400.0, 300.0)),
            opacity: 70,
            theme: Theme::Dark,
            view: View::Codes,
            background: Some(r"C:\a b\c,d.png".into()),
            font: Some("Meiryo".into()),
            font_size: 18,
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn parse_ignores_garbage_and_clamps() {
        let s = Settings::parse("opacity=500\nfont_size=1\nfoo\ntheme=nope\nwidth=-5\nheight=10\n");
        assert_eq!(s.opacity, 100);
        assert_eq!(s.font_size, FONT_SIZE_RANGE.0);
        assert_eq!(s.theme, Theme::Yellow);
        assert_eq!(s.size, None);
    }

    #[test]
    fn default_serializes_without_optional_keys() {
        let text = Settings::default().serialize();
        assert!(!text.contains("font=") && !text.contains("background="));
    }
}
