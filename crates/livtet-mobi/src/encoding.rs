//! Text decoding for MOBI metadata strings.
//!
//! Codepage 65001 is UTF-8 (decoded lossily); anything else is treated as
//! Windows-1252. A small entity decoder handles the HTML/XML escapes Amazon
//! embeds in titles and contributor names.

/// UTF-8 codepage.
pub const UTF8_CODEPAGE: u32 = 65001;

/// Windows-1252 mapping for 0x80–0x9F; `None` marks the five bytes with no
/// assigned character (0x81, 0x8D, 0x8F, 0x90, 0x9D).
const CP1252_80_9F: [Option<char>; 32] = [
    Some('\u{20AC}'), // 80 EURO SIGN
    None,             // 81
    Some('\u{201A}'), // 82 SINGLE LOW-9 QUOTATION MARK
    Some('\u{0192}'), // 83 LATIN SMALL LETTER F WITH HOOK
    Some('\u{201E}'), // 84 DOUBLE LOW-9 QUOTATION MARK
    Some('\u{2026}'), // 85 HORIZONTAL ELLIPSIS
    Some('\u{2020}'), // 86 DAGGER
    Some('\u{2021}'), // 87 DOUBLE DAGGER
    Some('\u{02C6}'), // 88 MODIFIER LETTER CIRCUMFLEX ACCENT
    Some('\u{2030}'), // 89 PER MILLE SIGN
    Some('\u{0160}'), // 8A LATIN CAPITAL LETTER S WITH CARON
    Some('\u{2039}'), // 8B SINGLE LEFT-POINTING ANGLE QUOTATION MARK
    Some('\u{0152}'), // 8C LATIN CAPITAL LIGATURE OE
    None,             // 8D
    Some('\u{017D}'), // 8E LATIN CAPITAL LETTER Z WITH CARON
    None,             // 8F
    None,             // 90
    Some('\u{2018}'), // 91 LEFT SINGLE QUOTATION MARK
    Some('\u{2019}'), // 92 RIGHT SINGLE QUOTATION MARK
    Some('\u{201C}'), // 93 LEFT DOUBLE QUOTATION MARK
    Some('\u{201D}'), // 94 RIGHT DOUBLE QUOTATION MARK
    Some('\u{2022}'), // 95 BULLET
    Some('\u{2013}'), // 96 EN DASH
    Some('\u{2014}'), // 97 EM DASH
    Some('\u{02DC}'), // 98 SMALL TILDE
    Some('\u{2122}'), // 99 TRADE MARK SIGN
    Some('\u{0161}'), // 9A LATIN SMALL LETTER S WITH CARON
    Some('\u{203A}'), // 9B SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
    Some('\u{0153}'), // 9C LATIN SMALL LIGATURE OE
    None,             // 9D
    Some('\u{017E}'), // 9E LATIN SMALL LETTER Z WITH CARON
    Some('\u{0178}'), // 9F LATIN CAPITAL LETTER Y WITH DIAERESIS
];

/// Decode `bytes` with the MOBI `text_encoding` codepage.
pub fn decode(bytes: &[u8], codepage: u32) -> String {
    if codepage == UTF8_CODEPAGE {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        decode_windows_1252(bytes)
    }
}

fn decode_windows_1252(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            0x00..=0x7F | 0xA0..=0xFF => out.push(b as char),
            0x80..=0x9F => match CP1252_80_9F[(b - 0x80) as usize] {
                Some(c) => out.push(c),
                None => out.push('\u{FFFD}'),
            },
        }
    }
    out
}

/// Resolve HTML/XML character references in `s`.
///
/// Handles `&amp; &lt; &gt; &quot; &apos; &nbsp;`, decimal `&#NNN;` and hex
/// `&#xHH;`; unknown entities (and stray `&` without a closing `;`) pass
/// through verbatim.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        match after.find(';') {
            Some(semi) => {
                let entity = &after[..semi];
                match resolve_entity(entity) {
                    Some(c) => out.push(c),
                    None if entity == "nbsp" => out.push('\u{00A0}'),
                    None => {
                        out.push('&');
                        out.push_str(entity);
                        out.push(';');
                    }
                }
                rest = &after[semi + 1..];
            }
            None => {
                out.push_str(&rest[amp..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn resolve_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => entity
            .strip_prefix('#')
            .and_then(|digits| parse_char_ref(digits, 10)),
    }
}

fn parse_char_ref(digits: &str, radix: u32) -> Option<char> {
    if digits.is_empty() {
        return None;
    }
    let (digits, radix) = match digits
        .strip_prefix('x')
        .or_else(|| digits.strip_prefix('X'))
    {
        Some(hex) if !hex.is_empty() => (hex, 16),
        Some(_) => return None,
        None => (digits, radix),
    };
    u32::from_str_radix(digits, radix)
        .ok()
        .and_then(char::from_u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp1252_high_range_maps_to_latin1() {
        // 0xA0–0xFF decode as Latin-1 (U+00A0–U+00FF).
        for b in 0xA0..=0xFFu8 {
            assert_eq!(decode(&[b], 1252), (b as char).to_string(), "byte {b:#04X}");
        }
    }

    #[test]
    fn cp1252_ascii_passthrough() {
        for b in 0x00..=0x7Fu8 {
            assert_eq!(decode(&[b], 1252), (b as char).to_string(), "byte {b:#04X}");
        }
    }

    #[test]
    fn cp1252_80_to_9f_table() {
        let expected: [(u8, Option<char>); 32] = [
            (0x80, Some('\u{20AC}')),
            (0x81, None),
            (0x82, Some('\u{201A}')),
            (0x83, Some('\u{0192}')),
            (0x84, Some('\u{201E}')),
            (0x85, Some('\u{2026}')),
            (0x86, Some('\u{2020}')),
            (0x87, Some('\u{2021}')),
            (0x88, Some('\u{02C6}')),
            (0x89, Some('\u{2030}')),
            (0x8A, Some('\u{0160}')),
            (0x8B, Some('\u{2039}')),
            (0x8C, Some('\u{0152}')),
            (0x8D, None),
            (0x8E, Some('\u{017D}')),
            (0x8F, None),
            (0x90, None),
            (0x91, Some('\u{2018}')),
            (0x92, Some('\u{2019}')),
            (0x93, Some('\u{201C}')),
            (0x94, Some('\u{201D}')),
            (0x95, Some('\u{2022}')),
            (0x96, Some('\u{2013}')),
            (0x97, Some('\u{2014}')),
            (0x98, Some('\u{02DC}')),
            (0x99, Some('\u{2122}')),
            (0x9A, Some('\u{0161}')),
            (0x9B, Some('\u{203A}')),
            (0x9C, Some('\u{0153}')),
            (0x9D, None),
            (0x9E, Some('\u{017E}')),
            (0x9F, Some('\u{0178}')),
        ];
        for (byte, mapped) in expected {
            let want = mapped.unwrap_or('\u{FFFD}').to_string();
            assert_eq!(decode(&[byte], 1252), want, "byte {byte:#04X}");
        }
    }

    #[test]
    fn entities_named_decimal_and_hex() {
        assert_eq!(
            decode_entities(
                "A &#38; B &#x26; C &amp; D &lt;E&gt; &quot;F&quot; &apos;G&apos; H&nbsp;I"
            ),
            "A & B & C & D <E> \"F\" 'G' H\u{00A0}I"
        );
    }

    #[test]
    fn entities_unknown_pass_through() {
        assert_eq!(decode_entities("a &bogus; b & c"), "a &bogus; b & c");
    }
}
