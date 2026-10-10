//! テキストの表示用加工(制御文字の可視化、文字コード一覧、文字数など)

use super::content::Content;
use super::settings::View;

/// 表示するテキストの上限(これを超える分は省略する。描画が重くなるため)
pub const MAX_DISPLAY_CHARS: usize = 20_000;
/// 文字コード一覧で表示する文字数の上限
pub const MAX_CODE_ROWS: usize = 1_000;

const CONTROL_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR", "SO",
    "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS",
    "GS", "RS", "US",
];

/// 目に見えない文字・空白・制御文字の(表示用の記号, 名前)。普通の文字なら None
pub fn special_char(c: char) -> Option<(String, String)> {
    let code = c as u32;
    let pair = |sym: &str, name: &str| Some((sym.to_string(), name.to_string()));
    match code {
        0x09 => pair("⟦TAB⟧", "HT (TAB)"),
        0x00..=0x1F => {
            let name = CONTROL_NAMES[code as usize];
            Some((format!("⟦{name}⟧"), name.to_string()))
        }
        0x20 => pair("·", "SPACE"),
        0x7F => pair("⟦DEL⟧", "DEL"),
        0x80..=0x9F => Some((format!("⟦{code:02X}⟧"), "C1 CONTROL".to_string())),
        0xA0 => pair("⟦NBSP⟧", "NO-BREAK SPACE"),
        0xAD => pair("⟦SHY⟧", "SOFT HYPHEN"),
        0x2028 => pair("⟦LS⟧", "LINE SEPARATOR"),
        0x2029 => pair("⟦PS⟧", "PARAGRAPH SEPARATOR"),
        0x200B => pair("⟦ZWSP⟧", "ZERO WIDTH SPACE"),
        0x200C => pair("⟦ZWNJ⟧", "ZERO WIDTH NON-JOINER"),
        0x200D => pair("⟦ZWJ⟧", "ZERO WIDTH JOINER"),
        0x200E => pair("⟦LRM⟧", "LEFT-TO-RIGHT MARK"),
        0x200F => pair("⟦RLM⟧", "RIGHT-TO-LEFT MARK"),
        0xFEFF => pair("⟦BOM⟧", "BOM / ZWNBSP"),
        0x3000 => pair("□", "IDEOGRAPHIC SPACE"),
        _ => None,
    }
}

/// 制御文字を記号に置き換える。改行・タブは記号の後ろで実際にも改行/タブする
pub fn show_controls(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                out.push_str("⟦CR⟧⟦LF⟧\n");
            }
            '\n' => out.push_str("⟦LF⟧\n"),
            '\t' => out.push_str("⟦TAB⟧\t"),
            _ => match special_char(c) {
                Some((sym, _)) => out.push_str(&sym),
                None => out.push(c),
            },
        }
    }
    out
}

/// 文字コード一覧の 1 行分
pub struct CodeRow {
    pub shown: String,
    pub code: String,
    pub utf8: String,
    pub name: String,
}

/// 1 文字 1 行で「文字 / Unicode コードポイント / UTF-8 バイト列 / 名前」を作る。
/// 2 つ目は、上限を超えて省略した文字数
pub fn code_rows(text: &str) -> (Vec<CodeRow>, usize) {
    let rows = text
        .chars()
        .take(MAX_CODE_ROWS)
        .map(|c| {
            let special = special_char(c);
            let mut buf = [0u8; 4];
            let utf8: Vec<String> = c.encode_utf8(&mut buf).bytes().map(|b| format!("{b:02X}")).collect();
            CodeRow {
                shown: special.as_ref().map_or_else(|| c.to_string(), |(s, _)| s.clone()),
                code: format!("U+{:04X}", c as u32),
                utf8: utf8.join(" "),
                name: special.map(|(_, n)| n).unwrap_or_default(),
            }
        })
        .collect();
    (rows, text.chars().count().saturating_sub(MAX_CODE_ROWS))
}

/// 改行コードを `\n` にそろえる(CRLF・単独の CR)
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// タブを、次のタブ位置(`width` 文字ごと)までの空白に展開する
pub fn expand_tabs(text: &str, width: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut col = 0;
    for c in text.chars() {
        match c {
            '\t' => {
                let n = width - col % width;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\n' => {
                out.push('\n');
                col = 0;
            }
            _ => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

/// 通常 / 制御文字表示のときに描画するテキストを作る(長すぎる分は省略)
pub fn display_text(text: &str, view: View) -> String {
    let total = text.chars().count();
    let body: String = text.chars().take(MAX_DISPLAY_CHARS).collect();
    let mut shown = if view == View::Controls {
        expand_tabs(&show_controls(&body), 4)
    } else {
        expand_tabs(&normalize_newlines(&body), 4)
    };
    if total > MAX_DISPLAY_CHARS {
        shown.push_str(&format!("\n…(以降 {} 文字は省略)", total - MAX_DISPLAY_CHARS));
    }
    shown
}

pub fn type_label(content: &Content) -> &'static str {
    match content {
        Content::Text(_) => "テキスト",
        Content::Image(_) => "画像",
        Content::Files(_) => "ファイル",
        Content::Unknown => "クリップボード",
        Content::Error(_) => "エラー",
    }
}

/// フッターに出す、内容の詳細(文字数・画像サイズ・ファイル件数)
pub fn detail(content: &Content) -> String {
    match content {
        Content::Text(t) => format!(
            "{} 文字 · {} 行 · UTF-8 {}B",
            t.chars().count(),
            t.lines().count(),
            t.len()
        ),
        Content::Image(d) => format!("{}×{}", d.width, d.height),
        Content::Files(f) => format!("{} 件", f.len()),
        Content::Unknown | Content::Error(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_are_visible_and_keep_line_breaks() {
        assert_eq!(show_controls("a\r\nb\tc\nd"), "a⟦CR⟧⟦LF⟧\nb⟦TAB⟧\tc⟦LF⟧\nd");
        assert_eq!(show_controls("a b\u{3000}c\u{200B}"), "a·b□c⟦ZWSP⟧");
        assert_eq!(show_controls("\u{0}\u{7F}"), "⟦NUL⟧⟦DEL⟧");
    }

    #[test]
    fn code_rows_describe_each_char() {
        let (rows, omitted) = code_rows("Aあ\n");
        assert_eq!(omitted, 0);
        assert_eq!(rows[0].code, "U+0041");
        assert_eq!(rows[0].utf8, "41");
        assert_eq!(rows[1].utf8, "E3 81 82");
        assert_eq!(rows[2].name, "LF");
    }

    #[test]
    fn code_rows_are_capped() {
        let text = "x".repeat(MAX_CODE_ROWS + 5);
        let (rows, omitted) = code_rows(&text);
        assert_eq!((rows.len(), omitted), (MAX_CODE_ROWS, 5));
    }

    #[test]
    fn tabs_expand_to_stops() {
        assert_eq!(expand_tabs("a\tb\n\tc", 4), "a   b\n    c");
    }

    #[test]
    fn newlines_are_normalized() {
        assert_eq!(normalize_newlines("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn long_text_is_truncated() {
        let text = "x".repeat(MAX_DISPLAY_CHARS + 3);
        assert!(display_text(&text, View::Normal).ends_with("(以降 3 文字は省略)"));
    }

    #[test]
    fn detail_counts_text() {
        let c = Content::Text("あい\nう".into());
        assert_eq!(detail(&c), "4 文字 · 2 行 · UTF-8 10B");
    }
}
