//! トレイとウィンドウの右クリックで共通に使うメニュー

use muda::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

use crate::core::convert::{CONVERSION_GROUPS, Conversion, conversion_by_id, conversion_info};
use crate::core::i18n::{LANGUAGES, Language, tr};
use crate::core::settings::{
    DEFAULT_OPACITY, OPACITY_CHOICES, Settings, THEMES, Theme, VIEWS, View, theme_label, view_label,
};
use crate::fonts::DEFAULT_FONT;
use crate::tr_fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Show,
    ToggleMonitoring,
    View(View),
    /// テキストを変換して、クリップボードに入れ直す
    Convert(Conversion),
    Theme(Theme),
    Opacity(u32),
    BackgroundSelect,
    BackgroundClear,
    FontSelect,
    FontReset,
    /// 表示言語を切り替える
    Language(Language),
    Exit,
}

impl Command {
    fn id(&self) -> String {
        match self {
            Command::Show => "show".into(),
            Command::ToggleMonitoring => "toggle".into(),
            Command::View(v) => format!("view:{}", VIEWS.iter().find(|x| x.0 == *v).unwrap().1),
            Command::Convert(c) => format!("conv:{}", conversion_info(*c).id),
            Command::Theme(t) => format!("theme:{}", THEMES.iter().find(|x| x.0 == *t).unwrap().1),
            Command::Opacity(o) => format!("opacity:{o}"),
            Command::BackgroundSelect => "bg:select".into(),
            Command::BackgroundClear => "bg:clear".into(),
            Command::FontSelect => "font:select".into(),
            Command::FontReset => "font:reset".into(),
            Command::Language(l) => format!("lang:{}", LANGUAGES.iter().find(|x| x.0 == *l).unwrap().1),
            Command::Exit => "exit".into(),
        }
    }

    pub fn from_id(id: &str) -> Option<Command> {
        Some(match id.split_once(':') {
            None => match id {
                "show" => Command::Show,
                "toggle" => Command::ToggleMonitoring,
                "exit" => Command::Exit,
                _ => return None,
            },
            Some(("view", name)) => Command::View(VIEWS.iter().find(|x| x.1 == name)?.0),
            Some(("conv", id)) => Command::Convert(conversion_by_id(id)?.conversion),
            Some(("theme", name)) => Command::Theme(THEMES.iter().find(|x| x.1 == name)?.0),
            Some(("opacity", n)) => Command::Opacity(n.parse().ok()?),
            Some(("lang", name)) => Command::Language(LANGUAGES.iter().find(|x| x.1 == name)?.0),
            Some(("bg", "select")) => Command::BackgroundSelect,
            Some(("bg", "clear")) => Command::BackgroundClear,
            Some(("font", "select")) => Command::FontSelect,
            Some(("font", "reset")) => Command::FontReset,
            _ => return None,
        })
    }
}

pub struct AppMenu {
    pub menu: Menu,
    toggle: MenuItem,
    views: Vec<(View, CheckMenuItem)>,
    themes: Vec<(Theme, CheckMenuItem)>,
    opacities: Vec<(u32, CheckMenuItem)>,
    languages: Vec<(Language, CheckMenuItem)>,
    bg_clear: MenuItem,
    font_info: MenuItem,
    font_reset: MenuItem,
    /// 「テキストを変換してコピー」。内容がテキストのときだけ使える
    convert_menu: Submenu,
}

fn item(cmd: Command, text: &str) -> MenuItem {
    MenuItem::with_id(cmd.id(), text, true, None)
}

/// 言語の項目の表示名。自動以外は、その言語の名前で書く(どの言語が選ばれていても、自分の言語を見つけられるように)
fn language_label(l: Language) -> &'static str {
    match l {
        Language::Auto => tr("自動(OS の言語)", "Auto (system language)"),
        Language::Ja => "日本語",
        Language::En => "English",
    }
}

impl AppMenu {
    /// 現在の言語(`i18n::set_lang`)でメニューを作る。言語を切り替えたら、作り直す
    pub fn new(settings: &Settings, monitoring: bool) -> AppMenu {
        let toggle = item(Command::ToggleMonitoring, tr("監視を一時停止", "Pause monitoring"));
        let views: Vec<_> = VIEWS
            .iter()
            .map(|&(v, _)| {
                (v, CheckMenuItem::with_id(Command::View(v).id(), view_label(v), true, false, None))
            })
            .collect();
        let themes: Vec<_> = THEMES
            .iter()
            .map(|&(t, _)| {
                (t, CheckMenuItem::with_id(Command::Theme(t).id(), theme_label(t), true, false, None))
            })
            .collect();
        let opacities: Vec<_> = OPACITY_CHOICES
            .iter()
            .map(|&o| {
                let label = if o == DEFAULT_OPACITY {
                    tr_fmt!("{o}%(標準)", "{o}% (default)")
                } else {
                    format!("{o}%")
                };
                (o, CheckMenuItem::with_id(Command::Opacity(o).id(), label, true, false, None))
            })
            .collect();
        let languages: Vec<_> = LANGUAGES
            .iter()
            .map(|&(l, _)| {
                (l, CheckMenuItem::with_id(Command::Language(l).id(), language_label(l), true, false, None))
            })
            .collect();
        // 変換メニュー: グループごとのサブメニュー
        let groups: Vec<Submenu> = CONVERSION_GROUPS
            .iter()
            .map(|group| {
                let items: Vec<MenuItem> =
                    group.items.iter().map(|i| item(Command::Convert(i.conversion), i.label())).collect();
                let refs: Vec<&dyn IsMenuItem> = items.iter().map(|m| m as &dyn IsMenuItem).collect();
                Submenu::with_items(group.name(), true, &refs).unwrap()
            })
            .collect();
        let group_refs: Vec<&dyn IsMenuItem> = groups.iter().map(|g| g as &dyn IsMenuItem).collect();
        let convert_menu =
            Submenu::with_items(tr("テキストを変換してコピー", "Convert text and copy"), true, &group_refs).unwrap();
        let bg_select = item(Command::BackgroundSelect, tr("画像を選択…", "Choose image…"));
        let bg_clear = item(Command::BackgroundClear, tr("解除", "Remove"));
        let font_info = MenuItem::with_id("font:info", "", false, None);
        let font_select = item(Command::FontSelect, tr("フォントを選択…", "Choose font…"));
        let font_reset = item(Command::FontReset, tr("既定に戻す", "Reset to default"));

        let sub = |text: &str, items: Vec<&dyn IsMenuItem>| Submenu::with_items(text, true, &items).unwrap();
        let view_menu = sub(tr("テキスト表示", "Text view"), views.iter().map(|(_, i)| i as &dyn IsMenuItem).collect());
        let theme_menu = sub(tr("テーマ", "Theme"), themes.iter().map(|(_, i)| i as &dyn IsMenuItem).collect());
        let opacity_menu = sub(tr("透明度", "Opacity"), opacities.iter().map(|(_, i)| i as &dyn IsMenuItem).collect());
        let bg_menu = sub(tr("背景画像", "Background image"), vec![&bg_select, &bg_clear]);
        let font_menu = sub(tr("フォント", "Font"), vec![&font_info, &font_select, &font_reset]);
        // 言語メニューの名前は、迷子にならないよう、常に二か国語で書く
        let language_menu = sub("言語 / Language", languages.iter().map(|(_, i)| i as &dyn IsMenuItem).collect());

        let show = item(Command::Show, tr("表示", "Show"));
        let exit = item(Command::Exit, tr("終了", "Quit"));
        let menu = Menu::new();
        let sep1 = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();
        menu.append_items(&[
            &show,
            &toggle,
            &sep1,
            &view_menu,
            &convert_menu,
            &theme_menu,
            &opacity_menu,
            &bg_menu,
            &font_menu,
            &language_menu,
            &sep2,
            &exit,
        ])
        .unwrap();

        let app_menu = AppMenu {
            menu,
            toggle,
            views,
            themes,
            opacities,
            languages,
            bg_clear,
            font_info,
            font_reset,
            convert_menu,
        };
        app_menu.sync(settings, monitoring, false);
        app_menu
    }

    /// 設定や状態に合わせて、チェック・文言・有効/無効を更新する
    pub fn sync(&self, s: &Settings, monitoring: bool, is_text: bool) {
        self.convert_menu.set_enabled(is_text);
        self.toggle.set_text(if monitoring {
            tr("監視を一時停止", "Pause monitoring")
        } else {
            tr("監視を再開", "Resume monitoring")
        });
        for (v, i) in &self.views {
            i.set_checked(*v == s.view);
        }
        for (t, i) in &self.themes {
            i.set_checked(*t == s.theme);
        }
        for (o, i) in &self.opacities {
            i.set_checked(*o == s.opacity);
        }
        for (l, i) in &self.languages {
            i.set_checked(*l == s.language);
        }
        self.bg_clear.set_enabled(s.background.is_some());
        let font = s.font.as_deref().unwrap_or(DEFAULT_FONT);
        self.font_info.set_text(tr_fmt!("現在: {} {}px", "Current: {} {}px", font, s.font_size));
        self.font_reset.set_enabled(s.font.is_some());
    }
}

/// トレイアイコン(黄色い付箋風の 32x32 RGBA)
pub fn tray_icon_rgba() -> (Vec<u8>, u32, u32) {
    const N: usize = 32;
    let mut rgba = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let edge = x < 2 || y < 2 || x >= N - 2 || y >= N - 2;
            let line = (9..N - 9).contains(&x) && (9..N - 6).contains(&y) && (y - 9) % 6 < 2;
            let c = if edge {
                [0xE6, 0xD3, 0x82]
            } else if line {
                [0x8C, 0x7F, 0x4A]
            } else {
                [0xFF, 0xF4, 0xB8]
            };
            rgba.extend_from_slice(&[c[0], c[1], c[2], 0xFF]);
        }
    }
    (rgba, N as u32, N as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_ids_roundtrip() {
        let mut cmds = vec![
            Command::Show,
            Command::ToggleMonitoring,
            Command::BackgroundSelect,
            Command::BackgroundClear,
            Command::FontSelect,
            Command::FontReset,
            Command::Exit,
        ];
        cmds.extend(VIEWS.iter().map(|v| Command::View(v.0)));
        cmds.extend(THEMES.iter().map(|t| Command::Theme(t.0)));
        cmds.extend(OPACITY_CHOICES.iter().map(|&o| Command::Opacity(o)));
        cmds.extend(LANGUAGES.iter().map(|l| Command::Language(l.0)));
        cmds.extend(
            CONVERSION_GROUPS.iter().flat_map(|g| g.items.iter().map(|i| Command::Convert(i.conversion))),
        );
        for c in cmds {
            assert_eq!(Command::from_id(&c.id()), Some(c));
        }
        assert_eq!(Command::from_id("font:info"), None);
    }
}
