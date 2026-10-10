#![windows_subsystem = "windows"]

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::{
    DragAcceptFiles, DragFinish, DragQueryFileW, HDROP, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
    NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, PWSTR, w};

const APP_NAME: &str = "ClipSticky";
const DEFAULT_OPACITY: u32 = 96;
const OPACITY_CHOICES: [u32; 7] = [100, 96, 90, 80, 70, 60, 50];
/// 背景画像の上に重ねるテーマ色の濃さ(0-255)。大きいほど文字が読みやすい
const SCRIM_ALPHA: u8 = 170;
/// 背景画像は読み込み時にこのサイズ(長辺)まで縮小する
const BACKGROUND_MAX_SIDE: u32 = 1280;
const DEFAULT_SIZE: (i32, i32) = (380, 220); // DIP
const MIN_SIZE: (i32, i32) = (240, 120); // DIP

// ---------- テーマ ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    Yellow,
    Light,
    Dark,
}

const THEMES: [(Theme, &str, &str); 3] = [
    (Theme::Yellow, "yellow", "イエロー"),
    (Theme::Light, "light", "ライト"),
    (Theme::Dark, "dark", "ダーク"),
];

// ---------- テキストの表示形式 ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Normal,
    Controls,
    Codes,
}

const VIEWS: [(View, &str, &str); 3] = [
    (View::Normal, "normal", "通常"),
    (View::Controls, "controls", "制御文字を表示"),
    (View::Codes, "codes", "文字コード一覧"),
];

/// 表示するテキストの上限(これを超える分は省略する。描画が重くなるため)
const MAX_DISPLAY_CHARS: usize = 20_000;
/// 文字コード一覧で表示する文字数の上限
const MAX_CODE_ROWS: usize = 1_000;

const CONTROL_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR", "SO",
    "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS",
    "GS", "RS", "US",
];

/// 目に見えない文字・空白・制御文字の(表示用の記号, 名前)。普通の文字なら None
fn special_char(c: char) -> Option<(String, String)> {
    let code = c as u32;
    let pair = |sym: &str, name: &str| Some((sym.to_string(), name.to_string()));
    match code {
        0x09 => pair("⟦TAB⟧", "HT (TAB)"),
        0x00..=0x1F => {
            let name = CONTROL_NAMES[code as usize];
            Some((format!("⟦{name}⟧"), name.to_string()))
        }
        0x20 => pair("·", "SPACE"),
        0x7F => pair("⟦DEL⟧", "DEL"),
        0x80..=0x9F => Some((format!("⟦{code:02X}⟧"), "C1 CONTROL".to_string())),
        0xA0 => pair("⟦NBSP⟧", "NO-BREAK SPACE"),
        0xAD => pair("⟦SHY⟧", "SOFT HYPHEN"),
        0x2028 => pair("⟦LS⟧", "LINE SEPARATOR"),
        0x2029 => pair("⟦PS⟧", "PARAGRAPH SEPARATOR"),
        0x200B => pair("⟦ZWSP⟧", "ZERO WIDTH SPACE"),
        0x200C => pair("⟦ZWNJ⟧", "ZERO WIDTH NON-JOINER"),
        0x200D => pair("⟦ZWJ⟧", "ZERO WIDTH JOINER"),
        0x200E => pair("⟦LRM⟧", "LEFT-TO-RIGHT MARK"),
        0x200F => pair("⟦RLM⟧", "RIGHT-TO-LEFT MARK"),
        0xFEFF => pair("⟦BOM⟧", "BOM / ZWNBSP"),
        0x3000 => pair("□", "IDEOGRAPHIC SPACE"),
        _ => None,
    }
}

/// 制御文字を記号に置き換える。改行・タブは記号の後ろで実際にも改行/タブする
fn show_controls(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                out.push_str("⟦CR⟧⟦LF⟧\n");
            }
            '\n' => out.push_str("⟦LF⟧\n"),
            '\t' => out.push_str("⟦TAB⟧\t"),
            _ => match special_char(c) {
                Some((sym, _)) => out.push_str(&sym),
                None => out.push(c),
            },
        }
    }
    out
}

/// 文字コード一覧の 1 行分
struct CodeRow {
    shown: String,
    code: String,
    utf8: String,
    name: String,
}

/// 1 文字 1 行で「文字 / Unicode コードポイント / UTF-8 バイト列 / 名前」を作る。
/// 2 つ目は、上限を超えて省略した文字数
fn code_rows(text: &str) -> (Vec<CodeRow>, usize) {
    let rows = text
        .chars()
        .take(MAX_CODE_ROWS)
        .map(|c| {
            let special = special_char(c);
            let mut buf = [0u8; 4];
            let utf8: Vec<String> = c.encode_utf8(&mut buf).bytes().map(|b| format!("{b:02X}")).collect();
            CodeRow {
                shown: special.as_ref().map_or_else(|| c.to_string(), |(s, _)| s.clone()),
                code: format!("U+{:04X}", c as u32),
                utf8: utf8.join(" "),
                name: special.map(|(_, n)| n).unwrap_or_default(),
            }
        })
        .collect();
    (rows, text.chars().count().saturating_sub(MAX_CODE_ROWS))
}

/// 通常 / 制御文字表示のときに描画するテキストを作る(長すぎる分は省略)
fn display_text(text: &str, view: View) -> String {
    let total = text.chars().count();
    let body: String = text.chars().take(MAX_DISPLAY_CHARS).collect();
    let mut shown = if view == View::Controls { show_controls(&body) } else { body };
    if total > MAX_DISPLAY_CHARS {
        shown.push_str(&format!("\n…(以降 {} 文字は省略)", total - MAX_DISPLAY_CHARS));
    }
    shown
}

struct Palette {
    bg: COLORREF,
    text: COLORREF,
    muted: COLORREF,
    border: COLORREF,
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// `fg` を `bg` に `percent`% の濃さで重ねた色
fn blend(fg: COLORREF, bg: COLORREF, percent: u32) -> COLORREF {
    let mix = |f: u32, b: u32| ((f * percent + b * (100 - percent)) / 100) as u8;
    let (f, b) = (fg.0, bg.0);
    rgb(
        mix(f & 0xFF, b & 0xFF),
        mix((f >> 8) & 0xFF, (b >> 8) & 0xFF),
        mix((f >> 16) & 0xFF, (b >> 16) & 0xFF),
    )
}

fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Yellow => Palette {
            bg: rgb(0xFF, 0xF4, 0xB8),
            text: rgb(0x3A, 0x32, 0x14),
            muted: rgb(0x8C, 0x7F, 0x4A),
            border: rgb(0xE6, 0xD3, 0x82),
        },
        Theme::Light => Palette {
            bg: rgb(0xFA, 0xFA, 0xFB),
            text: rgb(0x1C, 0x1C, 0x1E),
            muted: rgb(0x8A, 0x8A, 0x93),
            border: rgb(0xDD, 0xDD, 0xE3),
        },
        Theme::Dark => Palette {
            bg: rgb(0x20, 0x21, 0x24),
            text: rgb(0xEC, 0xEC, 0xF0),
            muted: rgb(0x98, 0x9A, 0xA4),
            border: rgb(0x3C, 0x3E, 0x44),
        },
    }
}

// ---------- 状態 ----------

struct Dib {
    data: Vec<u8>,
    width: i32,
    height: i32,
    bits_offset: usize,
}

enum Content {
    Text(String),
    Image(Dib),
    Files(String),
    Unknown,
    Error(String),
}

/// 背景画像(上から下へ並んだ 32bit BGRA)
struct Background {
    width: i32,
    height: i32,
    bgra: Vec<u8>,
}

struct State {
    content: Content,
    monitoring: bool,
    scroll: i32,
    max_scroll: i32,
    background: Option<Background>,
    /// 現在のウィンドウサイズに合わせて拡縮済みの背景(幅, 高さ, ビットマップ)
    background_cache: Option<(i32, i32, HBITMAP)>,
}

struct Settings {
    pos: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
    opacity: u32,
    theme: Theme,
    view: View,
    background: Option<String>,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            content: Content::Unknown,
            monitoring: true,
            scroll: 0,
            max_scroll: 0,
            background: None,
            background_cache: None,
        })
    };
    static SETTINGS: RefCell<Settings> = const {
        RefCell::new(Settings {
            pos: None,
            size: None,
            opacity: DEFAULT_OPACITY,
            theme: Theme::Yellow,
            view: View::Normal,
            background: None,
        })
    };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

// ---------- 設定ファイル ----------

fn settings_path() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join("ClipSticky");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("settings.txt"))
}

/// 保存形式は `key=value` の行。未知のキーや壊れた行は無視する
fn load_settings() {
    let Some(text) = settings_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return;
    };
    SETTINGS.with(|s| {
        let mut s = s.borrow_mut();
        let (mut left, mut top, mut width, mut height) = (None, None, None, None);
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "left" => left = value.parse::<i32>().ok(),
                "top" => top = value.parse::<i32>().ok(),
                "width" => width = value.parse::<i32>().ok(),
                "height" => height = value.parse::<i32>().ok(),
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
                _ => {}
            }
        }
        s.pos = left.zip(top);
        s.size = width.zip(height).filter(|&(w, h)| w > 0 && h > 0);
    });
}

unsafe fn save_settings(hwnd: HWND) {
    let mut rc = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let Some(path) = settings_path() else { return };
    let text = SETTINGS.with(|s| {
        let s = s.borrow();
        let theme = THEMES.iter().find(|(t, _, _)| *t == s.theme).map_or("yellow", |t| t.1);
        let view = VIEWS.iter().find(|(v, _, _)| *v == s.view).map_or("normal", |v| v.1);
        let mut text = format!(
            "left={}\ntop={}\nwidth={}\nheight={}\nopacity={}\ntheme={}\nview={}\n",
            rc.left,
            rc.top,
            rc.right - rc.left,
            rc.bottom - rc.top,
            s.opacity,
            theme,
            view
        );
        if let Some(bg) = &s.background {
            text.push_str(&format!("background={bg}\n"));
        }
        text
    });
    let _ = std::fs::write(path, text);
}

unsafe fn apply_opacity(hwnd: HWND, percent: u32) {
    SETTINGS.with(|s| s.borrow_mut().opacity = percent);
    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), (percent * 255 / 100) as u8, LWA_ALPHA);
    }
}

/// テーマに合わせて DWM の枠線色を更新し、再描画する
unsafe fn apply_theme(hwnd: HWND, theme: Theme) {
    SETTINGS.with(|s| s.borrow_mut().theme = theme);
    unsafe {
        let border = palette(theme).border.0;
        // DWMWA_BORDER_COLOR (Windows 11 以降。未対応の OS ではエラーになるだけ)
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(34),
            &border as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

// ---------- 背景画像 ----------

fn load_background(path: &str) -> Option<Background> {
    let mut img = image::open(path).ok()?;
    if img.width().max(img.height()) > BACKGROUND_MAX_SIDE {
        img = img.thumbnail(BACKGROUND_MAX_SIDE, BACKGROUND_MAX_SIDE);
    }
    let rgba = img.to_rgba8();
    let (width, height) = (rgba.width() as i32, rgba.height() as i32);
    let mut bgra = Vec::with_capacity(rgba.as_raw().len());
    for px in rgba.pixels() {
        // 透過部分は白と合成する
        let a = px[3] as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        bgra.extend_from_slice(&[blend(px[2]), blend(px[1]), blend(px[0]), 255]);
    }
    Some(Background { width, height, bgra })
}

/// `Some(path)` で画像を設定、`None` で解除。読み込みに失敗したら false
unsafe fn set_background(hwnd: HWND, path: Option<String>) -> bool {
    let background = match &path {
        Some(p) => match load_background(p) {
            Some(bg) => Some(bg),
            None => return false,
        },
        None => None,
    };
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.background = background;
        if let Some((_, _, bmp)) = s.background_cache.take() {
            unsafe {
                let _ = DeleteObject(bmp.into());
            }
        }
    });
    SETTINGS.with(|s| s.borrow_mut().background = path);
    unsafe {
        save_settings(hwnd);
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    true
}

unsafe fn pick_image(owner: HWND) -> Option<String> {
    let mut file = [0u16; 1024];
    let filter: Vec<u16> =
        "画像ファイル\0*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp\0すべてのファイル\0*.*\0\0"
            .encode_utf16()
            .collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_EXPLORER | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        let len = file.iter().position(|&c| c == 0)?;
        Some(String::from_utf16_lossy(&file[..len]))
    } else {
        None
    }
}

unsafe fn choose_background(hwnd: HWND, path: String) {
    if !unsafe { set_background(hwnd, Some(path)) } {
        unsafe {
            let _ = MessageBoxW(
                Some(hwnd),
                w!("画像を読み込めませんでした。\n対応形式: PNG / JPEG / BMP / GIF / WebP"),
                w!("ClipSticky"),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
}

// ---------- クリップボード ----------

unsafe fn read_dib() -> Option<Dib> {
    unsafe {
        let h = GetClipboardData(CF_DIB.0 as u32).ok()?;
        let hg = HGLOBAL(h.0);
        let size = GlobalSize(hg);
        let ptr = GlobalLock(hg) as *const u8;
        if ptr.is_null() || size < 40 {
            if !ptr.is_null() {
                let _ = GlobalUnlock(hg);
            }
            return None;
        }
        let data = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(hg);

        let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        let i32_at = |o: usize| i32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        let header = u32_at(0) as usize;
        let width = i32_at(4);
        let height = i32_at(8).abs();
        let bit_count = u16::from_le_bytes([data[14], data[15]]) as u32;
        let compression = u32_at(16);
        let clr_used = u32_at(32);
        let colors = if clr_used > 0 {
            clr_used
        } else if bit_count <= 8 {
            1 << bit_count
        } else {
            0
        } as usize;
        let mut bits_offset = header + colors * 4;
        if compression == 3 && header == 40 {
            bits_offset += 12; // BI_BITFIELDS のカラーマスク
        }
        (width > 0 && height > 0 && bits_offset < data.len()).then_some(Dib {
            data,
            width,
            height,
            bits_offset,
        })
    }
}

unsafe fn read_text() -> Option<String> {
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        let hg = HGLOBAL(h.0);
        let ptr = GlobalLock(hg) as *const u16;
        if ptr.is_null() {
            return None;
        }
        let max = GlobalSize(hg) / 2;
        let slice = std::slice::from_raw_parts(ptr, max);
        let len = slice.iter().position(|&c| c == 0).unwrap_or(max);
        let s = String::from_utf16_lossy(&slice[..len]);
        let _ = GlobalUnlock(hg);
        Some(s)
    }
}

unsafe fn read_files() -> Option<String> {
    unsafe {
        let h = GetClipboardData(CF_HDROP.0 as u32).ok()?;
        let drop = HDROP(h.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        let mut lines = Vec::new();
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let n = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            lines.push(String::from_utf16_lossy(&buf[..n]));
        }
        Some(lines.join("\r\n"))
    }
}

fn read_clipboard() -> Content {
    unsafe {
        // 他プロセスがクリップボードをロックしている場合はリトライする
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if !opened {
            return Content::Error("クリップボードが他のプロセスで使用中です。".into());
        }
        let content = if IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok()
            && let Some(dib) = read_dib()
        {
            Content::Image(dib)
        } else if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok()
            && let Some(text) = read_text()
        {
            Content::Text(text)
        } else if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok()
            && let Some(files) = read_files()
        {
            Content::Files(files)
        } else {
            Content::Unknown
        };
        let _ = CloseClipboard();
        content
    }
}

fn refresh_clipboard() {
    let content = read_clipboard();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.content = content;
        s.scroll = 0;
    });
}

// ---------- レイアウトと描画 ----------

struct Layout {
    header: RECT,
    close: RECT,
    content: RECT,
    footer: RECT,
}

fn layout(w: i32, h: i32, dpi: u32) -> Layout {
    let s = |v: i32| v * dpi as i32 / 96;
    let pad = s(16);
    let close = RECT { left: w - s(12) - s(28), top: s(10), right: w - s(12), bottom: s(10) + s(28) };
    let header = RECT { left: pad, top: s(12), right: close.left - s(4), bottom: s(12) + s(24) };
    let footer = RECT { left: pad, top: h - s(12) - s(18), right: w - pad - s(14), bottom: h - s(12) };
    let content = RECT {
        left: pad,
        top: header.bottom + s(8),
        right: w - pad,
        bottom: footer.top - s(8),
    };
    Layout { header, close, content, footer }
}

fn in_rect(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

unsafe fn make_font(dpi: u32, px: i32, bold: bool) -> HFONT {
    unsafe {
        CreateFontW(
            -(px * dpi as i32 / 96),
            0,
            0,
            0,
            if bold { FW_SEMIBOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Yu Gothic UI"),
        )
    }
}

unsafe fn draw_text(hdc: HDC, text: &str, rc: &mut RECT, flags: DRAW_TEXT_FORMAT) {
    if text.is_empty() {
        return; // 空の文字列を DrawTextW に渡すとアクセス違反になることがある
    }
    let mut buf = wide(text);
    unsafe { DrawTextW(hdc, &mut buf, rc, flags) };
}

fn type_label(content: &Content) -> &'static str {
    match content {
        Content::Text(_) => "テキスト",
        Content::Image(_) => "画像",
        Content::Files(_) => "ファイル",
        Content::Unknown => "クリップボード",
        Content::Error(_) => "エラー",
    }
}

fn detail(content: &Content) -> String {
    match content {
        Content::Text(t) => format!(
            "{} 文字 · {} 行 · UTF-8 {}B",
            t.chars().count(),
            t.lines().count(),
            t.len()
        ),
        Content::Image(d) => format!("{}×{}", d.width, d.height),
        Content::Files(t) => format!("{} 件", t.lines().count()),
        Content::Unknown | Content::Error(_) => String::new(),
    }
}

/// 背景画像を cover(縦横比を保ち、はみ出た分は中央でトリミング)で描き、テーマ色を重ねる
unsafe fn draw_background(mem: HDC, st: &mut State, w: i32, h: i32, pal: &Palette) {
    unsafe {
        let Some(bg) = &st.background else { return };
        if st.background_cache.map(|(cw, ch, _)| (cw, ch)) != Some((w, h)) {
            if let Some((_, _, old)) = st.background_cache.take() {
                let _ = DeleteObject(old.into());
            }
            let bmp = CreateCompatibleBitmap(mem, w, h);
            let dc = CreateCompatibleDC(Some(mem));
            let old = SelectObject(dc, bmp.into());

            let scale = (w as f64 / bg.width as f64).max(h as f64 / bg.height as f64);
            let sw = ((w as f64 / scale).round() as i32).clamp(1, bg.width);
            let sh = ((h as f64 / scale).round() as i32).clamp(1, bg.height);
            let (sx, sy) = ((bg.width - sw) / 2, (bg.height - sh) / 2);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: bg.width,
                    biHeight: -bg.height, // 上から下
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            SetStretchBltMode(dc, HALFTONE);
            let _ = SetBrushOrgEx(dc, 0, 0, None);
            StretchDIBits(
                dc,
                0,
                0,
                w,
                h,
                sx,
                sy,
                sw,
                sh,
                Some(bg.bgra.as_ptr() as *const _),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            SelectObject(dc, old);
            let _ = DeleteDC(dc);
            st.background_cache = Some((w, h, bmp));
        }
        let (_, _, bmp) = st.background_cache.unwrap();
        let dc = CreateCompatibleDC(Some(mem));
        let old = SelectObject(dc, bmp.into());
        let _ = BitBlt(mem, 0, 0, w, h, Some(dc), 0, 0, SRCCOPY);
        SelectObject(dc, old);
        let _ = DeleteDC(dc);

        // 文字を読みやすくするため、テーマ色を半透明で重ねる
        let scrim_dc = CreateCompatibleDC(Some(mem));
        let scrim_bmp = CreateCompatibleBitmap(mem, 1, 1);
        let old = SelectObject(scrim_dc, scrim_bmp.into());
        let brush = CreateSolidBrush(pal.bg);
        FillRect(scrim_dc, &RECT { left: 0, top: 0, right: 1, bottom: 1 }, brush);
        let _ = DeleteObject(brush.into());
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: SCRIM_ALPHA,
            AlphaFormat: 0,
        };
        let _ = AlphaBlend(mem, 0, 0, w, h, scrim_dc, 0, 0, 1, 1, blend);
        SelectObject(scrim_dc, old);
        let _ = DeleteObject(scrim_bmp.into());
        let _ = DeleteDC(scrim_dc);
    }
}

unsafe fn fill_ellipse(hdc: HDC, rc: &RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush.into());
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let _ = Ellipse(hdc, rc.left, rc.top, rc.right, rc.bottom);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(brush.into());
    }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let (w, h) = (client.right, client.bottom);
        let dpi = GetDpiForWindow(hwnd);
        let s = |v: i32| v * dpi as i32 / 96;
        let l = layout(w, h, dpi);
        let (theme, view) = SETTINGS.with(|s| {
            let s = s.borrow();
            (s.theme, s.view)
        });
        let pal = palette(theme);

        // ダブルバッファ
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old_bmp = SelectObject(mem, bmp.into());

        let bg = CreateSolidBrush(pal.bg);
        FillRect(mem, &client, bg);
        let _ = DeleteObject(bg.into());

        SetBkMode(mem, TRANSPARENT);
        let body = make_font(dpi, 15, false);
        let small = make_font(dpi, 12, false);
        let label = make_font(dpi, 12, true);
        let glyph = make_font(dpi, 22, false);

        STATE.with(|st| {
            let mut st = st.borrow_mut();
            draw_background(mem, &mut st, w, h, &pal);

            // ヘッダー: 種類ラベルと閉じるボタン
            // アプリ名(控えめに、背景に溶け込む薄い色で)
            let old_font = SelectObject(mem, small.into());
            SetTextColor(mem, blend(pal.muted, pal.bg, 55));
            let mut name_rc = l.header;
            draw_text(mem, APP_NAME, &mut name_rc, DT_SINGLELINE | DT_NOPREFIX | DT_CALCRECT);
            let mut r = RECT { top: l.header.top, bottom: l.header.bottom, ..name_rc };
            draw_text(mem, APP_NAME, &mut r, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);

            // 種類ラベル(アプリ名の右隣)
            SelectObject(mem, label.into());
            SetTextColor(mem, pal.muted);
            let mut r = RECT { left: name_rc.right + s(10), ..l.header };
            let header = match (&st.content, view) {
                (Content::Text(_), View::Controls) => "テキスト · 制御文字",
                (Content::Text(_), View::Codes) => "テキスト · 文字コード",
                (content, _) => type_label(content),
            };
            draw_text(mem, header, &mut r, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
            SelectObject(mem, glyph.into());
            let mut r = l.close;
            draw_text(mem, "×", &mut r, DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_NOPREFIX);

            // コンテンツ(クリップして描画)
            SelectObject(mem, body.into());
            let area = l.content;
            let saved = SaveDC(mem);
            let _ = IntersectClipRect(mem, area.left, area.top, area.right, area.bottom);
            let avail_w = area.right - area.left;
            let avail_h = area.bottom - area.top;
            let top = area.top - st.scroll;
            let content_h;
            SetTextColor(mem, pal.text);
            match &st.content {
                Content::Text(t) if view == View::Codes => {
                    // 文字コード一覧: 列ごとに描く(画面外の行は飛ばす)
                    let (rows, omitted) = code_rows(t);
                    let mut tm = TEXTMETRICW::default();
                    let _ = GetTextMetricsW(mem, &mut tm);
                    let row_h = tm.tmHeight + s(4);
                    let cols = [0, s(76), s(160), s(268)];
                    let flags = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS;
                    for (i, row) in rows.iter().enumerate() {
                        let y = top + i as i32 * row_h;
                        if y + row_h < area.top || y > area.bottom {
                            continue;
                        }
                        let cells = [&row.shown, &row.code, &row.utf8, &row.name];
                        for (k, cell) in cells.iter().enumerate() {
                            SetTextColor(mem, if k < 2 { pal.text } else { pal.muted });
                            let right = if k < 3 { area.left + cols[k + 1] - s(6) } else { area.right };
                            let mut r = RECT { left: area.left + cols[k], top: y, right, bottom: y + row_h };
                            draw_text(mem, cell, &mut r, flags);
                        }
                    }
                    let mut total_rows = rows.len() as i32;
                    if omitted > 0 {
                        SetTextColor(mem, pal.muted);
                        let y = top + total_rows * row_h;
                        let mut r = RECT { left: area.left, top: y, right: area.right, bottom: y + row_h };
                        draw_text(mem, &format!("…(以降 {omitted} 文字は省略)"), &mut r, flags);
                        total_rows += 1;
                    }
                    content_h = total_rows * row_h;
                }
                Content::Text(t) | Content::Files(t) | Content::Error(t) => {
                    let text = match &st.content {
                        Content::Error(_) => format!("取得エラー: {t}"),
                        Content::Text(_) => display_text(t, view),
                        _ => t.clone(),
                    };
                    let flags = DT_WORDBREAK | DT_NOPREFIX | DT_EXPANDTABS;
                    let mut r = RECT { left: area.left, top, right: area.right, bottom: top };
                    draw_text(mem, &text, &mut r, flags | DT_CALCRECT);
                    content_h = r.bottom - top;
                    draw_text(mem, &text, &mut r, flags);
                }
                Content::Unknown => {
                    SetTextColor(mem, pal.muted);
                    let mut r = RECT { left: area.left, top, right: area.right, bottom: top + s(22) };
                    draw_text(mem, "未知の形式(または空)です。", &mut r, DT_WORDBREAK | DT_NOPREFIX);
                    content_h = s(22);
                }
                Content::Image(d) => {
                    // 縦横比を保ち、表示領域に収める
                    let scale = (avail_w as f64 / d.width as f64).min(avail_h as f64 / d.height as f64);
                    let (dw, dh) = (
                        ((d.width as f64 * scale) as i32).max(1),
                        ((d.height as f64 * scale) as i32).max(1),
                    );
                    SetStretchBltMode(mem, HALFTONE);
                    let _ = SetBrushOrgEx(mem, 0, 0, None);
                    StretchDIBits(
                        mem,
                        area.left,
                        area.top,
                        dw,
                        dh,
                        0,
                        0,
                        d.width,
                        d.height,
                        Some(d.data.as_ptr().add(d.bits_offset) as *const _),
                        d.data.as_ptr() as *const BITMAPINFO,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                    content_h = 0; // 画像はスクロールしない
                }
            }
            let _ = RestoreDC(mem, saved);
            st.max_scroll = (content_h - avail_h).max(0);
            st.scroll = st.scroll.clamp(0, st.max_scroll);

            // フッター: 状態インジケーターと詳細
            SelectObject(mem, small.into());
            let dot = s(8);
            let cy = (l.footer.top + l.footer.bottom) / 2;
            let dot_color = if st.monitoring { rgb(0x34, 0xC7, 0x59) } else { rgb(0xFF, 0x9F, 0x0A) };
            fill_ellipse(
                mem,
                &RECT { left: l.footer.left, top: cy - dot / 2, right: l.footer.left + dot, bottom: cy + dot / 2 },
                dot_color,
            );
            let status = if st.monitoring { "監視中" } else { "一時停止中" };
            let info = match detail(&st.content) {
                d if d.is_empty() => status.to_string(),
                d => format!("{status}  ·  {d}"),
            };
            SetTextColor(mem, pal.muted);
            let mut r = RECT { left: l.footer.left + dot + s(8), ..l.footer };
            draw_text(mem, &info, &mut r, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);

            // リサイズつまみ(右下の斜線)
            let pen = CreatePen(PS_SOLID, 1, pal.muted);
            let old_pen = SelectObject(mem, pen.into());
            for k in 1..=3 {
                let d = s(4) * k;
                let (x, y) = (w - s(6), h - s(6));
                let _ = MoveToEx(mem, x, y - d, None);
                let _ = LineTo(mem, x - d, y);
            }
            SelectObject(mem, old_pen);
            let _ = DeleteObject(pen.into());

            SelectObject(mem, old_font);
        });

        for f in [body, small, label, glyph] {
            let _ = DeleteObject(f.into());
        }
        let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}

// ---------- タスクトレイ ----------

const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const CMD_SHOW: usize = 1;
const CMD_TOGGLE: usize = 2;
const CMD_EXIT: usize = 3;
const CMD_BG_SELECT: usize = 4;
const CMD_BG_CLEAR: usize = 5;
const CMD_OPACITY_BASE: usize = 100; // 100 + OPACITY_CHOICES のインデックス
const CMD_THEME_BASE: usize = 200; // 200 + THEMES のインデックス
const CMD_VIEW_BASE: usize = 300; // 300 + VIEWS のインデックス

/// 黄色い付箋風の 32x32 アイコンを生成する
unsafe fn create_icon() -> HICON {
    unsafe {
        const N: usize = 32;
        let mut px = vec![0u32; N * N];
        for y in 0..N {
            for x in 0..N {
                let edge = x < 2 || y < 2 || x >= N - 2 || y >= N - 2;
                let line = (9..N - 9).contains(&x) && y >= 9 && y < N - 6 && (y - 9) % 6 < 2;
                px[y * N + x] = if edge {
                    0xFFE6_D382
                } else if line {
                    0xFF8C_7F4A
                } else {
                    0xFFFF_F4B8
                };
            }
        }
        let color = CreateBitmap(N as i32, N as i32, 1, 32, Some(px.as_ptr() as *const _));
        let mask_bits = vec![0u8; N * N / 8];
        let mask = CreateBitmap(N as i32, N as i32, 1, 1, Some(mask_bits.as_ptr() as *const _));
        let info = ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    let tip = wide("ClipSticky");
    nid.szTip[..tip.len()].copy_from_slice(&tip);
    nid
}

unsafe fn add_tray(hwnd: HWND) {
    unsafe {
        let mut nid = tray_data(hwnd);
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = create_icon();
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        let _ = DestroyIcon(nid.hIcon);
    }
}

unsafe fn remove_tray(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
    }
}

unsafe fn show_window(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

unsafe fn toggle_monitoring(hwnd: HWND) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.monitoring = !s.monitoring;
    });
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

unsafe fn add_item(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, label: &str) {
    let mut buf = wide(label);
    buf.push(0);
    unsafe {
        let _ = AppendMenuW(menu, flags, id, PCWSTR(buf.as_ptr()));
    }
}

unsafe fn add_submenu(menu: HMENU, label: &str, build: impl FnOnce(HMENU)) {
    unsafe {
        if let Ok(sub) = CreatePopupMenu() {
            build(sub);
            add_item(menu, MF_POPUP, sub.0 as usize, label);
        }
    }
}

fn check(on: bool) -> MENU_ITEM_FLAGS {
    if on { MF_STRING | MF_CHECKED } else { MF_STRING }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let monitoring = STATE.with(|s| s.borrow().monitoring);
        let (opacity, theme, view, has_bg) = SETTINGS.with(|s| {
            let s = s.borrow();
            (s.opacity, s.theme, s.view, s.background.is_some())
        });

        add_item(menu, MF_STRING, CMD_SHOW, "表示");
        add_item(menu, MF_STRING, CMD_TOGGLE, if monitoring { "監視を一時停止" } else { "監視を再開" });
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        add_submenu(menu, "テキスト表示", |sub| {
            for (i, &(v, _, name)) in VIEWS.iter().enumerate() {
                add_item(sub, check(v == view), CMD_VIEW_BASE + i, name);
            }
        });
        add_submenu(menu, "テーマ", |sub| {
            for (i, &(t, _, name)) in THEMES.iter().enumerate() {
                add_item(sub, check(t == theme), CMD_THEME_BASE + i, name);
            }
        });
        add_submenu(menu, "透明度", |sub| {
            for (i, &pct) in OPACITY_CHOICES.iter().enumerate() {
                let label = if pct == DEFAULT_OPACITY {
                    format!("{pct}%(標準)")
                } else {
                    format!("{pct}%")
                };
                add_item(sub, check(pct == opacity), CMD_OPACITY_BASE + i, &label);
            }
        });
        add_submenu(menu, "背景画像", |sub| {
            add_item(sub, MF_STRING, CMD_BG_SELECT, "画像を選択…");
            let clear = if has_bg { MF_STRING } else { MF_STRING | MF_GRAYED };
            add_item(sub, clear, CMD_BG_CLEAR, "解除");
        });
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        add_item(menu, MF_STRING, CMD_EXIT, "終了");

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // メニューが閉じなくなる問題の回避(前面化してから表示し、WM_NULL を送る)
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, Some(0), hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        match cmd.0 as usize {
            CMD_SHOW => show_window(hwnd),
            CMD_TOGGLE => toggle_monitoring(hwnd),
            CMD_EXIT => {
                let _ = DestroyWindow(hwnd);
            }
            CMD_BG_SELECT => {
                if let Some(path) = pick_image(hwnd) {
                    choose_background(hwnd, path);
                }
            }
            CMD_BG_CLEAR => {
                set_background(hwnd, None);
            }
            c if (CMD_OPACITY_BASE..CMD_OPACITY_BASE + OPACITY_CHOICES.len()).contains(&c) => {
                apply_opacity(hwnd, OPACITY_CHOICES[c - CMD_OPACITY_BASE]);
                save_settings(hwnd);
            }
            c if (CMD_VIEW_BASE..CMD_VIEW_BASE + VIEWS.len()).contains(&c) => {
                SETTINGS.with(|s| s.borrow_mut().view = VIEWS[c - CMD_VIEW_BASE].0);
                STATE.with(|s| s.borrow_mut().scroll = 0);
                save_settings(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            c if (CMD_THEME_BASE..CMD_THEME_BASE + THEMES.len()).contains(&c) => {
                apply_theme(hwnd, THEMES[c - CMD_THEME_BASE].0);
                save_settings(hwnd);
            }
            _ => {}
        }
    }
}

// ---------- ウィンドウプロシージャ ----------

unsafe fn on_clipboard_update(hwnd: HWND) {
    unsafe {
        if !STATE.with(|s| s.borrow().monitoring) {
            return;
        }
        refresh_clipboard();
        // フォーカスを奪わずに前面へ出す
        show_window(hwnd);
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn lparam_point(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam.0 & 0xFFFF) as i16 as i32,
        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    )
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CLIPBOARDUPDATE => {
                on_clipboard_update(hwnd);
                LRESULT(0)
            }
            WM_TRAY => {
                match (lparam.0 & 0xFFFF) as u32 {
                    WM_LBUTTONUP | WM_LBUTTONDBLCLK => show_window(hwnd),
                    WM_RBUTTONUP | WM_CONTEXTMENU => show_tray_menu(hwnd),
                    _ => {}
                }
                LRESULT(0)
            }
            // ウィンドウ上の右クリックでも、トレイと同じメニューを出す
            WM_NCRBUTTONUP | WM_CONTEXTMENU => {
                show_tray_menu(hwnd);
                LRESULT(0)
            }
            m if m != 0 && m == TASKBAR_CREATED.with(|t| t.get()) => {
                add_tray(hwnd); // エクスプローラー再起動時にアイコンを再登録
                LRESULT(0)
            }
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(hwnd) as i32;
                info.ptMinTrackSize = POINT { x: MIN_SIZE.0 * dpi / 96, y: MIN_SIZE.1 * dpi / 96 };
                LRESULT(0)
            }
            // 非クライアント領域をなくす(枠なしでもリサイズ可能にするため WS_THICKFRAME を付けている)
            WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),
            WM_NCHITTEST => {
                let (x, y) = lparam_point(lparam);
                let mut pt = POINT { x, y };
                let _ = ScreenToClient(hwnd, &mut pt);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let dpi = GetDpiForWindow(hwnd);
                let l = layout(rc.right, rc.bottom, dpi);
                let edge = 6 * dpi as i32 / 96;
                let grip = 14 * dpi as i32 / 96;
                let (right, bottom) = (pt.x >= rc.right - edge, pt.y >= rc.bottom - edge);
                if in_rect(&l.close, pt.x, pt.y) {
                    LRESULT(HTCLIENT as isize)
                } else if (pt.x >= rc.right - grip && pt.y >= rc.bottom - grip) || (right && bottom) {
                    LRESULT(HTBOTTOMRIGHT as isize)
                } else if right {
                    LRESULT(HTRIGHT as isize)
                } else if bottom {
                    LRESULT(HTBOTTOM as isize)
                } else {
                    LRESULT(HTCAPTION as isize) // どこをドラッグしても移動できる
                }
            }
            WM_LBUTTONDOWN => {
                let (x, y) = lparam_point(lparam);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let l = layout(rc.right, rc.bottom, GetDpiForWindow(hwnd));
                if in_rect(&l.close, x, y) {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.scroll = (s.scroll - delta / 3).clamp(0, s.max_scroll);
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_DROPFILES => {
                // 画像ファイルをドロップして背景に設定する
                let drop = HDROP(wparam.0 as *mut _);
                let len = DragQueryFileW(drop, 0, None) as usize;
                let mut buf = vec![0u16; len + 1];
                let n = DragQueryFileW(drop, 0, Some(&mut buf)) as usize;
                DragFinish(drop);
                if n > 0 {
                    choose_background(hwnd, String::from_utf16_lossy(&buf[..n]));
                }
                LRESULT(0)
            }
            WM_SIZE => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_EXITSIZEMOVE => {
                save_settings(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                save_settings(hwnd);
                let _ = RemoveClipboardFormatListener(hwnd);
                remove_tray(hwnd);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn main() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance = GetModuleHandleW(None)?;
        let class_name = w!("ClipStickyWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);

        load_settings();
        let (saved_pos, saved_size, opacity, theme, background) = SETTINGS.with(|s| {
            let s = s.borrow();
            (s.pos, s.size, s.opacity, s.theme, s.background.clone())
        });

        let dpi = GetDpiForSystem() as i32;
        let (width, height) = saved_size.unwrap_or((DEFAULT_SIZE.0 * dpi / 96, DEFAULT_SIZE.1 * dpi / 96));

        // 保存位置が画面外(モニター構成の変更など)なら、作業領域の右上に出す
        let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        let (x, y) = match saved_pos {
            Some((x, y)) if x >= vx - width + 40 && x < vx + vw - 40 && y >= vy && y < vy + vh - 40 => (x, y),
            _ => {
                let mut work = RECT::default();
                let _ = SystemParametersInfoW(
                    SPI_GETWORKAREA,
                    0,
                    Some(&mut work as *mut RECT as *mut _),
                    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                );
                let margin = 24 * dpi / 96;
                (work.right - width - margin, work.top + margin)
            }
        };

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            class_name,
            w!("ClipSticky"),
            WS_POPUP | WS_THICKFRAME,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;
        apply_opacity(hwnd, opacity);
        // 角丸(DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND。Windows 11 以降)
        let round: i32 = 2;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33),
            &round as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
        apply_theme(hwnd, theme);
        DragAcceptFiles(hwnd, true);

        if let Some(path) = background {
            if !set_background(hwnd, Some(path)) {
                SETTINGS.with(|s| s.borrow_mut().background = None); // 読み込めなければ忘れる
            }
        }

        AddClipboardFormatListener(hwnd)?;
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
        add_tray(hwnd);
        refresh_clipboard();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
