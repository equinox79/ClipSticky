use std::sync::atomic::{AtomicIsize, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, CreateRectRgn, SetWindowRgn};
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

/// クリップボードが変わるたびに増える番号
pub fn change_count() -> u64 {
    unsafe { GetClipboardSequenceNumber() as u64 }
}

/// カーソルの画面座標(論理ポイント)。ウィンドウの移動ドラッグに使う。
/// ppp は 1 論理ポイントあたりの物理ピクセル数
pub fn cursor_pos(ppp: f32) -> Option<(f32, f32)> {
    let mut pt = windows::Win32::Foundation::POINT::default();
    unsafe { GetCursorPos(&mut pt).ok()? };
    Some((pt.x as f32 / ppp, pt.y as f32 / ppp))
}

/// 起動前に eframe の設定を OS 向けに調整する(Windows では不要)
pub fn configure(_options: &mut eframe::NativeOptions) {}

/// prepare_window で記録するウィンドウハンドル(どのスレッドからでも表示・非表示できるように)
static HWND_VALUE: AtomicIsize = AtomicIsize::new(0);

fn hwnd(frame: &eframe::Frame) -> Option<HWND> {
    match frame.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// ウィンドウの枠(非クライアント領域)の再描画を止める。
///
/// winit は影のために WS_CAPTION を残している。そのため、ウィンドウがアクティブになる(右クリックメニューを出す)と、
/// Windows がタイトルバーと枠を描いて、付箋の上と左に白い帯が出てしまう。
/// 枠なしウィンドウの定番の対処として、WM_NCACTIVATE の再描画の指示(lParam = -1)と、WM_NCPAINT を止める
unsafe extern "system" fn frame_guard(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_NCACTIVATE => DefSubclassProc(hwnd, msg, wparam, LPARAM(-1)),
            WM_NCPAINT => LRESULT(0),
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

/// ウィンドウ作成後に 1 回呼ぶ: フォーカスを奪わない・タスクバーに出さない・枠線を消す
pub fn prepare_window(frame: &eframe::Frame) {
    let Some(hwnd) = hwnd(frame) else { return };
    HWND_VALUE.store(hwnd.0 as isize, Ordering::Relaxed);
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(frame_guard), 1, 0);
    }
    ensure_style();
    unsafe {
        // 枠線を消し(DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE)、DWM の角丸もやめる(角丸は自前で描く)
        let none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(34),
            &none as *const u32 as *const _,
            size_of::<u32>() as u32,
        );
        let donotround: i32 = 1; // DWMWCP_DONOTROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33),
            &donotround as *const i32 as *const _,
            size_of::<i32>() as u32,
        );
    }
}

/// カーソル位置にネイティブのコンテキストメニューを出す(選択結果は MenuEvent で届く)
pub fn show_context_menu(frame: &eframe::Frame, menu: &muda::Menu) {
    use muda::ContextMenu;
    if let Some(hwnd) = hwnd(frame) {
        unsafe {
            menu.show_context_menu_for_hwnd(hwnd.0 as isize, None);
        }
    }
}

fn stored_hwnd() -> Option<HWND> {
    match HWND_VALUE.load(Ordering::Relaxed) {
        0 => None,
        v => Some(HWND(v as *mut _)),
    }
}

thread_local! {
    /// 最後に SetWindowRgn したときのウィンドウサイズ
    static CLIPPED_SIZE: std::cell::Cell<(i32, i32)> = const { std::cell::Cell::new((0, 0)) };
}

/// 描画範囲をクライアント領域に切り詰める。
/// 透明な枠なしウィンドウでも、非クライアント領域の最外周 1px を DWM が不透明な線として描くため、
/// そのままだと角丸の外側(四隅)に線が残る。サイズが変わったときだけ設定し直す
pub fn clip_to_client() {
    let Some(hwnd) = stored_hwnd() else { return };
    unsafe {
        let mut window = RECT::default();
        let mut client = RECT::default();
        if GetWindowRect(hwnd, &mut window).is_err() || GetClientRect(hwnd, &mut client).is_err() {
            return;
        }
        let size = (window.right - window.left, window.bottom - window.top);
        if CLIPPED_SIZE.get() == size {
            return;
        }
        // クライアント領域の左上を、ウィンドウ左上を原点とした座標に直す
        let mut origin = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut origin);
        let (x, y) = (origin.x - window.left, origin.y - window.top);
        let rgn = CreateRectRgn(x, y, x + client.right, y + client.bottom);
        if SetWindowRgn(hwnd, Some(rgn), true) != 0 {
            CLIPPED_SIZE.set(size);
        }
    }
}

/// フォーカスを奪わない・タスクバーに出さない拡張スタイルを保つ。
/// winit はウィンドウの表示などのたびに拡張スタイルを書き換えるため、毎フレーム呼んで付け直す
pub fn ensure_style() {
    if let Some(hwnd) = stored_hwnd() {
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let want = (ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize) & !(WS_EX_APPWINDOW.0 as isize);
            if ex != want {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
            }

            // タイトルバー(WS_CAPTION)を外す。winit は影のために残しているが、あると、ウィンドウがアクティブになったとき
            // (右クリックメニューを出したとき)に、Windows がタイトルバーを描いて、付箋の上に白い帯が出てしまう。
            // リサイズに使う WS_THICKFRAME は残す
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let caption = WS_CAPTION.0 as isize;
            if style & caption != 0 {
                SetWindowLongPtrW(hwnd, GWL_STYLE, style & !caption);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }
}

/// 前面に出す。フォーカスは奪わない。winit を経由せず Win32 を直接呼ぶ
/// (winit は表示のたびに拡張スタイルを書き換え、2 回目以降は SW_SHOW でアクティブ化するため)
pub fn show() {
    ensure_style();
    if let Some(hwnd) = stored_hwnd() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

pub fn hide() {
    if let Some(hwnd) = stored_hwnd() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}