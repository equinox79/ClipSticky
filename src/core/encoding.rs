//! 文字コードの変換(エンコード別のバイト列、文字化けの復元)

use encoding_rs::{EUC_JP, Encoding, ISO_2022_JP, SHIFT_JIS, UTF_8, WINDOWS_1252};

/// 調べるテキストの上限(文字数)
pub const MAX_CHARS: usize = 20_000;
/// 「変換できない文字」として並べる数の上限
const MAX_UNMAPPABLE_LISTED: usize = 16;
/// 文字化けの復元に使うテキストの上限(文字数)
const MAX_MOJIBAKE_CHARS: usize = 4_000;
/// 文字化けの候補の最大数
const MAX_CANDIDATES: usize = 5;

/// 1 つの文字コードで書き出した結果
#[derive(Debug, PartialEq)]
pub struct EncodingReport {
    pub label: &'static str,
    pub bytes: Vec<u8>,
    /// その文字コードに無い文字(重複なし)。バイト列には含まれない
    pub unmappable: Vec<char>,
    /// 無い文字の種類が、`MAX_UNMAPPABLE_LISTED` を超えて省略されたか
    pub unmappable_truncated: bool,
}

fn is_mappable(enc: &'static Encoding, c: char) -> bool {
    let mut buf = [0u8; 4];
    let (_, _, had_errors) = enc.encode(c.encode_utf8(&mut buf));
    !had_errors
}

fn encode_with(label: &'static str, enc: &'static Encoding, text: &str) -> EncodingReport {
    let mut kept = String::with_capacity(text.len());
    let mut unmappable = Vec::new();
    let mut truncated = false;
    for c in text.chars() {
        if is_mappable(enc, c) {
            kept.push(c);
        } else if !unmappable.contains(&c) {
            if unmappable.len() < MAX_UNMAPPABLE_LISTED {
                unmappable.push(c);
            } else {
                truncated = true;
            }
        }
    }
    let (bytes, _, _) = enc.encode(&kept);
    EncodingReport { label, bytes: bytes.into_owned(), unmappable, unmappable_truncated: truncated }
}

fn utf16_bytes(text: &str, to_bytes: fn(u16) -> [u8; 2]) -> Vec<u8> {
    text.encode_utf16().flat_map(to_bytes).collect()
}

/// UTF-8 / UTF-16 / Shift_JIS / EUC-JP / ISO-2022-JP で書き出したバイト列を作る。
/// Shift_JIS は Windows の CP932(Windows-31J)
pub fn encode_reports(text: &str) -> Vec<EncodingReport> {
    let text: String = text.chars().take(MAX_CHARS).collect();
    let plain = |label, bytes| EncodingReport { label, bytes, unmappable: Vec::new(), unmappable_truncated: false };
    vec![
        plain("UTF-8", text.as_bytes().to_vec()),
        plain("UTF-16LE", utf16_bytes(&text, u16::to_le_bytes)),
        plain("UTF-16BE", utf16_bytes(&text, u16::to_be_bytes)),
        encode_with("Shift_JIS (CP932)", SHIFT_JIS, &text),
        encode_with("EUC-JP", EUC_JP, &text),
        encode_with("ISO-2022-JP", ISO_2022_JP, &text),
    ]
}

/// バイト列を `E3 81 82` のような 16 進の文字列にする
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
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

    fn report<'a>(reports: &'a [EncodingReport], label: &str) -> &'a EncodingReport {
        reports.iter().find(|r| r.label.starts_with(label)).unwrap()
    }

    #[test]
    fn encodes_in_each_encoding() {
        let r = encode_reports("Aあ");
        assert_eq!(hex(&report(&r, "UTF-8").bytes), "41 E3 81 82");
        assert_eq!(hex(&report(&r, "UTF-16LE").bytes), "41 00 42 30");
        assert_eq!(hex(&report(&r, "UTF-16BE").bytes), "00 41 30 42");
        assert_eq!(hex(&report(&r, "Shift_JIS").bytes), "41 82 A0");
        assert_eq!(hex(&report(&r, "EUC-JP").bytes), "41 A4 A2");
        // ISO-2022-JP は、漢字の前後にエスケープシーケンス(ESC $ B … ESC ( B)が入る
        assert_eq!(hex(&report(&r, "ISO-2022-JP").bytes), "41 1B 24 42 24 22 1B 28 42");
    }

    #[test]
    fn unmappable_characters_are_reported() {
        // 絵文字は、Shift_JIS・EUC-JP・ISO-2022-JP のどれにも無い。UTF 系にはある
        let r = encode_reports("a😀b");
        for label in ["Shift_JIS", "EUC-JP", "ISO-2022-JP"] {
            let rep = report(&r, label);
            assert_eq!(rep.unmappable, vec!['😀'], "{label}");
            assert_eq!(hex(&rep.bytes).replace(' ', "").contains("F09F"), false);
        }
        assert!(report(&r, "UTF-8").unmappable.is_empty());
        // 丸数字は、Shift_JIS(CP932)にも、EUC-JP・ISO-2022-JP(NEC 拡張を含む)にもある
        let r = encode_reports("①");
        assert!(report(&r, "Shift_JIS").unmappable.is_empty());
        assert!(report(&r, "ISO-2022-JP").unmappable.is_empty());
    }

    #[test]
    fn unmappable_characters_are_deduplicated_and_capped() {
        let text: String = ('\u{1F600}'..='\u{1F640}').chain(std::iter::repeat_n('\u{1F600}', 5)).collect();
        let rep = encode_reports(&text);
        let sjis = report(&rep, "Shift_JIS");
        assert_eq!(sjis.unmappable.len(), MAX_UNMAPPABLE_LISTED);
        assert!(sjis.unmappable_truncated);
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

    #[test]
    fn hex_format() {
        assert_eq!(hex(&[0xE3, 0x81, 0x02]), "E3 81 02");
        assert_eq!(hex(&[]), "");
    }
}
