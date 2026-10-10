//! テキストの変換(改行コード・空白・全角半角・正規化・大文字小文字)。
//! 変換した結果は、クリップボードに入れ直して使う。

use std::collections::HashMap;
use std::sync::OnceLock;

use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conversion {
    NewlineLf,
    NewlineCrlf,
    NewlineCr,
    TrimEndNewlines,
    TrimBoth,
    TrimLineEnds,
    ToHalfWidthAscii,
    ToFullWidthAscii,
    KanaToFull,
    KanaToHalf,
    Nfc,
    Nfd,
    Nfkc,
    Lower,
    Upper,
}

pub struct ConversionInfo {
    pub conversion: Conversion,
    /// メニューの項目 ID に使う名前
    pub id: &'static str,
    pub label: &'static str,
}

const fn info(conversion: Conversion, id: &'static str, label: &'static str) -> ConversionInfo {
    ConversionInfo { conversion, id, label }
}

/// メニューに並べる順(グループ名, 項目)
pub const CONVERSION_GROUPS: [(&str, &[ConversionInfo]); 5] = [
    (
        "改行コード",
        &[
            info(Conversion::NewlineLf, "newline-lf", "LF にそろえる(Linux・macOS)"),
            info(Conversion::NewlineCrlf, "newline-crlf", "CRLF にそろえる(Windows)"),
            info(Conversion::NewlineCr, "newline-cr", "CR にそろえる(古い Mac)"),
        ],
    ),
    (
        "空白・改行の削除",
        &[
            info(Conversion::TrimEndNewlines, "trim-end-newlines", "末尾の改行を削除"),
            info(Conversion::TrimBoth, "trim-both", "前後の空白・改行を削除"),
            info(Conversion::TrimLineEnds, "trim-line-ends", "行末の空白を削除"),
        ],
    ),
    (
        "全角・半角",
        &[
            info(Conversion::ToHalfWidthAscii, "to-half-ascii", "全角の英数・記号を半角に"),
            info(Conversion::ToFullWidthAscii, "to-full-ascii", "半角の英数・記号を全角に"),
            info(Conversion::KanaToFull, "kana-to-full", "半角カナを全角カナに"),
            info(Conversion::KanaToHalf, "kana-to-half", "全角カナを半角カナに"),
        ],
    ),
    (
        "Unicode 正規化",
        &[
            info(Conversion::Nfc, "nfc", "NFC(合成。macOS のファイル名の濁点を直す)"),
            info(Conversion::Nfd, "nfd", "NFD(分解)"),
            info(Conversion::Nfkc, "nfkc", "NFKC(互換文字を通常の文字に。㈱ → (株)、① → 1)"),
        ],
    ),
    (
        "大文字・小文字",
        &[
            info(Conversion::Lower, "lower", "小文字にする"),
            info(Conversion::Upper, "upper", "大文字にする"),
        ],
    ),
];

pub fn conversion_by_id(id: &str) -> Option<&'static ConversionInfo> {
    CONVERSION_GROUPS.iter().flat_map(|(_, items)| items.iter()).find(|i| i.id == id)
}

pub fn conversion_info(c: Conversion) -> &'static ConversionInfo {
    CONVERSION_GROUPS
        .iter()
        .flat_map(|(_, items)| items.iter())
        .find(|i| i.conversion == c)
        .expect("every conversion is listed")
}

pub fn convert(text: &str, conversion: Conversion) -> String {
    match conversion {
        Conversion::NewlineLf => normalize_newlines(text, "\n"),
        Conversion::NewlineCrlf => normalize_newlines(text, "\r\n"),
        Conversion::NewlineCr => normalize_newlines(text, "\r"),
        Conversion::TrimEndNewlines => text.trim_end_matches(['\r', '\n']).to_string(),
        Conversion::TrimBoth => text.trim().to_string(),
        Conversion::TrimLineEnds => trim_line_ends(text),
        Conversion::ToHalfWidthAscii => text.chars().map(to_half_width).collect(),
        Conversion::ToFullWidthAscii => text.chars().map(to_full_width).collect(),
        Conversion::KanaToFull => kana_to_full(text),
        Conversion::KanaToHalf => kana_to_half(text),
        Conversion::Nfc => text.nfc().collect(),
        Conversion::Nfd => text.nfd().collect(),
        Conversion::Nfkc => text.nfkc().collect(),
        Conversion::Lower => text.to_lowercase(),
        Conversion::Upper => text.to_uppercase(),
    }
}

/// 改行(CRLF・LF・単独の CR)を、すべて `to` にそろえる
fn normalize_newlines(text: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str(to);
            }
            '\n' => out.push_str(to),
            _ => out.push(c),
        }
    }
    out
}

/// 各行の末尾の空白(半角・タブ・全角)を削除する。改行の種類は変えない
fn trim_line_ends(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for segment in text.split_inclusive('\n') {
        let (body, newline) = match segment.strip_suffix("\r\n") {
            Some(b) => (b, "\r\n"),
            None => match segment.strip_suffix('\n') {
                Some(b) => (b, "\n"),
                None => (segment, ""),
            },
        };
        out.push_str(body.trim_end_matches([' ', '\t', '\u{3000}']));
        out.push_str(newline);
    }
    out
}

/// 全角の英数・記号(U+FF01〜U+FF5E)と全角スペースを、半角にする
fn to_half_width(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{3000}' => ' ',
        _ => c,
    }
}

/// 半角の英数・記号(U+0021〜U+007E)と半角スペースを、全角にする
fn to_full_width(c: char) -> char {
    match c {
        '\u{21}'..='\u{7E}' => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
        ' ' => '\u{3000}',
        _ => c,
    }
}

fn is_half_width_kana(c: char) -> bool {
    ('\u{FF61}'..='\u{FF9F}').contains(&c)
}

/// 全角カナ → 半角カナの対応(半角カナを NFKC で全角にしたものから逆引きする。濁点・半濁点の結合文字も含む)
fn full_to_half_map() -> &'static HashMap<char, char> {
    static MAP: OnceLock<HashMap<char, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        ('\u{FF61}'..='\u{FF9F}')
            .filter_map(|h| {
                let full: Vec<char> = h.to_string().nfkc().collect();
                (full.len() == 1).then(|| (full[0], h))
            })
            .collect()
    })
}

/// 半角カナ(濁点・半濁点つきも)を、全角カナにする。半角カナ以外は変えない
fn kana_to_full(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars() {
        if is_half_width_kana(c) {
            run.push(c);
        } else {
            if !run.is_empty() {
                out.extend(run.nfkc());
                run.clear();
            }
            out.push(c);
        }
    }
    out.extend(run.nfkc());
    out
}

/// 全角カナ(濁点つきも)を、半角カナにする。ガ → ｶﾞ。カナ以外は変えない
fn kana_to_half(text: &str) -> String {
    let map = full_to_half_map();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if let Some(&h) = map.get(&c) {
            out.push(h);
            continue;
        }
        // 濁点つきの文字は、分解(カ + ゛)してから、それぞれを半角にする
        let parts: Vec<char> = c.nfd().collect();
        if parts.len() > 1 && parts.iter().all(|p| map.contains_key(p)) {
            out.extend(parts.iter().map(|p| map[p]));
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newlines_are_unified() {
        let text = "a\r\nb\nc\rd";
        assert_eq!(convert(text, Conversion::NewlineLf), "a\nb\nc\nd");
        assert_eq!(convert(text, Conversion::NewlineCrlf), "a\r\nb\r\nc\r\nd");
        assert_eq!(convert(text, Conversion::NewlineCr), "a\rb\rc\rd");
    }

    #[test]
    fn trimming() {
        assert_eq!(convert("a  \n\n", Conversion::TrimEndNewlines), "a  ");
        assert_eq!(convert("\n  a b \r\n", Conversion::TrimBoth), "a b");
        assert_eq!(convert("a  \r\nb\t \nc \u{3000}", Conversion::TrimLineEnds), "a\r\nb\nc");
    }

    #[test]
    fn full_and_half_width_ascii() {
        assert_eq!(convert("ＡＢＣ１２３！　ｘ", Conversion::ToHalfWidthAscii), "ABC123! x");
        assert_eq!(convert("ABC 123!", Conversion::ToFullWidthAscii), "ＡＢＣ　１２３！");
        assert_eq!(convert("あア漢", Conversion::ToHalfWidthAscii), "あア漢");
    }

    #[test]
    fn kana_widths() {
        assert_eq!(convert("ｶﾞｷﾞｸﾞﾊﾟｱｰ｡､｢｣･ﾝ", Conversion::KanaToFull), "ガギグパアー。、「」・ン");
        assert_eq!(convert("ガギグパアーン、ヴ", Conversion::KanaToHalf), "ｶﾞｷﾞｸﾞﾊﾟｱｰﾝ､ｳﾞ");
        // かな・漢字・英数は変えない
        assert_eq!(convert("あ漢A1", Conversion::KanaToHalf), "あ漢A1");
        assert_eq!(convert("あ漢A1", Conversion::KanaToFull), "あ漢A1");
    }

    #[test]
    fn kana_roundtrip() {
        let full = "コンピューターガパヴ";
        assert_eq!(convert(&convert(full, Conversion::KanaToHalf), Conversion::KanaToFull), full);
    }

    #[test]
    fn normalization() {
        // 「が」を、1 文字(NFC)と、か + 濁点(NFD)で行き来する
        assert_eq!(convert("か\u{3099}", Conversion::Nfc), "が");
        assert_eq!(convert("が", Conversion::Nfd), "か\u{3099}");
        assert_eq!(convert("㈱①ｱ", Conversion::Nfkc), "(株)1ア");
    }

    #[test]
    fn case() {
        assert_eq!(convert("Hello ÄÖ", Conversion::Lower), "hello äö");
        assert_eq!(convert("Hello ß", Conversion::Upper), "HELLO SS");
    }

    #[test]
    fn ids_are_unique_and_resolvable() {
        let mut seen = std::collections::HashSet::new();
        for (_, items) in CONVERSION_GROUPS {
            for i in items {
                assert!(seen.insert(i.id), "duplicate id {}", i.id);
                assert_eq!(conversion_by_id(i.id).unwrap().conversion, i.conversion);
                assert_eq!(conversion_info(i.conversion).id, i.id);
            }
        }
        assert_eq!(seen.len(), 15);
    }
}
