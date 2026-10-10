/// クリップボードから読み取った画像(RGBA、上から下へ)
pub struct ImageContent {
    /// 内容が変わるたびに増える番号(テクスチャのキャッシュ用)
    pub id: u64,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub enum Content {
    Text(String),
    Image(ImageContent),
    Files(Vec<String>),
    Unknown,
    Error(String),
}
