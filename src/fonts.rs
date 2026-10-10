//! システムのフォントを egui に読み込む(日本語表示と、フォントの選択)

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use eframe::egui::{FontData, FontDefinitions, FontFamily};

#[cfg(windows)]
pub const DEFAULT_FONT: &str = "Yu Gothic UI";
#[cfg(not(windows))]
pub const DEFAULT_FONT: &str = "Hiragino Sans";

/// 既定のフォントが無いときに順に試す日本語フォント
#[cfg(windows)]
const JAPANESE_FALLBACKS: &[&str] = &["Yu Gothic UI", "Meiryo UI", "Meiryo", "MS UI Gothic", "MS Gothic"];
#[cfg(not(windows))]
const JAPANESE_FALLBACKS: &[&str] = &["Hiragino Sans", "Hiragino Kaku Gothic ProN", "Osaka"];

/// 記号(⟦ ⟧ など)を補うためのフォント
#[cfg(windows)]
const SYMBOL_FALLBACKS: &[&str] = &["Segoe UI Symbol", "Segoe UI"];
#[cfg(not(windows))]
const SYMBOL_FALLBACKS: &[&str] = &["Apple Symbols", "Arial Unicode MS"];

pub struct FontBook {
    db: fontdb::Database,
    cache: HashMap<String, Option<Arc<FontData>>>,
    /// インストール済みのフォントファミリー名(重複なし・昇順)
    pub families: Vec<String>,
}

impl FontBook {
    pub fn new() -> FontBook {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let names: BTreeSet<String> = db
            .faces()
            .flat_map(|f| f.families.iter().map(|(name, _)| name.clone()))
            .filter(|n| !n.starts_with('.') && !n.starts_with('@'))
            .collect();
        FontBook { db, cache: HashMap::new(), families: names.into_iter().collect() }
    }

    fn load(&mut self, name: &str) -> Option<Arc<FontData>> {
        if let Some(hit) = self.cache.get(name) {
            return hit.clone();
        }
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        let data = self.db.query(&query).and_then(|id| {
            self.db.with_face_data(id, |bytes, index| {
                let mut data = FontData::from_owned(bytes.to_vec());
                data.index = index;
                Arc::new(data)
            })
        });
        self.cache.insert(name.to_string(), data.clone());
        data
    }

    /// 指定のフォント(なければ日本語の既定)を先頭にしたフォント定義を作る
    pub fn definitions(&mut self, preferred: Option<&str>) -> FontDefinitions {
        let mut defs = FontDefinitions::default();
        let mut chain: Vec<String> = Vec::new();
        let add = |this: &mut FontBook, defs: &mut FontDefinitions, name: &str, chain: &mut Vec<String>| {
            if chain.iter().any(|n| n == name) {
                return false;
            }
            match this.load(name) {
                Some(data) => {
                    defs.font_data.insert(name.to_string(), data);
                    chain.push(name.to_string());
                    true
                }
                None => false,
            }
        };

        if let Some(name) = preferred {
            add(self, &mut defs, name, &mut chain);
        }
        // 日本語が出せるフォントを 1 つは入れる(選んだフォントに日本語が無い場合の補完)
        for name in JAPANESE_FALLBACKS {
            if add(self, &mut defs, name, &mut chain) && preferred.is_none() {
                break;
            }
        }
        for name in SYMBOL_FALLBACKS {
            add(self, &mut defs, name, &mut chain);
        }

        let proportional = defs.families.entry(FontFamily::Proportional).or_default();
        for (i, name) in chain.iter().enumerate() {
            proportional.insert(i, name.clone());
        }
        defs
    }
}
