//! 表示言語の切り替え(日本語 / English)。
//!
//! 現在の言語は、スレッドごとに持つ(画面を描くメインスレッドで設定する。テストは、スレッドごとに独立する)。
//! 文字列は、使う場所に日本語と英語を並べて書き、`tr!`(固定の文字列)・`tr_fmt!`(書式つき)で選ぶ。

use std::cell::Cell;

/// 実際に使う言語
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ja,
    En,
}

/// 設定で選ぶ言語。`Auto` は OS の言語に従う
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Auto,
    Ja,
    En,
}

/// (値, 設定ファイル上の名前)
pub const LANGUAGES: [(Language, &str); 3] =
    [(Language::Auto, "auto"), (Language::Ja, "ja"), (Language::En, "en")];

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::Ja) };
}

pub fn lang() -> Lang {
    CURRENT.with(|c| c.get())
}

pub fn set_lang(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

/// OS のロケール(`ja-JP`、`en_US` など)から、使う言語を決める。日本語以外は英語にする
pub fn lang_from_locale(locale: &str) -> Lang {
    let primary = locale.split(['-', '_']).next().unwrap_or_default();
    if primary.eq_ignore_ascii_case("ja") { Lang::Ja } else { Lang::En }
}

/// 設定から、実際に使う言語を決める
pub fn resolve(language: Language) -> Lang {
    match language {
        Language::Ja => Lang::Ja,
        Language::En => Lang::En,
        Language::Auto => sys_locale::get_locale().map_or(Lang::En, |l| lang_from_locale(&l)),
    }
}

/// 日本語と英語のうち、現在の言語のほうを返す
pub fn tr(ja: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Ja => ja,
        Lang::En => en,
    }
}

/// 書式つきの文字列を、現在の言語で作る(`tr_fmt!("{n} 件", "{n} files", n = 3)`)
#[macro_export]
macro_rules! tr_fmt {
    ($ja:literal, $en:literal $(, $($args:tt)*)?) => {
        match $crate::core::i18n::lang() {
            $crate::core::i18n::Lang::Ja => format!($ja $(, $($args)*)?),
            $crate::core::i18n::Lang::En => format!($en $(, $($args)*)?),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tr_follows_the_current_language() {
        set_lang(Lang::Ja);
        assert_eq!(tr("終了", "Quit"), "終了");
        set_lang(Lang::En);
        assert_eq!(tr("終了", "Quit"), "Quit");
        let n = 3;
        assert_eq!(tr_fmt!("{n} 件", "{n} files"), "3 files");
        assert_eq!(tr_fmt!("{} 件", "{} files", n + 1), "4 files");
        set_lang(Lang::Ja);
        assert_eq!(tr_fmt!("{n} 件", "{n} files"), "3 件");
    }

    /// 画面に出す文字列は、日本語・英語のどちらでも出せる(英語に日本語が残っていない)
    #[test]
    fn english_labels_have_no_japanese() {
        use crate::core::content::{ClipboardError, Content, ImageContent};
        use crate::core::convert::CONVERSION_GROUPS;
        use crate::core::settings::{THEMES, VIEWS, theme_label, view_label};
        use crate::core::text::{detail, omitted_note, type_label};

        let has_japanese = |s: &str| s.chars().any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}'));
        set_lang(Lang::En);
        let mut labels: Vec<String> = Vec::new();
        for g in CONVERSION_GROUPS {
            labels.push(g.name().to_string());
            labels.extend(g.items.iter().map(|i| i.label().to_string()));
        }
        labels.extend(THEMES.iter().map(|t| theme_label(t.0).to_string()));
        labels.extend(VIEWS.iter().map(|v| view_label(v.0).to_string()));
        labels.push(omitted_note(5));
        labels.push(ClipboardError::Open.message().to_string());
        labels.push(ClipboardError::Busy.message().to_string());
        let image = Content::Image(ImageContent { id: 1, width: 2, height: 3, rgba: vec![0; 24] });
        for c in [
            Content::Text("a\nb".into()),
            image,
            Content::Files(vec!["x".into()]),
            Content::Unknown,
            Content::Error(ClipboardError::Busy),
        ] {
            labels.push(type_label(&c).to_string());
            labels.push(detail(&c));
        }
        for l in &labels {
            // ㈱ → (株) のような、変換の例に出てくる日本語は、英語の説明にも残してよい
            if l.contains('㈱') {
                continue;
            }
            assert!(!has_japanese(l), "英語の表示に日本語が残っている: {l}");
        }
        set_lang(Lang::Ja);
        assert!(has_japanese(&omitted_note(5)));
        assert!(has_japanese(ClipboardError::Busy.message()));
    }

    #[test]
    fn locale_detection() {
        assert_eq!(lang_from_locale("ja-JP"), Lang::Ja);
        assert_eq!(lang_from_locale("ja_JP.UTF-8"), Lang::Ja);
        assert_eq!(lang_from_locale("JA"), Lang::Ja);
        assert_eq!(lang_from_locale("en-US"), Lang::En);
        assert_eq!(lang_from_locale("fr-FR"), Lang::En);
        assert_eq!(lang_from_locale(""), Lang::En);
    }

    #[test]
    fn explicit_languages_ignore_the_system() {
        assert_eq!(resolve(Language::Ja), Lang::Ja);
        assert_eq!(resolve(Language::En), Lang::En);
    }

    #[test]
    fn language_names_are_unique() {
        let names: std::collections::HashSet<_> = LANGUAGES.iter().map(|l| l.1).collect();
        assert_eq!(names.len(), LANGUAGES.len());
    }
}
