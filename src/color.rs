//! Hex colour parsing. No Win32, no I/O.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Accepts `#RRGGBB` and `RRGGBB`, case-insensitive.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let t = s.strip_prefix('#').unwrap_or(s);
    if t.len() != 6 || !t.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(Rgb {
        r: u8::from_str_radix(&t[0..2], 16).ok()?,
        g: u8::from_str_radix(&t[2..4], 16).ok()?,
        b: u8::from_str_radix(&t[4..6], 16).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_hash() {
        assert_eq!(parse_hex("#FFFFFF"), Some(Rgb { r: 255, g: 255, b: 255 }));
    }

    #[test]
    fn parses_without_hash() {
        assert_eq!(parse_hex("1A2B3C"), Some(Rgb { r: 0x1A, g: 0x2B, b: 0x3C }));
    }

    #[test]
    fn is_case_insensitive() {
        assert_eq!(parse_hex("#abcdef"), parse_hex("#ABCDEF"));
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(parse_hex("#FFF"), None);
        assert_eq!(parse_hex("#FFFFFFFF"), None);
    }

    #[test]
    fn rejects_non_hex() {
        assert_eq!(parse_hex("#GGGGGG"), None);
        assert_eq!(parse_hex(""), None);
    }
}
