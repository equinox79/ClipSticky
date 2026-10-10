//! macOS 向けの実装。
//!
//! 注意: この環境(Windows)では型検査(`cargo check --target aarch64-apple-darwin`)までしかできず、
//! 実機での動作確認はまだ行っていない。フォーカスを奪わない挙動などは Mac 上での確認が必要。

use std::cell::RefCell;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSEvent, NSFloatingWindowLevel, NSPasteboard, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

thread_local! {
    /// prepare_window で記録するウィンドウ(メインスレッドからだけ使う)
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}

/// クリップボードが変わるたびに増える番号(変更通知の API が無いため、ポーリングで見る)
pub fn change_count() -> u64 {
    NSPasteboard::generalPasteboard().changeCount() as u64
}

/// 起動前に eframe の設定を調整する: Dock にアイコンを出さない・Cmd+Tab に出さない(アクセサリアプリ)
pub fn configure(options: &mut eframe::NativeOptions) {
    use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
    options.event_loop_builder = Some(Box::new(|builder| {
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }));
}

fn ns_view(frame: &eframe::Frame) -> Option<&NSView> {
    match frame.window_handle().ok()?.as_raw() {
        // ハンドルの指す NSView は、frame が生きている間は有効
        RawWindowHandle::AppKit(h) => Some(unsafe { h.ns_view.cast::<NSView>().as_ref() }),
        _ => None,
    }
}

/// ウィンドウ作成後に 1 回呼ぶ: 全 Space・フルスクリーンアプリの上に出し、影を消す
pub fn prepare_window(frame: &eframe::Frame) {
    let Some(view) = ns_view(frame) else { return };
    let Some(window) = view.window() else { return };
    window.setLevel(NSFloatingWindowLevel);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary,
    );
    window.setHidesOnDeactivate(false);
    window.setHasShadow(false);
    WINDOW.with(|w| *w.borrow_mut() = Some(window));
}

/// Windows のようにスタイルが書き換えられることはないので、何もしない
pub fn ensure_style() {}

/// 前面に出す。アプリをアクティブにしない(`orderFrontRegardless`)ので、他のアプリのフォーカスは奪わない想定
pub fn show() {
    WINDOW.with(|w| {
        if let Some(window) = &*w.borrow() {
            window.orderFrontRegardless();
        }
    });
}

pub fn hide() {
    WINDOW.with(|w| {
        if let Some(window) = &*w.borrow() {
            window.orderOut(None);
        }
    });
}

/// カーソルの画面座標(論理ポイント、左上が原点)。ウィンドウの移動ドラッグに使う
pub fn cursor_pos(_ppp: f32) -> Option<(f32, f32)> {
    let mtm = MainThreadMarker::new()?;
    let p = NSEvent::mouseLocation();
    // NSEvent は左下が原点。メインスクリーンの高さで上下を反転する
    let height = NSScreen::screens(mtm).firstObject()?.frame().size.height;
    Some((p.x as f32, (height - p.y) as f32))
}

/// カーソル位置にネイティブのコンテキストメニューを出す(選択結果は MenuEvent で届く)
pub fn show_context_menu(frame: &eframe::Frame, menu: &muda::Menu) {
    use muda::ContextMenu;
    if let Some(view) = ns_view(frame) {
        unsafe {
            menu.show_context_menu_for_nsview(view as *const NSView as *const _, None);
        }
    }
}
