//! Query string parsing with JavaScript `URLSearchParams` semantics:
//! `+` decodes to space, `%XX` percent-decoding, empty `&`-segments skipped,
//! duplicate keys preserved in order, and invalid encodings are errors.

/// Decode one key/value segment the way JS `URLSearchParams` does.
fn decode_segment(seg: &str) -> Result<String, ()> {
    let bytes = seg.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(());
                }
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push(h * 16 + l);
                        i += 3;
                    }
                    _ => return Err(()),
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse a raw query string into ordered `(key, value)` pairs.
/// Returns `Err(())` on invalid percent-encoding (JS throws there too).
pub fn parse_query(raw: &str) -> Result<Vec<(String, String)>, ()> {
    let mut pairs = Vec::new();
    for segment in raw.split('&') {
        // JS URLSearchParams skips empty segments ("a=1&&b=2" => 2 entries).
        if segment.is_empty() {
            continue;
        }
        let (key, value) = match segment.find('=') {
            Some(idx) => (&segment[..idx], &segment[idx + 1..]),
            None => (segment, ""),
        };
        pairs.push((decode_segment(key)?, decode_segment(value)?));
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_parsing() {
        let pairs = parse_query("v=abc&c=def&x=url").unwrap();
        assert_eq!(
            pairs,
            vec![
                ("v".into(), "abc".into()),
                ("c".into(), "def".into()),
                ("x".into(), "url".into())
            ]
        );
    }

    #[test]
    fn plus_is_space() {
        let pairs = parse_query("q=a+b+c").unwrap();
        assert_eq!(pairs, vec![("q".into(), "a b c".into())]);
    }

    #[test]
    fn percent_decoding() {
        let pairs = parse_query("x=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1%26c%3D2").unwrap();
        assert_eq!(
            pairs,
            vec![("x".into(), "https://example.com/a?b=1&c=2".into())]
        );
    }

    #[test]
    fn empty_segments_skipped() {
        let pairs = parse_query("a=1&&b=2&").unwrap();
        assert_eq!(
            pairs,
            vec![("a".into(), "1".into()), ("b".into(), "2".into())]
        );
    }

    #[test]
    fn bare_key_and_embedded_equals() {
        let pairs = parse_query("v&x=a=b").unwrap();
        assert_eq!(
            pairs,
            vec![("v".into(), "".into()), ("x".into(), "a=b".into())]
        );
    }

    #[test]
    fn duplicates_preserved_in_order() {
        let pairs = parse_query("v=1&v=2").unwrap();
        assert_eq!(
            pairs,
            vec![("v".into(), "1".into()), ("v".into(), "2".into())]
        );
    }

    #[test]
    fn invalid_encoding_is_error() {
        assert!(parse_query("v=%zz").is_err());
        assert!(parse_query("v=%2").is_err());
    }
}
