//! クリップボードの監視と読み取り(別スレッドで変更番号をポーリングする)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use eframe::egui;

use crate::core::content::{ClipboardError, Content, ImageContent};
use crate::platform;

const POLL_INTERVAL: Duration = Duration::from_millis(150);
const RETRY_INTERVAL: Duration = Duration::from_millis(50);
const RETRIES: usize = 5;

/// 監視スレッドを起動する。クリップボードが変わるたびに内容を送り、`ctx` を再描画させる。
/// `monitoring` が false の間は読み取りも通知もしない(ただし最初の 1 回は必ず送る)
pub fn spawn(ctx: egui::Context, monitoring: Arc<AtomicBool>) -> mpsc::Receiver<Content> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut clipboard: Option<arboard::Clipboard> = None;
        let mut last_count = None;
        let mut next_id = 1u64;
        let mut first = true;
        loop {
            let count = platform::change_count();
            if last_count != Some(count) {
                last_count = Some(count);
                if first || monitoring.load(Ordering::Relaxed) {
                    first = false;
                    if clipboard.is_none() {
                        clipboard = arboard::Clipboard::new().ok();
                    }
                    let content = match clipboard.as_mut() {
                        Some(cb) => read(cb, next_id),
                        None => Content::Error(ClipboardError::Open),
                    };
                    next_id += 1;
                    if tx.send(content).is_err() {
                        return; // アプリが終了した
                    }
                    ctx.request_repaint();
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
    rx
}

/// 画像 → テキスト → ファイル の順に読む。他のプロセスが使用中ならリトライする
fn read(cb: &mut arboard::Clipboard, id: u64) -> Content {
    use arboard::Error;
    for _ in 0..RETRIES {
        let mut occupied = false;
        match cb.get_image() {
            Ok(img) => {
                return Content::Image(ImageContent {
                    id,
                    width: img.width,
                    height: img.height,
                    rgba: img.bytes.into_owned(),
                });
            }
            Err(Error::ClipboardOccupied) => occupied = true,
            Err(_) => {}
        }
        match cb.get_text() {
            Ok(text) => return Content::Text(text),
            Err(Error::ClipboardOccupied) => occupied = true,
            Err(_) => {}
        }
        match cb.get().file_list() {
            Ok(files) if !files.is_empty() => {
                return Content::Files(files.iter().map(|p| p.display().to_string()).collect());
            }
            Err(Error::ClipboardOccupied) => occupied = true,
            _ => {}
        }
        if !occupied {
            return Content::Unknown;
        }
        std::thread::sleep(RETRY_INTERVAL);
    }
    Content::Error(ClipboardError::Busy)
}
