//! OS ごとに異なる処理(クリップボードの変更番号、ウィンドウの設定、メニューの表示)

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("ClipSticky は Windows と macOS のみに対応しています");
