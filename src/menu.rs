//! トレイとウィンドウの右クリックで共通に使うメニュー

use muda::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

use crate::core::convert::{CONVERSION_GROUPS, Conversion, conversion_by_id, conversion_info};
use crate::core::settings::{
    OPACITY_CHOICES, DEFAULT_OPACITY, Settings, THEMES, Theme, VIEWS, View,
};
use crate::fonts::DEFAULT_FONT;

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
    bg_clear: MenuItem,
    font_info: MenuItem,
    font_reset: MenuItem,
    /// 「テキストを変換してコピー」。内容がテキストのときだけ使える
    convert_menu: Submenu,
}

fn item(cmd: Command, text: &str) -> MenuItem {
    MenuItem::with_id(cmd.id(), text, true, None)
}

impl AppMenu {
    pub fn new(settings: &Settings, monitoring: bool) -> AppMenu {
        let toggle = item(Command::ToggleMonitoring, "監視を一時停止");
        let views: Vec<_> = VIEWS
            .iter()
            .map(|&(v, _, name)| (v, CheckMenuItem::with_id(Command::View(v).id(), name, true, false, None)))
            .collect();
        let themes: Vec<_> = THEMES
            .iter()
            .map(|&(t, _, name)| (t, CheckMenuItem::with_id(Command::Theme(t).id(), name, true, false, None)))
            .collect();
        let opacities: Vec<_> = OPACITY_CHOICES
            .iter()
            .map(|&o| {
                let label = if o == DEFAULT_OPACITY { format!("{o}%(標準)") } else { format!("{o}%") };
                (o, CheckMenuItem::with_id(Command::Opacity(o).id(), label, true, false, None))
            })
            .collect();
        // 変換メニュー: グループごとのサブメニュー
        let groups: Vec<Submenu> = CONVERSION_GROUPS
            .iter()
            .map(|(name, infos)| {
                let items: Vec<MenuItem> =
                    infos.iter().map(|i| item(Command::Convert(i.conversion), i.label)).collect();
                let refs: Vec<&dyn IsMenuItem> = items.iter().map(|m| m as &dyn IsMenuItem).collect();
                Submenu::with_items(*name, true, &refs).unwrap()
            })
            .collect();
        let group_refs: Vec<&dyn IsMenuItem> = groups.iter().map(|g| g as &dyn IsMenuItem).collect();
        let convert_menu = Submenu::with_items("テキストを変換してコピー", true, &group_refs).unwrap();
        let bg_select = item(Command::BackgroundSelect, "画像を選択…");
        let bg_clear = item(Command::BackgroundClear, "解除");
        let font_info = MenuItem::with_id("font:info", "", false, None);
        let font_select = item(Command::FontSelect, "フォントを選択…");
        let font_reset = item(Command::FontReset, "既定に戻す");

        let sub = |text: &str, items: Vec<&dyn IsMenuItem>| Submenu::with_items(text, true, &items).unwrap();
        let view_menu = sub("テキスト表示", views.iter().map(|(_, i)| i as &dyn IsMenuItem).collect());
        let theme_menu = sub("テーマ", themes.iter().map(|(_, i)| i as &dyn IsMenuItem).collect());
        let opacity_menu = sub("透明度", opacities.iter().map(|(_, i)| i as &dyn IsMenuItem).collect());
        let bg_menu = sub("背景画像", vec![&bg_select, &bg_clear]);
        let font_menu = sub("フォント", vec![&font_info, &font_select, &font_reset]);

        let show = item(Command::Show, "表示");
        let exit = item(Command::Exit, "終了");
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
            &sep2,
            &exit,
        ])
        .unwrap();

        let app_menu =
            AppMenu { menu, toggle, views, themes, opacities, bg_clear, font_info, font_reset, convert_menu };
        app_menu.sync(settings, monitoring, false);
        app_menu
    }

    /// 設定や状態に合わせて、チェック・文言・有効/無効を更新する
    pub fn sync(&self, s: &Settings, monitoring: bool, is_text: bool) {
        self.convert_menu.set_enabled(is_text);
        self.toggle.set_text(if monitoring { "監視を一時停止" } else { "監視を再開" });
        for (v, i) in &self.views {
            i.set_checked(*v == s.view);
        }
        for (t, i) in &self.themes {
            i.set_checked(*t == s.theme);
        }
        for (o, i) in &self.opacities {
            i.set_checked(*o == s.opacity);
        }
        self.bg_clear.set_enabled(s.background.is_some());
        self.font_info
            .set_text(format!("現在: {} {}px", s.font.as_deref().unwrap_or(DEFAULT_FONT), s.font_size));
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
        cmds.extend(
            CONVERSION_GROUPS.iter().flat_map(|(_, items)| items.iter().map(|i| Command::Convert(i.conversion))),
        );
        for c in cmds {
            assert_eq!(Command::from_id(&c.id()), Some(c));
        }
        assert_eq!(Command::from_id("font:info"), None);
    }
}
