use super::i18n::tr;

/// クリップボードから読み取った画像(RGBA、上から下へ)
pub struct ImageContent {
    /// 内容が変わるたびに増える番号(テクスチャのキャッシュ用)
    pub id: u64,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// クリップボードを読めなかった理由。表示のたびに、現在の言語で文にする
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardError {
    /// クリップボードを開けなかった
    Open,
    /// 他のプロセスが使用中だった
    Busy,
}

impl ClipboardError {
    pub fn message(self) -> &'static str {
        match self {
            ClipboardError::Open => tr("クリップボードを開けませんでした。", "Could not open the clipboard."),
            ClipboardError::Busy => {
                tr("クリップボードが他のプロセスで使用中です。", "The clipboard is in use by another process.")
            }
        }
    }
}

pub enum Content {
    Text(String),
    Image(ImageContent),
    Files(Vec<String>),
    Unknown,
    Error(ClipboardError),
}
