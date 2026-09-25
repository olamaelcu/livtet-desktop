//! Tolerant, self-contained XML reading for OCF/OPF/encryption documents.
//!
//! Real-world EPUBs in the wild are frequently malformed: stray `&`, mixed
//! encodings, illegal control characters, and unclosed tags. This module
//! normalizes a document just enough for [`quick_xml`] to parse it, then
//! exposes a small owned tree so the rest of the crate never has to touch
//! `quick_xml` types.
//!
//! Matching is namespace-agnostic by *local name*; OCF/OPF documents use
//! consistent local names across prefixes, so this keeps the parser simple
//! while remaining correct for real files.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::EpubError;

/// Maximum element nesting depth accepted while parsing.
///
/// Tree traversal (`descendants`/`find`/`text`) and the derived `Drop` recurse
/// once per level, so a hostile document with hundreds of thousands of nested
/// tags could otherwise abort the process with a stack overflow. Rejecting at
/// parse time keeps the built tree shallow enough for all of them.
pub(crate) const MAX_DEPTH: usize = 512;

/// One attribute of an [`Element`]. `name` keeps the raw qualified name while
/// `local`/`prefix` split it (so `opf:role` and `role` can both be matched).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Attr {
    pub name: String,
    pub local: String,
    pub prefix: Option<String>,
    pub value: String,
}

/// An owned XML element with its children and its own direct text.
#[derive(Debug, Clone)]
pub(crate) struct Element {
    pub local: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Element>,
    text: String,
}

impl Element {
    /// The first attribute whose local name matches (namespace-agnostic).
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.local == local)
            .map(|a| a.value.as_str())
    }

    /// Every descendant (including `self`) in document order.
    pub fn descendants(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a Element>) {
        out.push(self);
        for child in &self.children {
            child.collect(out);
        }
    }

    /// First element with the given local name, depth-first.
    pub fn find(&self, local: &str) -> Option<&Element> {
        if self.local == local {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(local))
    }

    /// All elements with the given local name, in document order.
    pub fn find_all(&self, local: &str) -> Vec<&Element> {
        self.descendants()
            .into_iter()
            .filter(|e| e.local == local)
            .collect()
    }

    /// Concatenated text of this element and all of its descendants.
    pub fn text(&self) -> String {
        let mut out = self.text.clone();
        for child in &self.children {
            out.push_str(&child.text());
        }
        out
    }

    fn from_start(start: &BytesStart<'_>) -> Self {
        let local = start.name().local_name().into_inner().to_string();
        let mut attrs = Vec::new();
        for attribute in start.attributes().flatten() {
            let key = attribute.key;
            attrs.push(Attr {
                name: key.into_inner().to_string(),
                local: key.local_name().into_inner().to_string(),
                prefix: key.prefix().map(|p| p.into_inner().to_string()),
                value: decode_entities(&attribute.value),
            });
        }
        Self {
            local,
            attrs,
            children: Vec::new(),
            text: String::new(),
        }
    }
}

/// Parse an XML document, tolerating real-world damage.
pub(crate) fn parse(bytes: &[u8]) -> Result<Element, EpubError> {
    let decoded = decode(bytes);
    let sanitized = sanitize(&decoded);
    parse_str(&sanitized)
}

fn parse_str(xml: &str) -> Result<Element, EpubError> {
    let mut reader = Reader::from_str(xml);
    {
        let config = reader.config_mut();
        config.check_end_names = false;
        config.allow_unmatched_ends = true;
        config.allow_dangling_amp = true;
        config.expand_empty_elements = false;
    }

    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let mut saw_element = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(EpubError::Xml(format!(
                        "element nesting exceeds the {MAX_DEPTH}-level limit"
                    )));
                }
                saw_element = true;
                stack.push(Element::from_start(&start));
            }
            Ok(Event::Empty(start)) => {
                saw_element = true;
                let element = Element::from_start(&start);
                attach(&mut stack, &mut root, element);
            }
            Ok(Event::End(_)) => {
                if let Some(element) = stack.pop() {
                    attach(&mut stack, &mut root, element);
                }
            }
            Ok(Event::Text(text)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(text.into_inner().as_ref());
                }
            }
            Ok(Event::CData(cdata)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(cdata.into_inner().as_ref());
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(top) = stack.last_mut() {
                    top.text
                        .push_str(&resolve_ref(reference.into_inner().as_ref()));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            // A parse error must not discard data captured so far; stop reading
            // and return whatever tree we have built.
            Err(_) => break,
        }
    }

    // Close any elements left open by malformed markup.
    while let Some(element) = stack.pop() {
        attach(&mut stack, &mut root, element);
    }

    match root {
        Some(element) if saw_element => Ok(element),
        _ => Err(EpubError::Xml("document has no root element".to_string())),
    }
}

fn attach(stack: &mut [Element], root: &mut Option<Element>, element: Element) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(element);
    } else if root.is_none() {
        *root = Some(element);
    }
}

/// Decode the raw document bytes into a `String`, honouring BOMs and a leading
/// encoding declaration.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16_string(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16_string(rest, false);
    }
    // UTF-16 without a BOM: `<\0` or `\0<`.
    if bytes.len() >= 2 {
        if bytes[0] == b'<' && bytes[1] == 0x00 {
            return utf16_string(bytes, true);
        }
        if bytes[0] == 0x00 && bytes[1] == b'<' {
            return utf16_string(bytes, false);
        }
    }
    if let Some(encoding) = declared_encoding(bytes) {
        match encoding.to_ascii_lowercase().as_str() {
            "utf-16" | "utf-16le" => return utf16_string(bytes, true),
            "utf-16be" => return utf16_string(bytes, false),
            "iso-8859-1" | "iso8859-1" | "latin-1" | "latin1" | "windows-1252" | "cp1252" => {
                return bytes.iter().map(|&b| char::from(b)).collect();
            }
            _ => {}
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn utf16_string(bytes: &[u8], little_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let window = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(window);
    let lower = text.to_ascii_lowercase();
    let index = lower.find("encoding")?;
    let after = text[index + "encoding".len()..].trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &after[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Strip characters illegal in XML 1.0, escape stray `&`, and pre-map common
/// HTML named entities so a lenient parse can proceed.
fn sanitize(input: &str) -> String {
    let cleaned: String = input.chars().filter(|&c| is_xml_char(c)).collect();
    let bytes = cleaned.as_bytes();
    let mut out = String::with_capacity(cleaned.len());
    let mut index = 0;
    while index < bytes.len() {
        // CDATA and comments are literal regions: escaping a stray `&` inside
        // them would corrupt the text (and is unnecessary), so copy verbatim.
        if bytes[index] == b'<'
            && let Some(end) = literal_region_end(&cleaned, index)
        {
            out.push_str(&cleaned[index..end]);
            index = end;
            continue;
        }
        if bytes[index] == b'&' {
            if let Some((name, end)) = scan_ref(&cleaned, index) {
                match html_named(&name.to_ascii_lowercase()) {
                    Some(ch) => out.push_str(&format!("&#{};", ch as u32)),
                    None => out.push_str(&cleaned[index..end]),
                }
                index = end;
                continue;
            }
            out.push_str("&amp;");
            index += 1;
            continue;
        }
        let ch = cleaned[index..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// If `index` opens a CDATA section or comment, return the index just past its
/// terminator; otherwise `None`. Unclosed regions fall through to normal
/// handling.
fn literal_region_end(input: &str, index: usize) -> Option<usize> {
    let rest = input.get(index..)?;
    for (open, close) in [("<![CDATA[", "]]>"), ("<!--", "-->")] {
        if let Some(body) = rest.strip_prefix(open) {
            return body
                .find(close)
                .map(|offset| index + open.len() + offset + close.len());
        }
    }
    None
}

/// Decode entities in an already-parsed string (attribute values, refs).
fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'&'
            && let Some((name, end)) = scan_ref(input, index)
        {
            out.push_str(&resolve_ref(name));
            index = end;
            continue;
        }
        let ch = input[index..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// Scan an entity reference starting at the `&` at `amp`; returns the name
/// (e.g. `amp`, `#146`, `#x2014`) and the index just past the `;`.
fn scan_ref(input: &str, amp: usize) -> Option<(&str, usize)> {
    let bytes = input.as_bytes();
    let mut index = amp + 1;
    if index >= bytes.len() {
        return None;
    }
    if bytes[index] == b'#' {
        index += 1;
        let hex = index < bytes.len() && (bytes[index] == b'x' || bytes[index] == b'X');
        if hex {
            index += 1;
        }
        let start = index;
        while index < bytes.len()
            && if hex {
                bytes[index].is_ascii_hexdigit()
            } else {
                bytes[index].is_ascii_digit()
            }
        {
            index += 1;
        }
        if index == start {
            return None;
        }
    } else {
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphanumeric() {
            index += 1;
        }
        if index == start || index - start > 32 {
            return None;
        }
    }
    if bytes.get(index) == Some(&b';') {
        Some((&input[amp + 1..index], index + 1))
    } else {
        None
    }
}

fn resolve_ref(name: &str) -> String {
    if let Some(rest) = name.strip_prefix('#') {
        let code = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()
        } else {
            rest.parse::<u32>().ok()
        };
        return code
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_default();
    }
    match name {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        other => match html_named(&other.to_ascii_lowercase()) {
            Some(ch) => ch.to_string(),
            None => format!("&{other};"),
        },
    }
}

fn html_named(name: &str) -> Option<char> {
    Some(match name {
        "nbsp" => '\u{A0}',
        "copy" => '\u{A9}',
        "reg" => '\u{AE}',
        "deg" => '\u{B0}',
        "middot" => '\u{B7}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "bull" => '\u{2022}',
        "hellip" => '\u{2026}',
        "trade" => '\u{2122}',
        _ => return None,
    })
}

fn is_xml_char(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{A}' | '\u{D}'
            | '\u{20}'..='\u{D7FF}'
            | '\u{E000}'..='\u{FFFD}'
            | '\u{10000}'..='\u{10FFFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_root_and_attributes() {
        let root = parse(br#"<package version="3.0" xmlns:opf="x"><metadata/></package>"#).unwrap();
        assert_eq!(root.local, "package");
        assert_eq!(root.attr("version"), Some("3.0"));
        assert!(root.find("metadata").is_some());
    }

    #[test]
    fn decodes_standard_and_numeric_entities() {
        let root = parse(br#"<a>&amp;&lt;&gt;&#65;&#x42;</a>"#).unwrap();
        assert_eq!(root.text(), "&<>AB");
    }

    #[test]
    fn tolerates_stray_ampersand() {
        let root = parse(b"<p>Tom & Jerry &amp; friends</p>").unwrap();
        assert_eq!(root.text(), "Tom & Jerry & friends");
    }

    #[test]
    fn maps_common_html_entities() {
        let root = parse(br#"<p>a&nbsp;b&mdash;c&rsquo;d&hellip;</p>"#).unwrap();
        assert_eq!(root.text(), "a\u{A0}b\u{2014}c\u{2019}d\u{2026}");
    }

    #[test]
    fn strips_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"<a>x</a>"#);
        let root = parse(&bytes).unwrap();
        assert_eq!(root.text(), "x");
    }

    #[test]
    fn transcodes_utf16le_with_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "<a>héllo</a>".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let root = parse(&bytes).unwrap();
        assert_eq!(root.text(), "héllo");
    }

    #[test]
    fn transcodes_utf16be_with_bom() {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in "<a>héllo</a>".encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        let root = parse(&bytes).unwrap();
        assert_eq!(root.text(), "héllo");
    }

    #[test]
    fn honours_latin1_declaration() {
        let mut bytes = br#"<?xml version="1.0" encoding="ISO-8859-1"?><a>"#.to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(b"</a>");
        let root = parse(&bytes).unwrap();
        assert_eq!(root.text(), "é");
    }

    #[test]
    fn strips_illegal_control_characters() {
        let root = parse(b"<a>be\x01fore\x0cafter</a>").unwrap();
        assert_eq!(root.text(), "beforeafter");
    }

    #[test]
    fn tolerates_mismatched_tags() {
        let root = parse(b"<a><b>hi</a>").unwrap();
        assert_eq!(root.local, "a");
        assert_eq!(root.find("b").unwrap().text(), "hi");
    }

    #[test]
    fn treats_empty_elements_as_children() {
        let root = parse(br#"<m><item id="x"/><item id="y"/></m>"#).unwrap();
        assert_eq!(root.find_all("item").len(), 2);
    }

    #[test]
    fn matches_attributes_by_local_name() {
        let root = parse(br#"<x opf:role="aut" file-as="Morris, S."/>"#).unwrap();
        assert_eq!(root.attr("role"), Some("aut"));
        assert_eq!(root.attr("file-as"), Some("Morris, S."));
    }

    #[test]
    fn rejects_excessively_nested_documents() {
        let depth = 100_000;
        let mut xml = String::with_capacity(depth * 7);
        for _ in 0..depth {
            xml.push_str("<a>");
        }
        for _ in 0..depth {
            xml.push_str("</a>");
        }
        let error = parse(xml.as_bytes()).unwrap_err();
        assert!(matches!(error, EpubError::Xml(_)), "got {error:?}");
    }

    #[test]
    fn accepts_documents_at_the_depth_limit() {
        let mut xml = String::new();
        for _ in 0..MAX_DEPTH {
            xml.push_str("<a>");
        }
        for _ in 0..MAX_DEPTH {
            xml.push_str("</a>");
        }
        assert!(parse(xml.as_bytes()).is_ok());
    }

    #[test]
    fn preserves_ampersand_inside_cdata() {
        let root = parse(b"<p><![CDATA[a & b < c]]></p>").unwrap();
        assert_eq!(root.text(), "a & b < c");
    }

    #[test]
    fn ignores_ampersand_inside_comments() {
        let root = parse(b"<p><!-- a & b -->c</p>").unwrap();
        assert_eq!(root.text(), "c");
    }
}
