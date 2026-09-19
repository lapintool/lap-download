use reqwest::header::{CONTENT_DISPOSITION, HeaderMap};
use url::Url;

pub fn filename_from_headers(headers: &HeaderMap, fallback_url: &str) -> String {
    if let Some(value) = headers.get(CONTENT_DISPOSITION).and_then(|v| v.to_str().ok()) {
        if let Some(name) = parse_content_disposition(value) {
            return sanitize_filename(&name);
        }
    }
    filename_from_url(fallback_url)
}

fn parse_content_disposition(value: &str) -> Option<String> {
    for part in value.split(';') {
        let part = part.trim();
        if let Some(rest) = part
            .strip_prefix("filename*")
            .or_else(|| part.strip_prefix("FILENAME*"))
        {
            let rest = rest.trim().trim_start_matches('=');
            if let Some(encoded) = rest.split("''").nth(1) {
                let decoded = percent_decode(encoded.trim_matches('"'));
                if !decoded.is_empty() {
                    return Some(decoded);
                }
            }
        }
    }
    for part in value.split(';') {
        let part = part.trim();
        if let Some(rest) = part
            .strip_prefix("filename")
            .or_else(|| part.strip_prefix("FILENAME"))
        {
            if rest.starts_with('*') {
                continue;
            }
            let name = rest
                .trim()
                .trim_start_matches('=')
                .trim()
                .trim_matches('"')
                .to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

pub fn filename_from_url(raw: &str) -> String {
    let Ok(parsed) = Url::parse(raw) else {
        return "download.bin".into();
    };
    let path = parsed.path();
    let name = path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("download.bin");
    let cleaned = sanitize_filename(&percent_decode(name));
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "download.bin".into()
    } else {
        cleaned
    }
}

pub fn sanitize_filename(name: &str) -> String {
    let trimmed = name.trim().trim_matches(['"', '\'']);
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\0')
            || ch.is_control()
        {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out.trim().trim_end_matches('.').to_string()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
