//! 文字エンコードの変換(テキストを別の文字コードのバイト列にする、文字化けの復元)

use encoding_rs::{EUC_JP, Encoding, ISO_2022_JP, SHIFT_JIS, UTF_8, WINDOWS_1252};

/// 文字化けの復元に使うテキストの上限(文字数)
const MAX_MOJIBAKE_CHARS: usize = 4_000;
/// 文字化けの候補の最大数
const MAX_CANDIDATES: usize = 5;
/// 「変換できない文字」として知らせる文字の数の上限
const MAX_UNMAPPABLE_LISTED: usize = 8;

/// 変換先の文字エンコード
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    ShiftJis,
    EucJp,
    Iso2022Jp,
}

/// (値, メニューの項目 ID に使う名前, 表示名)。Shift_JIS は Windows の CP932(Windows-31J)
pub const TARGET_ENCODINGS: [(TargetEncoding, &str, &str); 6] = [
    (TargetEncoding::Utf8, "utf8", "UTF-8"),
    (TargetEncoding::Utf16Le, "utf16le", "UTF-16LE"),
    (TargetEncoding::Utf16Be, "utf16be", "UTF-16BE"),
    (TargetEncoding::ShiftJis, "sjis", "Shift_JIS (CP932)"),
    (TargetEncoding::EucJp, "eucjp", "EUC-JP"),
    (TargetEncoding::Iso2022Jp, "iso2022jp", "ISO-2022-JP"),
];

/// バイト列を文字列にする書式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteFormat {
    Hex,
    Base64,
    Percent,
    /// 1 バイト = 1 文字(U+0000〜U+00FF)。Latin-1 として読めば、元のバイト列に戻る
    Latin1,
}

pub const BYTE_FORMATS: [(ByteFormat, &str, &str); 4] = [
    (ByteFormat::Hex, "hex", "16 進(E3 81 82)"),
    (ByteFormat::Base64, "base64", "Base64(44GC)"),
    (ByteFormat::Percent, "percent", "パーセントエンコード(%E3%81%82)"),
    (ByteFormat::Latin1, "latin1", "1 バイト = 1 文字(日本語 → ÆüËÜ¸ì)"),
];

pub fn encoding_label(e: TargetEncoding) -> &'static str {
    TARGET_ENCODINGS.iter().find(|x| x.0 == e).map_or("", |x| x.2)
}

pub fn format_label(f: ByteFormat) -> &'static str {
    BYTE_FORMATS.iter().find(|x| x.0 == f).map_or("", |x| x.2)
}

fn legacy(e: TargetEncoding) -> Option<&'static Encoding> {
    match e {
        TargetEncoding::ShiftJis => Some(SHIFT_JIS),
        TargetEncoding::EucJp => Some(EUC_JP),
        TargetEncoding::Iso2022Jp => Some(ISO_2022_JP),
        _ => None,
    }
}

fn is_mappable(enc: &'static Encoding, c: char) -> bool {
    let mut buf = [0u8; 4];
    let (_, _, had_errors) = enc.encode(c.encode_utf8(&mut buf));
    !had_errors
}

/// テキストを、指定の文字エンコードのバイト列にする。
/// その文字エンコードに無い文字があるときは、(変換せずに)その文字の一覧(重複なし)を返す
pub fn encode_text(text: &str, encoding: TargetEncoding) -> Result<Vec<u8>, Vec<char>> {
    match encoding {
        TargetEncoding::Utf8 => Ok(text.as_bytes().to_vec()),
        TargetEncoding::Utf16Le => Ok(text.encode_utf16().flat_map(u16::to_le_bytes).collect()),
        TargetEncoding::Utf16Be => Ok(text.encode_utf16().flat_map(u16::to_be_bytes).collect()),
        _ => {
            let enc = legacy(encoding).expect("legacy encoding");
            let mut unmappable: Vec<char> = Vec::new();
            for c in text.chars() {
                if !unmappable.contains(&c) && !is_mappable(enc, c) {
                    unmappable.push(c);
                    if unmappable.len() > MAX_UNMAPPABLE_LISTED {
                        break;
                    }
                }
            }
            if !unmappable.is_empty() {
                return Err(unmappable);
            }
            Ok(enc.encode(text).0.into_owned())
        }
    }
}

/// バイト列を `E3 81 82` のような 16 進の文字列にする
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(TABLE[(n >> 18 & 63) as usize] as char);
        out.push(TABLE[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// URL などで使う %XX の形。英数字と - . _ ~ はそのまま
fn percent(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub fn format_bytes(bytes: &[u8], format: ByteFormat) -> String {
    match format {
        ByteFormat::Hex => hex(bytes),
        ByteFormat::Base64 => base64(bytes),
        ByteFormat::Percent => percent(bytes),
        ByteFormat::Latin1 => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// その書式では、バイト列を文字列にできない理由(できるなら None)。
/// 「1 バイト = 1 文字」は、NUL(0x00)を含むと、クリップボードの文字列が途中で終わってしまうため使えない
/// (UTF-16 の英数字などは、上位バイトが 0x00 になる)
pub fn format_unavailable(bytes: &[u8], format: ByteFormat) -> Option<&'static str> {
    (format == ByteFormat::Latin1 && bytes.contains(&0))
        .then_some("NUL(0x00)を含むため、この書式ではコピーできません")
}

/// 変換できなかった文字を、通知に出す形(「😀 ① …」)にする
pub fn unmappable_summary(chars: &[char]) -> String {
    let shown: Vec<String> = chars.iter().take(MAX_UNMAPPABLE_LISTED).map(|c| c.to_string()).collect();
    let more = if chars.len() > MAX_UNMAPPABLE_LISTED { " …" } else { "" };
    format!("{}{more}", shown.join(" "))
}

/// 文字化けを直した候補
#[derive(Debug, PartialEq)]
pub struct Candidate {
    /// 読み違えた文字コード(文字化けしたときに、バイト列が誤って読まれた側)
    pub misread_as: &'static str,
    /// 本来の文字コード
    pub truly: &'static str,
    /// 直した結果
    pub text: String,
}

/// 普段よく使う漢字(JIS X 0208 の第 1 水準。EUC-JP の 1 バイト目が 0xB0〜0xCF)か。
/// 読み違えで出てくる無意味な漢字は、珍しい漢字(第 2 水準など)が多く混ざるため、見分けに使う
fn is_common_kanji(c: char) -> bool {
    let mut buf = [0u8; 4];
    let (bytes, _, had_errors) = EUC_JP.encode(c.encode_utf8(&mut buf));
    !had_errors && bytes.len() == 2 && (0xB0..=0xCF).contains(&bytes[0])
}

/// 文字として自然かの点数(0.0〜1.0)。かな・よく使う漢字・英数・空白・全角記号は 1、
/// 珍しい漢字・半角カナ・記号の寄せ集め・置換文字・制御文字は低くする
fn plausibility(text: &str) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0.0f32;
    for c in text.chars() {
        count += 1.0;
        total += match c {
            '\u{FFFD}' => 0.0,
            '\n' | '\r' | '\t' | ' '..='~' => 1.0,
            c if c.is_control() => 0.0,
            '\u{3000}'..='\u{30FF}' | '\u{FF01}'..='\u{FF5E}' => 1.0,
            '\u{4E00}'..='\u{9FFF}' => {
                if is_common_kanji(c) {
                    1.0
                } else {
                    0.3
                }
            }
            '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}' => 0.8,
            '\u{FF61}'..='\u{FF9F}' => 0.2,
            '\u{0080}'..='\u{024F}' | '\u{2000}'..='\u{206F}' | '\u{2190}'..='\u{23FF}' | '\u{2500}'..='\u{27BF}' => 0.1,
            _ => 0.5,
        };
    }
    if count == 0.0 { 0.0 } else { total / count }
}

/// 文字化けした(ように見える)テキストを、別の文字コードとして読み直した候補を返す。
///
/// 「本来の文字コード(truly)のバイト列を、誤って別の文字コード(misread_as)で読んだ」とみなし、
/// テキストを misread_as で書き出し、truly で読み直す。読み直した結果が自然な文字になるものだけを、
/// 点数の高い順に並べる。
pub fn mojibake_candidates(text: &str) -> Vec<Candidate> {
    let text: String = text.chars().take(MAX_MOJIBAKE_CHARS).collect();
    let misreads: [(&'static str, &'static Encoding); 3] =
        [("Windows-1252", WINDOWS_1252), ("Shift_JIS", SHIFT_JIS), ("EUC-JP", EUC_JP)];
    let trulys: [(&'static str, &'static Encoding); 4] =
        [("UTF-8", UTF_8), ("Shift_JIS", SHIFT_JIS), ("EUC-JP", EUC_JP), ("ISO-2022-JP", ISO_2022_JP)];

    let mut found: Vec<(f32, Candidate)> = Vec::new();
    for (mis_label, mis) in misreads {
        let (bytes, _, had_errors) = mis.encode(&text);
        if had_errors {
            continue; // その文字コードに無い文字がある = その読み違えではない
        }
        for (truly_label, truly) in trulys {
            if std::ptr::eq(mis, truly) {
                continue;
            }
            let (decoded, had_errors) = truly.decode_without_bom_handling(&bytes);
            // 末尾などの欠けで、置換文字が 1 文字だけ残るのは許す
            let replaced = decoded.chars().filter(|&c| c == '\u{FFFD}').count();
            if (had_errors && replaced > 1) || decoded == text {
                continue;
            }
            let score = plausibility(&decoded);
            if score < 0.7 || found.iter().any(|(_, c)| c.text == decoded) {
                continue;
            }
            found.push((
                score,
                Candidate { misread_as: mis_label, truly: truly_label, text: decoded.into_owned() },
            ));
        }
    }
    // 点数の高い順。同点なら、UTF-8 を先に
    found.sort_by(|a, b| {
        b.0.total_cmp(&a.0).then_with(|| (b.1.truly == "UTF-8").cmp(&(a.1.truly == "UTF-8")))
    });
    found.into_iter().map(|(_, c)| c).take(MAX_CANDIDATES).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(text: &str, e: TargetEncoding, f: ByteFormat) -> String {
        format_bytes(&encode_text(text, e).expect("encodable"), f)
    }

    #[test]
    fn encodes_in_each_encoding() {
        use TargetEncoding::*;
        assert_eq!(encode("Aあ", Utf8, ByteFormat::Hex), "41 E3 81 82");
        assert_eq!(encode("Aあ", Utf16Le, ByteFormat::Hex), "41 00 42 30");
        assert_eq!(encode("Aあ", Utf16Be, ByteFormat::Hex), "00 41 30 42");
        assert_eq!(encode("Aあ", ShiftJis, ByteFormat::Hex), "41 82 A0");
        assert_eq!(encode("Aあ", EucJp, ByteFormat::Hex), "41 A4 A2");
        // ISO-2022-JP は、漢字の前後にエスケープシーケンス(ESC $ B … ESC ( B)が入る
        assert_eq!(encode("Aあ", Iso2022Jp, ByteFormat::Hex), "41 1B 24 42 24 22 1B 28 42");
        // 絵文字は UTF 系ならそのまま(サロゲートペア)
        assert_eq!(encode("😀", Utf16Le, ByteFormat::Hex), "3D D8 00 DE");
    }

    #[test]
    fn unmappable_characters_stop_the_conversion() {
        // 絵文字は、Shift_JIS・EUC-JP・ISO-2022-JP のどれにも無い。変換せずに、その文字を知らせる
        for e in [TargetEncoding::ShiftJis, TargetEncoding::EucJp, TargetEncoding::Iso2022Jp] {
            assert_eq!(encode_text("a😀b😀", e), Err(vec!['😀']), "{e:?}");
        }
        assert!(encode_text("a😀b", TargetEncoding::Utf8).is_ok());
        // 丸数字は、Shift_JIS(CP932)にも、EUC-JP・ISO-2022-JP(NEC 拡張を含む)にもある
        assert!(encode_text("①", TargetEncoding::ShiftJis).is_ok());
        assert!(encode_text("①", TargetEncoding::Iso2022Jp).is_ok());
    }

    #[test]
    fn unmappable_list_is_capped() {
        let text: String = ('\u{1F600}'..='\u{1F64F}').collect();
        let Err(chars) = encode_text(&text, TargetEncoding::ShiftJis) else { panic!("should fail") };
        assert!(chars.len() <= MAX_UNMAPPABLE_LISTED + 1);
        assert!(unmappable_summary(&chars).ends_with('…'));
        assert_eq!(unmappable_summary(&['😀', '①']), "😀 ①");
    }

    #[test]
    fn byte_formats() {
        assert_eq!(hex(&[0xE3, 0x81, 0x02]), "E3 81 02");
        assert_eq!(hex(&[]), "");
        // Base64(RFC 4648 のテストベクタ)
        for (input, expected) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(format_bytes(input.as_bytes(), ByteFormat::Base64), expected, "{input:?}");
        }
        assert_eq!(encode("あ", TargetEncoding::Utf8, ByteFormat::Base64), "44GC");
        // パーセントエンコード
        assert_eq!(encode("日本 a-b_c.d~", TargetEncoding::Utf8, ByteFormat::Percent), "%E6%97%A5%E6%9C%AC%20a-b_c.d~");
        assert_eq!(encode("日", TargetEncoding::ShiftJis, ByteFormat::Percent), "%93%FA");
    }

    #[test]
    fn one_byte_per_char() {
        // EUC-JP の「日本語」(C6 FC CB DC B8 EC)を、1 バイト = 1 文字にする
        assert_eq!(encode("日本語", TargetEncoding::EucJp, ByteFormat::Latin1), "ÆüËÜ¸ì");
        // Latin-1 として読み直すと、元のバイト列に戻る(往復)
        let bytes = encode_text("日本語のテキスト", TargetEncoding::EucJp).unwrap();
        let text = format_bytes(&bytes, ByteFormat::Latin1);
        let back: Vec<u8> = text.chars().map(|c| c as u32 as u8).collect();
        assert_eq!(back, bytes);
        // 文字化けの復元でも、同じ文字列から「日本語のテキスト」に戻せる
        assert!(mojibake_candidates(&text).iter().any(|c| c.text == "日本語のテキスト"));
    }

    #[test]
    fn one_byte_per_char_is_refused_with_nul() {
        let utf16 = encode_text("A", TargetEncoding::Utf16Le).unwrap();
        assert!(format_unavailable(&utf16, ByteFormat::Latin1).is_some());
        assert!(format_unavailable(&utf16, ByteFormat::Hex).is_none());
        let euc = encode_text("日本語", TargetEncoding::EucJp).unwrap();
        assert!(format_unavailable(&euc, ByteFormat::Latin1).is_none());
    }

    #[test]
    fn ids_are_unique() {
        let ids: std::collections::HashSet<_> = TARGET_ENCODINGS.iter().map(|x| x.1).collect();
        assert_eq!(ids.len(), TARGET_ENCODINGS.len());
        let ids: std::collections::HashSet<_> = BYTE_FORMATS.iter().map(|x| x.1).collect();
        assert_eq!(ids.len(), BYTE_FORMATS.len());
        assert_eq!(encoding_label(TargetEncoding::ShiftJis), "Shift_JIS (CP932)");
        assert_eq!(format_label(ByteFormat::Base64), "Base64(44GC)");
    }
    #[test]
    fn utf8_misread_as_shift_jis() {
        // 「こんにちは」の UTF-8 を、Shift_JIS として読むと文字化けする(よくある例)
        let (bytes, _, _) = UTF_8.encode("こんにちは");
        let (garbled, _) = SHIFT_JIS.decode_without_bom_handling(&bytes);
        let found = mojibake_candidates(&garbled);
        assert!(
            found.iter().any(|c| c.text == "こんにちは" && c.truly == "UTF-8" && c.misread_as == "Shift_JIS"),
            "{garbled} -> {found:?}"
        );
    }

    #[test]
    fn nonsense_restorations_are_rejected() {
        // 「こんにちは」の化けた文字列からは、UTF-8 として直した 1 件だけが出る
        // (Shift_JIS を EUC-JP と読むと、珍しい漢字ばかりの無意味な文字列になるが、それは出さない)
        let (bytes, _, _) = UTF_8.encode("こんにちは");
        let (garbled, _) = SHIFT_JIS.decode_without_bom_handling(&bytes);
        let found = mojibake_candidates(&garbled);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].text, "こんにちは");
    }

    #[test]
    fn common_kanji_detection() {
        assert!(is_common_kanji('日') && is_common_kanji('本') && is_common_kanji('語'));
        assert!(!is_common_kanji('裔') || !is_common_kanji('瘤')); // 第 2 水準を含む
        assert!(!is_common_kanji('あ'));
    }

    #[test]
    fn utf8_misread_as_latin1() {
        let (bytes, _, _) = UTF_8.encode("こんにちは");
        let (garbled, _) = WINDOWS_1252.decode_without_bom_handling(&bytes);
        let found = mojibake_candidates(&garbled);
        assert_eq!(found.first().map(|c| c.text.as_str()), Some("こんにちは"), "{found:?}");
    }

    #[test]
    fn shift_jis_misread_as_latin1() {
        let (bytes, _, _) = SHIFT_JIS.encode("クリップ");
        let (garbled, _) = WINDOWS_1252.decode_without_bom_handling(&bytes);
        let found = mojibake_candidates(&garbled);
        assert!(found.iter().any(|c| c.text == "クリップ" && c.truly == "Shift_JIS"), "{found:?}");
    }

    #[test]
    fn euc_jp_misread_as_latin1() {
        // 「日本語のテキスト」の EUC-JP を Windows-1252 で読むと、ÆüËܸì¤Î¥Æ¥­¥¹¥È になる
        let (bytes, _, _) = EUC_JP.encode("日本語のテキスト");
        let (garbled, _) = WINDOWS_1252.decode_without_bom_handling(&bytes);
        let found = mojibake_candidates(&garbled);
        assert!(found.iter().any(|c| c.text == "日本語のテキスト" && c.truly == "EUC-JP"), "{garbled} -> {found:?}");
    }

    #[test]
    fn normal_text_has_no_candidates() {
        for text in ["Hello, world", "こんにちは、世界。", "日本語と English の混在 123", "", "行1\r\n行2"] {
            assert!(mojibake_candidates(text).is_empty(), "{text:?} -> {:?}", mojibake_candidates(text));
        }
    }


}
