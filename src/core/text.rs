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

/// 表示用のテキストの一片。`marker` が true のものは、制御文字などを表す記号(薄い色で描く)
#[derive(Debug, PartialEq)]
pub struct Piece {
    pub text: String,
    pub marker: bool,
}

/// 同じ種類が続くなら、末尾の片にまとめて足す
fn push_piece(out: &mut Vec<Piece>, text: &str, marker: bool) {
    match out.last_mut() {
        Some(last) if last.marker == marker => last.text.push_str(text),
        _ => out.push(Piece { text: text.to_string(), marker }),
    }
}

/// タブ位置の幅(桁数)
const TAB_WIDTH: usize = 4;

/// 制御文字をエディタのように記号で表す。記号は `marker` の片になる。
///
/// - 改行: CRLF は `↵`、LF は `↓`、単独の CR は `←`。記号の後ろで実際にも改行する
/// - タブ: `→`(次のタブ位置まで空白で埋める)
/// - 半角スペースは `·`、全角スペースは `□`、NBSP は `°`
/// - そのほかの制御文字や不可視文字は `⟦名前⟧`
/// - 末尾に `[EOF]`(`eof` が true のとき。省略して切った場合は付けない)
pub fn controls_pieces(text: &str, eof: bool) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut col = 0usize;
    let mut chars = text.chars().peekable();
    let mut plain = [0u8; 4];
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                push_piece(&mut out, "↵", true);
                push_piece(&mut out, "\n", false);
                col = 0;
            }
            '\n' => {
                push_piece(&mut out, "↓", true);
                push_piece(&mut out, "\n", false);
                col = 0;
            }
            '\r' => {
                push_piece(&mut out, "←", true);
                push_piece(&mut out, "\n", false);
                col = 0;
            }
            '\t' => {
                let n = TAB_WIDTH - col % TAB_WIDTH;
                push_piece(&mut out, &format!("→{}", " ".repeat(n - 1)), true);
                col += n;
            }
            ' ' => {
                push_piece(&mut out, "·", true);
                col += 1;
            }
            '\u{3000}' => {
                push_piece(&mut out, "□", true);
                col += 2;
            }
            '\u{A0}' => {
                push_piece(&mut out, "°", true);
                col += 1;
            }
            _ => match special_char(c) {
                Some((sym, _)) => push_piece(&mut out, &sym, true),
                None => {
                    push_piece(&mut out, c.encode_utf8(&mut plain), false);
                    col += 1;
                }
            },
        }
    }
    if eof {
        push_piece(&mut out, "[EOF]", true);
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
pub fn display_pieces(text: &str, view: View) -> Vec<Piece> {
    let total = text.chars().count();
    let truncated = total > MAX_DISPLAY_CHARS;
    let body: String = text.chars().take(MAX_DISPLAY_CHARS).collect();
    let mut pieces = if view == View::Controls {
        controls_pieces(&body, !truncated)
    } else {
        vec![Piece { text: expand_tabs(&normalize_newlines(&body), TAB_WIDTH), marker: false }]
    };
    if truncated {
        push_piece(&mut pieces, &format!("\n…(以降 {} 文字は省略)", total - MAX_DISPLAY_CHARS), true);
    }
    pieces
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

    fn piece(text: &str, marker: bool) -> Piece {
        Piece { text: text.to_string(), marker }
    }

    #[test]
    fn newlines_are_shown_like_an_editor() {
        // CRLF は ↵、LF は ↓、単独の CR は ←。記号の後ろで実際にも改行する
        assert_eq!(
            controls_pieces("a\r\nb\nc\rd", true),
            vec![
                piece("a", false),
                piece("↵", true),
                piece("\nb", false),
                piece("↓", true),
                piece("\nc", false),
                piece("←", true),
                piece("\nd", false),
                piece("[EOF]", true),
            ]
        );
    }

    #[test]
    fn tabs_fill_to_the_next_tab_stop() {
        // 桁 1 のタブは 3 桁分、桁 0 のタブは 4 桁分
        assert_eq!(
            controls_pieces("a\tb\n\tc", false),
            vec![
                piece("a", false),
                piece("→  ", true),
                piece("b", false),
                piece("↓", true),
                piece("\n", false),
                piece("→   ", true),
                piece("c", false),
            ]
        );
    }

    #[test]
    fn spaces_and_invisibles_are_markers() {
        assert_eq!(
            controls_pieces("a b\u{3000}c\u{A0}\u{200B}", false),
            vec![
                piece("a", false),
                piece("·", true),
                piece("b", false),
                piece("□", true),
                piece("c", false),
                piece("°⟦ZWSP⟧", true),
            ]
        );
        assert_eq!(controls_pieces("\u{0}\u{7F}", false), vec![piece("⟦NUL⟧⟦DEL⟧", true)]);
    }

    #[test]
    fn eof_marker_is_omitted_when_truncated() {
        let text = "x".repeat(MAX_DISPLAY_CHARS + 3);
        let pieces = display_pieces(&text, View::Controls);
        assert!(pieces.iter().all(|p| p.text != "[EOF]"));
        assert!(pieces.last().unwrap().text.ends_with("(以降 3 文字は省略)"));
        let short = display_pieces("ab", View::Controls);
        assert_eq!(short.last().unwrap().text, "[EOF]");
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
        let pieces = display_pieces(&text, View::Normal);
        assert!(pieces.last().unwrap().text.ends_with("(以降 3 文字は省略)"));
    }

    #[test]
    fn detail_counts_text() {
        let c = Content::Text("あい\nう".into());
        assert_eq!(detail(&c), "4 文字 · 2 行 · UTF-8 10B");
    }
}
