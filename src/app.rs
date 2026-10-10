//! egui のアプリ本体(描画と操作)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Label, Rect, ResizeDirection, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, UiBuilder,
    ViewportCommand, pos2, vec2,
};
use eframe::egui::scroll_area::ScrollSource;
use muda::MenuEvent;
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::clipboard;
use crate::core::content::{Content, ImageContent};
use crate::core::settings::{
    DEFAULT_FONT_SIZE, DEFAULT_SIZE, FONT_SIZE_RANGE, Settings, Theme, View,
};
use crate::core::text::{code_rows, detail, display_text, type_label};
use crate::core::theme::{Palette, blend, palette};
use crate::fonts::{DEFAULT_FONT, FontBook};
use crate::menu::{AppMenu, Command, tray_icon_rgba};
use crate::platform;

const PAD: f32 = 16.0;
const HEADER_TOP: f32 = 12.0;
const HEADER_H: f32 = 24.0;
const FOOTER_BOTTOM: f32 = 12.0;
const FOOTER_H: f32 = 18.0;
const RADIUS: f32 = 14.0;
const EDGE: f32 = 6.0;
const SMALL: f32 = 12.0;
/// 背景画像の上に重ねるテーマ色の濃さ(0-255)。大きいほど文字が読みやすい
const SCRIM_ALPHA: u32 = 170;
/// 背景画像は読み込み時にこのサイズ(長辺)まで縮小する
const BACKGROUND_MAX_SIDE: u32 = 1280;
/// フォント選択中にウィンドウを広げる最小サイズ
const PICKER_MIN_SIZE: egui::Vec2 = vec2(420.0, 360.0);

struct BackgroundTexture {
    /// (幅px, 高さpx, テーマ, 透明度, 画像の世代)
    key: (usize, usize, Theme, u32, u64),
    tex: TextureHandle,
}

struct FontPicker {
    filter: String,
    /// 開く前の(フォント, サイズ)。キャンセルで戻す
    original: (Option<String>, i32),
    /// 開く前のウィンドウサイズ(広げた場合のみ)
    restore_size: Option<egui::Vec2>,
}

pub struct App {
    settings: Settings,
    content: Content,
    clipboard_rx: mpsc::Receiver<Content>,
    monitoring: Arc<AtomicBool>,
    menu: AppMenu,
    command_rx: mpsc::Receiver<Command>,
    tray: Option<TrayIcon>,
    fonts: FontBook,
    prepared: bool,
    scroll_reset: bool,
    image_tex: Option<(u64, TextureHandle)>,
    background: Option<image::RgbaImage>,
    background_gen: u64,
    background_tex: Option<BackgroundTexture>,
    font_picker: Option<FontPicker>,
    /// ウィンドウの位置・サイズが変わってから保存するまでの待ち
    geometry_dirty_since: Option<Instant>,
    /// 移動ドラッグ中の(開始時のウィンドウ位置, 開始時のカーソルの画面座標)
    window_drag: Option<(egui::Pos2, (f32, f32))>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, mut settings: Settings) -> App {
        let ctx = cc.egui_ctx.clone();
        let mut fonts = FontBook::new();
        ctx.set_fonts(fonts.definitions(settings.font.as_deref()));
        apply_visuals(&ctx, settings.theme);

        let monitoring = Arc::new(AtomicBool::new(true));
        let clipboard_rx = clipboard::spawn(ctx.clone(), monitoring.clone());

        let menu = AppMenu::new(&settings, true);
        let (tx, command_rx) = mpsc::channel();
        {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if let Some(cmd) = Command::from_id(&e.id.0) {
                    let _ = tx.send(cmd);
                    ctx.request_repaint();
                }
            }));
        }
        {
            let (tx, ctx) = (tx, ctx.clone());
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { .. } = e
                {
                    let _ = tx.send(Command::Show);
                    ctx.request_repaint();
                }
            }));
        }

        let (rgba, w, h) = tray_icon_rgba();
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu.menu.clone()))
            .with_tooltip("ClipSticky")
            .with_menu_on_left_click(cfg!(target_os = "macos"));
        if let Ok(icon) = tray_icon::Icon::from_rgba(rgba, w, h) {
            builder = builder.with_icon(icon);
        }
        let tray = builder.build().ok();

        let background = settings.background.as_deref().and_then(load_background);
        if background.is_none() {
            settings.background = None; // 読み込めなければ忘れる
        }

        App {
            settings,
            content: Content::Unknown,
            clipboard_rx,
            monitoring,
            menu,
            command_rx,
            tray,
            fonts,
            prepared: false,
            scroll_reset: false,
            image_tex: None,
            background,
            background_gen: 0,
            background_tex: None,
            font_picker: None,
            geometry_dirty_since: None,
            window_drag: None,
        }
    }

    fn is_monitoring(&self) -> bool {
        self.monitoring.load(Ordering::Relaxed)
    }

    /// 設定を保存し、メニューと画面に反映する
    fn commit(&mut self, ctx: &egui::Context) {
        self.menu.sync(&self.settings, self.is_monitoring());
        self.settings.save();
        ctx.request_repaint();
    }

    fn apply_font(&mut self, ctx: &egui::Context) {
        ctx.set_fonts(self.fonts.definitions(self.settings.font.as_deref()));
    }

    fn set_background(&mut self, ctx: &egui::Context, path: Option<String>) {
        match path {
            Some(p) => match load_background(&p) {
                Some(img) => {
                    self.background = Some(img);
                    self.settings.background = Some(p);
                }
                None => {
                    rfd::MessageDialog::new()
                        .set_title("ClipSticky")
                        .set_description("画像を読み込めませんでした。\n対応形式: PNG / JPEG / BMP / GIF / WebP")
                        .set_level(rfd::MessageLevel::Warning)
                        .show();
                    return;
                }
            },
            None => {
                self.background = None;
                self.settings.background = None;
            }
        }
        self.background_gen += 1;
        self.background_tex = None;
        self.commit(ctx);
    }

    fn run(&mut self, cmd: Command, ctx: &egui::Context, frame: &eframe::Frame) {
        match cmd {
            Command::Show => platform::show(),
            Command::ToggleMonitoring => {
                let now = !self.is_monitoring();
                self.monitoring.store(now, Ordering::Relaxed);
                self.commit(ctx);
            }
            Command::View(v) => {
                self.settings.view = v;
                self.scroll_reset = true;
                self.commit(ctx);
            }
            Command::Theme(t) => {
                self.settings.theme = t;
                apply_visuals(ctx, t);
                self.commit(ctx);
            }
            Command::Opacity(o) => {
                self.settings.opacity = o;
                self.commit(ctx);
            }
            Command::BackgroundSelect => {
                let picked = rfd::FileDialog::new()
                    .set_parent(frame)
                    .add_filter("画像ファイル", &["png", "jpg", "jpeg", "bmp", "gif", "webp"])
                    .pick_file();
                if let Some(path) = picked {
                    self.set_background(ctx, Some(path.display().to_string()));
                }
            }
            Command::BackgroundClear => self.set_background(ctx, None),
            Command::FontSelect => {
                platform::show();
                let size = ctx.input(|i| i.viewport().inner_rect).map(|r| r.size());
                let restore_size = size.filter(|s| s.x < PICKER_MIN_SIZE.x || s.y < PICKER_MIN_SIZE.y);
                if let Some(s) = size {
                    let target = vec2(s.x.max(PICKER_MIN_SIZE.x), s.y.max(PICKER_MIN_SIZE.y));
                    if restore_size.is_some() {
                        ctx.send_viewport_cmd(ViewportCommand::InnerSize(target));
                    }
                }
                self.font_picker = Some(FontPicker {
                    filter: String::new(),
                    original: (self.settings.font.clone(), self.settings.font_size),
                    restore_size,
                });
                ctx.request_repaint();
            }
            Command::FontReset => {
                self.settings.font = None;
                self.settings.font_size = DEFAULT_FONT_SIZE;
                self.apply_font(ctx);
                self.commit(ctx);
            }
            Command::Exit => {
                self.settings.save();
                self.tray = None; // トレイのアイコンを消してから終了する
                std::process::exit(0);
            }
        }
    }

    /// 初回のウィンドウ設定と、保存位置がないときの初期位置
    fn first_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        platform::prepare_window(frame);
        if self.settings.pos.is_none()
            && let Some(monitor) = ctx.input(|i| i.viewport().monitor_size)
        {
            let size = self.settings.size.unwrap_or(DEFAULT_SIZE);
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(monitor.x - size.0 - 24.0, 24.0)));
        }
    }

    /// ウィンドウの位置・サイズを記録し、落ち着いたら保存する
    fn track_geometry(&mut self, ctx: &egui::Context) {
        let (outer, inner) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
        if let (Some(outer), Some(inner)) = (outer, inner) {
            let pos = Some((outer.min.x, outer.min.y));
            let size = Some((inner.width(), inner.height()));
            // フォント選択中に一時的に広げたサイズは保存しない
            let picking = self.font_picker.is_some();
            if pos != self.settings.pos || (!picking && size != self.settings.size) {
                self.settings.pos = pos;
                if !picking {
                    self.settings.size = size;
                }
                self.geometry_dirty_since = Some(Instant::now());
            }
        }
        if let Some(since) = self.geometry_dirty_since {
            let idle = !ctx.input(|i| i.pointer.any_down());
            if idle && since.elapsed() > Duration::from_millis(500) {
                self.settings.save();
                self.geometry_dirty_since = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
    }

    fn image_texture(cache: &mut Option<(u64, TextureHandle)>, ctx: &egui::Context, img: &ImageContent) -> TextureHandle {
        if let Some((id, tex)) = &*cache
            && *id == img.id
        {
            return tex.clone();
        }
        let max = ctx.input(|i| i.max_texture_side);
        let color = if img.width.max(img.height) > max {
            let src = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.rgba.clone());
            match src {
                Some(src) => {
                    let small = image::imageops::thumbnail(&src, max as u32, max as u32);
                    egui::ColorImage::from_rgba_unmultiplied(
                        [small.width() as usize, small.height() as usize],
                        small.as_raw(),
                    )
                }
                None => egui::ColorImage::example(),
            }
        } else {
            egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.rgba)
        };
        let tex = ctx.load_texture("clipboard-image", color, TextureOptions::LINEAR);
        *cache = Some((img.id, tex.clone()));
        tex
    }

    // ---------- 描画 ----------

    fn paint_card(&mut self, ui: &egui::Ui, rect: Rect, pal: &Palette) {
        let ppp = ui.ctx().pixels_per_point();
        let alpha = (self.settings.opacity * 255 / 100) as u8;
        let radius = CornerRadius::same(RADIUS as u8);
        let painter = ui.painter();

        if let Some(bg) = &self.background {
            let size = ((rect.width() * ppp).round() as usize, (rect.height() * ppp).round() as usize);
            let key = (size.0, size.1, self.settings.theme, self.settings.opacity, self.background_gen);
            let dragging = ui.ctx().input(|i| i.pointer.any_down());
            let stale = match &self.background_tex {
                None => true,
                // サイズ変更中は作り直さず、いまの画像を引き伸ばして使う(重いため)
                Some(t) => t.key != key && !(dragging && t.key.2 == key.2 && t.key.3 == key.3 && t.key.4 == key.4),
            };
            if stale {
                let img = bake_background(bg, size, RADIUS * ppp, pal, alpha);
                let tex = ui.ctx().load_texture("background", img, TextureOptions::LINEAR);
                self.background_tex = Some(BackgroundTexture { key, tex });
            }
            if let Some(t) = &self.background_tex {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(t.tex.id(), rect, uv, Color32::WHITE);
            }
        } else {
            let c = pal.bg;
            painter.rect_filled(rect, radius, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha));
        }
        let b = pal.border;
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(b.r(), b.g(), b.b(), alpha.max(160))),
            StrokeKind::Inside,
        );
    }

    /// ウィンドウ端のリサイズ。端の上にいれば方向を返す
    fn resize_zone(pos: egui::Pos2, rect: Rect) -> Option<ResizeDirection> {
        let near_left = pos.x <= rect.left() + EDGE;
        let near_right = pos.x >= rect.right() - EDGE;
        let near_top = pos.y <= rect.top() + EDGE;
        let near_bottom = pos.y >= rect.bottom() - EDGE;
        // 右下は、つまみ(角)を広めに取る
        let grip = pos.x >= rect.right() - 14.0 && pos.y >= rect.bottom() - 14.0;
        use ResizeDirection::*;
        match (near_left, near_right || grip && near_right, near_top, near_bottom || grip) {
            _ if grip => Some(SouthEast),
            (true, _, true, _) => Some(NorthWest),
            (_, true, true, _) => Some(NorthEast),
            (true, _, _, true) => Some(SouthWest),
            (_, true, _, true) => Some(SouthEast),
            (true, ..) => Some(West),
            (_, true, ..) => Some(East),
            (_, _, true, _) => Some(North),
            (_, _, _, true) => Some(South),
            _ => None,
        }
    }

    fn cursor_for(dir: ResizeDirection) -> CursorIcon {
        use ResizeDirection::*;
        match dir {
            North | South => CursorIcon::ResizeVertical,
            East | West => CursorIcon::ResizeHorizontal,
            NorthWest | SouthEast => CursorIcon::ResizeNwSe,
            NorthEast | SouthWest => CursorIcon::ResizeNeSw,
        }
    }

    fn ui_content(&mut self, ui: &mut egui::Ui, area: Rect, pal: &Palette) {
        let px = self.settings.font_size as f32;
        let body = FontId::proportional(px);
        let reset = std::mem::take(&mut self.scroll_reset);
        let text_color = pal.text;

        let scroll = |reset: bool| {
            let mut s = ScrollArea::vertical()
                .id_salt("content")
                .auto_shrink([false, false])
                .scroll_source(ScrollSource::MOUSE_WHEEL | ScrollSource::SCROLL_BAR);
            if reset {
                s = s.vertical_scroll_offset(0.0);
            }
            s
        };

        match &self.content {
            Content::Image(img) => {
                let tex = Self::image_texture(&mut self.image_tex, ui.ctx(), img);
                let (iw, ih) = (img.width as f32, img.height as f32);
                let scale = (area.width() / iw).min(area.height() / ih);
                let rect = Rect::from_min_size(area.min, vec2(iw * scale, ih * scale));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
            }
            Content::Text(t) if self.settings.view == View::Codes => {
                let (rows, omitted) = code_rows(t);
                let row_h = px + 7.0;
                let k = px / DEFAULT_FONT_SIZE as f32;
                let cols = [0.0, 76.0 * k, 160.0 * k, 268.0 * k];
                let total = rows.len() + usize::from(omitted > 0);
                scroll(reset).show_rows(ui, row_h, total, |ui, range| {
                    for i in range {
                        let (rect, _) = ui.allocate_exact_size(vec2(area.width(), row_h), Sense::hover());
                        let painter = ui.painter();
                        if let Some(row) = rows.get(i) {
                            let cells = [&row.shown, &row.code, &row.utf8, &row.name];
                            for (k, cell) in cells.iter().enumerate() {
                                let color = if k < 2 { text_color } else { pal.muted };
                                painter.text(
                                    pos2(rect.left() + cols[k], rect.center().y),
                                    Align2::LEFT_CENTER,
                                    cell,
                                    body.clone(),
                                    color,
                                );
                            }
                        } else {
                            painter.text(
                                rect.left_center(),
                                Align2::LEFT_CENTER,
                                format!("…(以降 {omitted} 文字は省略)"),
                                body.clone(),
                                pal.muted,
                            );
                        }
                    }
                });
            }
            Content::Text(_) | Content::Files(_) | Content::Error(_) | Content::Unknown => {
                let (text, color) = match &self.content {
                    Content::Text(t) => (display_text(t, self.settings.view), text_color),
                    Content::Files(f) => (f.join("\n"), text_color),
                    Content::Error(e) => (format!("取得エラー: {e}"), text_color),
                    _ => ("未知の形式(または空)です。".to_string(), pal.muted),
                };
                scroll(reset).show(ui, |ui| {
                    ui.set_width(area.width());
                    ui.add(
                        Label::new(RichText::new(text).font(body).color(color))
                            .wrap()
                            .selectable(false),
                    );
                });
            }
        }
    }

    fn ui_font_picker(&mut self, ui: &mut egui::Ui, area: Rect) {
        #[derive(PartialEq)]
        enum Action {
            None,
            Choose(String),
            Ok,
            Cancel,
        }
        let mut action = Action::None;
        let mut size = self.settings.font_size;
        let current = self.settings.font.clone().unwrap_or_else(|| DEFAULT_FONT.to_string());
        let mut picker = self.font_picker.take().expect("font picker is open");

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("絞り込み");
                ui.add(egui::TextEdit::singleline(&mut picker.filter).desired_width(150.0));
                ui.label("サイズ");
                ui.add(
                    egui::DragValue::new(&mut size)
                        .range(FONT_SIZE_RANGE.0..=FONT_SIZE_RANGE.1)
                        .suffix(" px"),
                );
            });
            let filter = picker.filter.to_lowercase();
            let names: Vec<&String> = self
                .fonts
                .families
                .iter()
                .filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter))
                .collect();
            let list_h = (area.height() - 70.0).max(60.0);
            ScrollArea::vertical()
                .id_salt("fonts")
                .max_height(list_h)
                .auto_shrink([false, false])
                .show_rows(ui, 22.0, names.len(), |ui, range| {
                    for i in range {
                        let name = names[i];
                        if ui.selectable_label(*name == current, name.as_str()).clicked() {
                            action = Action::Choose(name.clone());
                        }
                    }
                });
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    action = Action::Ok;
                }
                if ui.button("キャンセル").clicked() {
                    action = Action::Cancel;
                }
            });
        });

        let ctx = ui.ctx().clone();
        if size != self.settings.font_size {
            self.settings.font_size = size;
        }
        match action {
            Action::Choose(name) => {
                self.settings.font = Some(name);
                self.apply_font(&ctx);
                self.font_picker = Some(picker);
            }
            Action::None => self.font_picker = Some(picker),
            Action::Ok => {
                if let Some(s) = picker.restore_size {
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(s));
                }
                self.commit(&ctx);
            }
            Action::Cancel => {
                (self.settings.font, self.settings.font_size) = picker.original.clone();
                self.apply_font(&ctx);
                if let Some(s) = picker.restore_size {
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(s));
                }
                self.commit(&ctx);
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        while let Ok(content) = self.clipboard_rx.try_recv() {
            self.content = content;
            self.scroll_reset = true;
            platform::show();
        }
        while let Ok(cmd) = self.command_rx.try_recv() {
            self.run(cmd, ctx, frame);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.prepared {
            self.prepared = true;
            self.first_frame(&ctx, frame);
        }
        platform::ensure_style();
        platform::clip_to_client();
        self.track_geometry(&ctx);

        let pal = palette(self.settings.theme);
        let full = ui.max_rect();
        self.paint_card(ui, full, &pal);

        // ウィンドウ全体: ドラッグで移動、右クリックでメニュー、端でリサイズ
        let whole = ui.interact(full, egui::Id::new("window"), Sense::click());
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let zone = pointer.and_then(|p| Self::resize_zone(p, full));
        if let Some(dir) = zone {
            ctx.set_cursor_icon(Self::cursor_for(dir));
            if ctx.input(|i| i.pointer.primary_pressed()) {
                ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
            }
        }
        // 移動: ウィジェット(スクロールバーなど)をドラッグしていなければ、どこをドラッグしても動かせる。
        // 子ウィジェットが先にポインタを取ることがあるため、レスポンスではなく生の入力で判定する。
        // 移動は、カーソルの画面座標の差でウィンドウ位置を更新する(winit のドラッグ開始は、この種の窓では効かない)
        let (dragging, down, origin) = ctx.input(|i| {
            (i.pointer.is_decidedly_dragging(), i.pointer.primary_down(), i.pointer.press_origin())
        });
        if !down {
            self.window_drag = None;
        } else if self.window_drag.is_none()
            && dragging
            && ctx.dragged_id().is_none()
            && origin.is_some_and(|o| Self::resize_zone(o, full).is_none())
        {
            let outer = ctx.input(|i| i.viewport().outer_rect);
            let hover = ctx.input(|i| i.pointer.hover_pos());
            if let (Some(outer), Some(cursor), Some(hover), Some(origin)) = (outer, platform::cursor_pos(ctx.pixels_per_point()), hover, origin) {
                // 判定が遅れる分(押してからの移動量)を引いて、押した位置のカーソル座標を基準にする
                let moved = hover - origin;
                self.window_drag = Some((outer.min, (cursor.0 - moved.x, cursor.1 - moved.y)));
            }
        }
        if let (Some((start_pos, start_cursor)), Some(cursor)) = (self.window_drag, platform::cursor_pos(ctx.pixels_per_point())) {
            let delta = vec2(cursor.0 - start_cursor.0, cursor.1 - start_cursor.1);
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(start_pos + delta));
        }
        if whole.secondary_clicked() {
            platform::show_context_menu(frame, &self.menu.menu);
        }

        // ヘッダー: アプリ名・種類・閉じるボタン
        let header = Rect::from_min_size(
            full.min + vec2(PAD, HEADER_TOP),
            vec2(full.width() - PAD * 2.0 - 28.0, HEADER_H),
        );
        let painter = ui.painter();
        let small = FontId::proportional(SMALL);
        let name_color = blend(pal.muted, pal.bg, 55);
        let name_rect = painter.text(header.left_center(), Align2::LEFT_CENTER, "ClipSticky", small.clone(), name_color);
        let label = if self.font_picker.is_some() {
            "フォント"
        } else {
            match (&self.content, self.settings.view) {
                (Content::Text(_), View::Controls) => "テキスト · 制御文字",
                (Content::Text(_), View::Codes) => "テキスト · 文字コード",
                (c, _) => type_label(c),
            }
        };
        painter.with_clip_rect(header).text(
            pos2(name_rect.right() + 10.0, header.center().y),
            Align2::LEFT_CENTER,
            label,
            small.clone(),
            pal.muted,
        );

        let close = Rect::from_center_size(pos2(full.right() - 12.0 - 14.0, full.top() + 10.0 + 14.0), vec2(28.0, 28.0));
        let close_resp = ui.interact(close, egui::Id::new("close"), Sense::click());
        let close_color = if close_resp.hovered() { pal.text } else { pal.muted };
        ui.painter().text(close.center(), Align2::CENTER_CENTER, "×", FontId::proportional(22.0), close_color);
        if close_resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if close_resp.clicked() {
            platform::hide();
        }

        // フッター: 状態インジケーターと詳細・リサイズつまみ
        let footer = Rect::from_min_max(
            pos2(full.left() + PAD, full.bottom() - FOOTER_BOTTOM - FOOTER_H),
            pos2(full.right() - PAD - 14.0, full.bottom() - FOOTER_BOTTOM),
        );
        let monitoring = self.is_monitoring();
        let dot = if monitoring { Color32::from_rgb(0x34, 0xC7, 0x59) } else { Color32::from_rgb(0xFF, 0x9F, 0x0A) };
        let painter = ui.painter();
        painter.circle_filled(pos2(footer.left() + 4.0, footer.center().y), 4.0, dot);
        let status = if monitoring { "監視中" } else { "一時停止中" };
        let info = match detail(&self.content) {
            d if d.is_empty() => status.to_string(),
            d => format!("{status}  ·  {d}"),
        };
        painter.with_clip_rect(footer).text(
            pos2(footer.left() + 16.0, footer.center().y),
            Align2::LEFT_CENTER,
            info,
            small,
            pal.muted,
        );
        for k in 1..=3 {
            let d = 4.0 * k as f32;
            let (x, y) = (full.right() - 6.0, full.bottom() - 6.0);
            painter.line_segment([pos2(x, y - d), pos2(x - d, y)], Stroke::new(1.0, pal.muted));
        }

        // コンテンツ
        let area = Rect::from_min_max(
            pos2(full.left() + PAD, header.bottom() + 8.0),
            pos2(full.right() - PAD, footer.top() - 8.0),
        );
        if area.width() > 10.0 && area.height() > 10.0 {
            ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                ui.set_clip_rect(area.intersect(ui.clip_rect()));
                if self.font_picker.is_some() {
                    self.ui_font_picker(ui, area);
                } else {
                    self.ui_content(ui, area, &pal);
                }
            });
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.save();
    }
}

fn apply_visuals(ctx: &egui::Context, theme: Theme) {
    let mut v = if theme == Theme::Dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = Color32::TRANSPARENT;
    ctx.set_visuals(v);
}

fn load_background(path: &str) -> Option<image::RgbaImage> {
    let mut img = image::open(path).ok()?;
    if img.width().max(img.height()) > BACKGROUND_MAX_SIDE {
        img = img.thumbnail(BACKGROUND_MAX_SIDE, BACKGROUND_MAX_SIDE);
    }
    let mut rgba = img.to_rgba8();
    // 透過部分は白と合成する
    for px in rgba.pixels_mut() {
        let a = px[3] as u32;
        for c in &mut px.0[..3] {
            *c = ((*c as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
        px[3] = 255;
    }
    Some(rgba)
}

/// 背景画像を cover(縦横比を保ち、はみ出た分は中央でトリミング)で `size` に合わせ、
/// テーマ色を重ね、角丸の外側を透明にしたテクスチャ用の画像を作る
fn bake_background(src: &image::RgbaImage, size: (usize, usize), radius: f32, pal: &Palette, alpha: u8) -> egui::ColorImage {
    let (w, h) = (size.0.max(1), size.1.max(1));
    let (iw, ih) = src.dimensions();
    let scale = (w as f32 / iw as f32).max(h as f32 / ih as f32);
    let cw = ((w as f32 / scale).round() as u32).clamp(1, iw);
    let ch = ((h as f32 / scale).round() as u32).clamp(1, ih);
    let cropped = image::imageops::crop_imm(src, (iw - cw) / 2, (ih - ch) / 2, cw, ch).to_image();
    let resized = image::imageops::resize(&cropped, w as u32, h as u32, image::imageops::FilterType::Triangle);

    let (br, bg, bb) = (pal.bg.r() as u32, pal.bg.g() as u32, pal.bg.b() as u32);
    let mix = |c: u8, b: u32| ((c as u32 * (255 - SCRIM_ALPHA) + b * SCRIM_ALPHA) / 255) as u8;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut out = Vec::with_capacity(w * h * 4);
    for (x, y, px) in resized.enumerate_pixels() {
        // 角丸の距離場でアンチエイリアスしたカバレッジを求める
        let dx = ((x as f32 + 0.5 - cx).abs() - (cx - radius)).max(0.0);
        let dy = ((y as f32 + 0.5 - cy).abs() - (cy - radius)).max(0.0);
        let coverage = (0.5 - ((dx * dx + dy * dy).sqrt() - radius)).clamp(0.0, 1.0);
        out.extend_from_slice(&[mix(px[0], br), mix(px[1], bg), mix(px[2], bb), (alpha as f32 * coverage) as u8]);
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &out)
}
