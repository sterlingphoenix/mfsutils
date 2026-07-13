//! Shared helpers: CRC-16, MacRoman decoding, Mac dates, glob matching.

/// CRC-16/XMODEM (poly 0x1021, MSB-first). Used by both MacBinary II
/// (init 0) and BinHex 4.0 (init 0).
pub fn crc16(data: &[u8], init: u16) -> u16 {
    let mut crc = init;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// MacRoman 0x80..=0xFF to Unicode.
const MACROMAN_HIGH: [char; 128] = [
    'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', //
    'ê', 'ë', 'í', 'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', //
    '†', '°', '¢', '£', '§', '•', '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', //
    '∞', '±', '≤', '≥', '¥', 'µ', '∂', '∑', '∏', 'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', //
    '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«', '»', '…', '\u{00A0}', 'À', 'Ã', 'Õ', 'Œ', 'œ', //
    '–', '—', '“', '”', '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄', '€', '‹', '›', 'ﬁ', 'ﬂ', //
    '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì', 'Ó', 'Ô', //
    '\u{F8FF}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛', 'ˇ',
];

pub fn decode_macroman(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0x80 {
                b as char
            } else {
                MACROMAN_HIGH[(b - 0x80) as usize]
            }
        })
        .collect()
}

/// Render a 4-byte type/creator code, with unprintable bytes as spaces.
pub fn fourcc(code: &[u8; 4]) -> String {
    code.iter()
        .map(|&b| {
            if b < 0x20 || b == 0x7F {
                ' '
            } else if b < 0x80 {
                b as char
            } else {
                MACROMAN_HIGH[(b - 0x80) as usize]
            }
        })
        .collect()
}

/// Seconds between 1904-01-01 and 1970-01-01 (Mac vs. Unix epoch).
pub const MAC_EPOCH_OFFSET: i64 = 2_082_844_800;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// Civil date from days since Unix epoch (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn parts(mac_secs: u32) -> (i64, u32, u32, i64, i64, i64, usize) {
    let unix = mac_secs as i64 - MAC_EPOCH_OFFSET;
    let days = unix.div_euclid(86_400);
    let rem = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let wd = days.rem_euclid(7) as usize; // Unix day 0 was a Thursday
    (y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60, wd)
}

/// "Thu May 17 11:16:45 1984" (hmount style).
pub fn mac_date_long(mac_secs: u32) -> String {
    if mac_secs == 0 {
        return "(never)".into();
    }
    let (y, m, d, hh, mm, ss, wd) = parts(mac_secs);
    format!(
        "{} {} {:2} {:02}:{:02}:{:02} {}",
        WEEKDAYS[wd],
        MONTHS[(m - 1) as usize],
        d,
        hh,
        mm,
        ss,
        y
    )
}

/// "May 17 1984 11:16" (mfsls -l style, fixed width).
pub fn mac_date_short(mac_secs: u32) -> String {
    if mac_secs == 0 {
        return "                 ".into();
    }
    let (y, m, d, hh, mm, _, _) = parts(mac_secs);
    format!("{} {:2} {:4} {:02}:{:02}", MONTHS[(m - 1) as usize], d, y, hh, mm)
}

/// Case-insensitive glob match supporting `*` and `?` (MFS names are
/// case-insensitive).
pub fn glob_match_ci(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let s: Vec<char> = name.to_lowercase().chars().collect();
    glob_chars(&p, &s)
}

fn glob_chars(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

pub fn eq_ci(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

pub fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.to_lowercase().starts_with(&prefix.to_lowercase())
}

/// Make an MFS filename safe as a Unix filename: '/' and control
/// characters are percent-encoded (hfsutils convention).
pub fn sanitize_unix_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c == '/' || (c as u32) < 0x20 || c as u32 == 0x7F || c == '%' {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{b:02x}"));
            }
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
    fn crc16_xmodem_vector() {
        assert_eq!(crc16(b"123456789", 0), 0x31C3);
    }

    #[test]
    fn macroman_decode() {
        assert_eq!(decode_macroman(b"caf\x8e"), "café");
    }

    #[test]
    fn date_formatting() {
        // 1984-05-17 11:16:45 relative to Mac epoch.
        // Known from `file` output for the test image: 2536485405.
        assert_eq!(mac_date_long(2536485405), "Thu May 17 11:16:45 1984");
    }

    #[test]
    fn globbing() {
        assert!(glob_match_ci("*.txt", "Hello.TXT"));
        assert!(glob_match_ci("sys?em", "System"));
        assert!(glob_match_ci("*", "anything"));
        assert!(!glob_match_ci("a*b", "acd"));
    }
}
