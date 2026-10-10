#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::{
    DragQueryFileW, HDROP, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const DEFAULT_TITLE: &str = "最新のクリップボード";
const MAX_CONTENT_W: i32 = 340;
const MAX_IMAGE_H: i32 = 140;
const DEFAULT_OPACITY: u32 = 96;
const OPACITY_CHOICES: [u32; 7] = [100, 96, 90, 80, 70, 60, 50];

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

struct State {
    title: String,
    status: String,
    content: Content,
    monitoring: bool,
    scroll: i32,
    max_scroll: i32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        title: DEFAULT_TITLE.into(),
        status: "監視中".into(),
        content: Content::Unknown,
        monitoring: true,
        scroll: 0,
        max_scroll: 0,
    });
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

// ---------- 設定（ウィンドウ位置の保存） ----------

fn settings_path() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join("ClipSticky");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("position.txt"))
}

/// 保存形式: "left,top,opacity(%)"
fn load_settings() -> (Option<(i32, i32)>, u32) {
    let parse = || -> Option<((i32, i32), u32)> {
        let text = std::fs::read_to_string(settings_path()?).ok()?;
        let mut it = text.trim().split(',');
        let l = it.next()?.parse::<i32>().ok()?;
        let t = it.next()?.parse::<i32>().ok()?;
        let o = it.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(DEFAULT_OPACITY);
        Some(((l, t), o.clamp(10, 100)))
    };
    match parse() {
        Some(((l, t), o)) => ((l >= 0 && t >= 0).then_some((l, t)), o),
        None => (None, DEFAULT_OPACITY),
    }
}

unsafe fn save_settings(hwnd: HWND) {
    let mut rc = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rc) }.is_ok() {
        if let Some(path) = settings_path() {
            let opacity = OPACITY.with(|o| o.get());
            let _ = std::fs::write(path, format!("{},{},{}", rc.left, rc.top, opacity));
        }
    }
}

unsafe fn apply_opacity(hwnd: HWND, percent: u32) {
    OPACITY.with(|o| o.set(percent));
    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), (percent * 255 / 100) as u8, LWA_ALPHA);
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
        let (title, detected) = match &content {
            Content::Image(_) => ("（画像）", "（画像を検出）"),
            Content::Text(_) => ("（テキスト）", "（テキストを検出）"),
            Content::Files(_) => ("（ファイル）", "（ファイルを検出）"),
            Content::Unknown => ("", "（未知の形式）"),
            Content::Error(_) => ("（エラー）", "（エラー）"),
        };
        s.title = format!("{DEFAULT_TITLE}{title}");
        s.status = if s.monitoring {
            format!("監視中{detected}")
        } else {
            "一時停止中".into()
        };
        s.content = content;
        s.scroll = 0;
    });
}

// ---------- レイアウトと描画 ----------

struct Layout {
    title: RECT,
    content: RECT,
    sep_y: i32,
    status: RECT,
    button: RECT,
}

fn layout(w: i32, h: i32, dpi: u32) -> Layout {
    let s = |v: i32| v * dpi as i32 / 96;
    let pad = s(11);
    let title = RECT { left: pad, top: pad, right: w - pad, bottom: pad + s(22) };
    let button = RECT {
        left: w - pad - s(64),
        top: h - pad - s(24),
        right: w - pad,
        bottom: h - pad,
    };
    let sep_y = button.top - s(6);
    let content = RECT {
        left: pad,
        top: title.bottom + s(4),
        right: w - pad,
        bottom: sep_y - s(6),
    };
    let status = RECT { left: pad, top: button.top, right: button.left - s(8), bottom: button.bottom };
    Layout { title, content, sep_y, status, button }
}

fn in_rect(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

unsafe fn make_font(dpi: u32, bold: bool) -> HFONT {
    unsafe {
        CreateFontW(
            -(16 * dpi as i32 / 96),
            0,
            0,
            0,
            if bold { FW_BOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
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
    let mut buf = wide(text);
    unsafe { DrawTextW(hdc, &mut buf, rc, flags) };
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

        // ダブルバッファ
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old_bmp = SelectObject(mem, bmp.into());

        let bg = CreateSolidBrush(rgb(0xFF, 0xF7, 0xC8));
        FillRect(mem, &client, bg);
        let _ = DeleteObject(bg.into());
        let border = CreatePen(PS_SOLID, 1, rgb(0x96, 0x96, 0x82));
        let old_pen = SelectObject(mem, border.into());
        let old_brush = SelectObject(mem, GetStockObject(NULL_BRUSH));
        let _ = Rectangle(mem, 0, 0, w, h);
        SelectObject(mem, old_brush);
        SelectObject(mem, old_pen);
        let _ = DeleteObject(border.into());

        SetBkMode(mem, TRANSPARENT);
        let font = make_font(dpi, false);
        let bold = make_font(dpi, true);

        STATE.with(|st| {
            let mut st = st.borrow_mut();

            // タイトル
            let old_font = SelectObject(mem, bold.into());
            SetTextColor(mem, rgb(0, 0, 0));
            let mut r = l.title;
            draw_text(mem, &st.title, &mut r, DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);

            // コンテンツ（クリップして描画）
            SelectObject(mem, font.into());
            let area = l.content;
            let saved = SaveDC(mem);
            let _ = IntersectClipRect(mem, area.left, area.top, area.right, area.bottom);
            let avail_w = (area.right - area.left).min(s(MAX_CONTENT_W));
            let top = area.top - st.scroll;
            let content_h;
            match &st.content {
                Content::Text(t) | Content::Files(t) | Content::Error(t) => {
                    let text = if matches!(st.content, Content::Error(_)) {
                        format!("取得エラー: {t}")
                    } else {
                        t.clone()
                    };
                    let mut r = RECT { left: area.left, top, right: area.left + avail_w, bottom: top };
                    draw_text(mem, &text, &mut r, DT_WORDBREAK | DT_NOPREFIX | DT_CALCRECT);
                    content_h = r.bottom - top;
                    draw_text(mem, &text, &mut r, DT_WORDBREAK | DT_NOPREFIX);
                }
                Content::Unknown => {
                    SetTextColor(mem, rgb(0x80, 0x80, 0x80));
                    let mut r = RECT { left: area.left, top, right: area.right, bottom: top + s(22) };
                    draw_text(mem, "未知の形式（または空）です。", &mut r, DT_WORDBREAK | DT_NOPREFIX);
                    content_h = s(22);
                }
                Content::Image(d) => {
                    // Uniform: 縦横比を保って最大 340x140 に収める
                    let (mw, mh) = (avail_w, s(MAX_IMAGE_H));
                    let scale = (mw as f64 / d.width as f64).min(mh as f64 / d.height as f64);
                    let (dw, dh) = ((d.width as f64 * scale) as i32, (d.height as f64 * scale) as i32);
                    SetStretchBltMode(mem, HALFTONE);
                    let _ = SetBrushOrgEx(mem, 0, 0, None);
                    StretchDIBits(
                        mem,
                        area.left,
                        top,
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
                    content_h = dh;
                }
            }
            let _ = RestoreDC(mem, saved);
            st.max_scroll = (content_h - (area.bottom - area.top)).max(0);
            st.scroll = st.scroll.clamp(0, st.max_scroll);

            // 区切り線
            let sep = CreatePen(PS_SOLID, 1, rgb(0xC8, 0xC8, 0xB0));
            let old = SelectObject(mem, sep.into());
            let _ = MoveToEx(mem, l.title.left, l.sep_y, None);
            let _ = LineTo(mem, l.title.right, l.sep_y);
            SelectObject(mem, old);
            let _ = DeleteObject(sep.into());

            // ステータス
            SetTextColor(mem, rgb(0x70, 0x70, 0x60));
            let mut r = l.status;
            draw_text(mem, &st.status, &mut r, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);

            // 閉じるボタン
            let btn_bg = CreateSolidBrush(rgb(0xE1, 0xE1, 0xE1));
            let btn_pen = CreatePen(PS_SOLID, 1, rgb(0xAD, 0xAD, 0xAD));
            let ob = SelectObject(mem, btn_bg.into());
            let op = SelectObject(mem, btn_pen.into());
            let _ = Rectangle(mem, l.button.left, l.button.top, l.button.right, l.button.bottom);
            SelectObject(mem, ob);
            SelectObject(mem, op);
            let _ = DeleteObject(btn_bg.into());
            let _ = DeleteObject(btn_pen.into());
            SetTextColor(mem, rgb(0, 0, 0));
            let mut r = l.button;
            draw_text(mem, "閉じる", &mut r, DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_NOPREFIX);

            SelectObject(mem, old_font);
        });

        let _ = DeleteObject(font.into());
        let _ = DeleteObject(bold.into());
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
const CMD_OPACITY_BASE: usize = 100; // 100 + OPACITY_CHOICES のインデックス

thread_local! {
    static OPACITY: std::cell::Cell<u32> = const { std::cell::Cell::new(DEFAULT_OPACITY) };
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

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
                    0xFF96_9682
                } else if line {
                    0xFF70_7060
                } else {
                    0xFFFF_F0A8
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
        s.status = if s.monitoring { "監視中".into() } else { "一時停止中".into() };
    });
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let monitoring = STATE.with(|s| s.borrow().monitoring);
        let toggle = wide(if monitoring { "監視を一時停止\0" } else { "監視を再開\0" });
        let _ = AppendMenuW(menu, MF_STRING, CMD_SHOW, w!("表示"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_TOGGLE, PCWSTR(toggle.as_ptr()));
        let current = OPACITY.with(|o| o.get());
        if let Ok(sub) = CreatePopupMenu() {
            for (i, &pct) in OPACITY_CHOICES.iter().enumerate() {
                let label = if pct == DEFAULT_OPACITY {
                    format!("{pct}%（標準）\0")
                } else {
                    format!("{pct}%\0")
                };
                let label = wide(&label);
                let flags = if pct == current { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(sub, flags, CMD_OPACITY_BASE + i, PCWSTR(label.as_ptr()));
            }
            let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("透明度"));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("終了"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // メニューが閉じなくなる問題の回避（前面化してから表示し、WM_NULL を送る）
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
            c if (CMD_OPACITY_BASE..CMD_OPACITY_BASE + OPACITY_CHOICES.len()).contains(&c) => {
                apply_opacity(hwnd, OPACITY_CHOICES[c - CMD_OPACITY_BASE]);
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
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
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
            // 非クライアント領域をなくす（枠なしでもリサイズ可能にするため WS_THICKFRAME を付けている）
            WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),
            WM_NCHITTEST => {
                let mut pt = POINT {
                    x: (lparam.0 & 0xFFFF) as i16 as i32,
                    y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
                };
                let _ = ScreenToClient(hwnd, &mut pt);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let dpi = GetDpiForWindow(hwnd);
                let l = layout(rc.right, rc.bottom, dpi);
                let grip = 10 * dpi as i32 / 96;
                if in_rect(&l.button, pt.x, pt.y) {
                    LRESULT(HTCLIENT as isize)
                } else if pt.x >= rc.right - grip && pt.y >= rc.bottom - grip {
                    LRESULT(HTBOTTOMRIGHT as isize)
                } else {
                    LRESULT(HTCAPTION as isize) // どこをドラッグしても移動できる
                }
            }
            WM_LBUTTONDOWN => {
                let (x, y) = ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let l = layout(rc.right, rc.bottom, GetDpiForWindow(hwnd));
                if in_rect(&l.button, x, y) {
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

        let dpi = GetDpiForSystem();
        let (width, height) = (380 * dpi as i32 / 96, 220 * dpi as i32 / 96);
        let (pos, opacity) = load_settings();
        let (x, y) = pos.unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));

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
