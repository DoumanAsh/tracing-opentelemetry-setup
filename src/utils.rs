//! Misc utilities

///Decodes url encoded `value` returning decoded string if `value` is correctly url encoded
fn url_decode(value: &str) -> Option<String> {
    let mut result = String::with_capacity(value.len());
    let mut all_chars = value.chars();

    let mut temp_percent_chars = String::new();
    let mut chars_to_decode = Vec::<u8>::new();
    loop {
        let ch = all_chars.next();

        if let Some('%') = ch {
            temp_percent_chars.push(all_chars.next()?);
            temp_percent_chars.push(all_chars.next()?);
            let char = u8::from_str_radix(&temp_percent_chars, 16).ok()?;
            temp_percent_chars.clear();
            chars_to_decode.push(char);
            continue;
        }

        if !chars_to_decode.is_empty() {
            result.push_str(core::str::from_utf8(&chars_to_decode).ok()?);
            chars_to_decode.clear();
        }

        if let Some(c) = ch {
            result.push(c);
        } else {
            return Some(result);
        }
    }
}

#[inline(always)]
pub(crate) fn extract_otlp_headers(value: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();

    for header in value.trim().split_terminator(',') {
        if let Some((key, value)) = header.split_once('=') {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }

            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            if let Some(value) = url_decode(value) {
                result.push((key.to_owned(), value.to_owned()));
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_url_decode() {
        let test_cases = vec![
            ("v%201", Some("v 1")),
            ("v 1", Some("v 1")),
            ("%C3%B6%C3%A0%C2%A7%C3%96abcd%C3%84", Some("öà§ÖabcdÄ")),
            ("v%XX1", None),
        ];

        for (encoded, expected_decoded) in test_cases {
            assert_eq!(
                url_decode(encoded),
                expected_decoded.map(|value| value.to_owned()),
            )
        }
    }

    #[test]
    fn should_extract_headers() {
        let test_cases = [
            ("k1=v1", [("k1", "v1")].as_slice()),
            ("k1=v1,k2=v2", [("k1", "v1"), ("k2", "v2")].as_slice()),
            ("k1=v1=10,k2,k3", [("k1", "v1=10")].as_slice()),
            ("k1=v1,,,k2,k3=10,Authentication=Basic%20AAA", [("k1", "v1"), ("k3", "10"), ("Authentication", "Basic AAA")].as_slice()),
        ];

        for (input_str, expected_headers) in test_cases {
            let expected_headers: Vec<_> = expected_headers.into_iter().map(|(key, val)| (key.to_string(), val.to_string())).collect();
            assert_eq!(extract_otlp_headers(input_str), expected_headers);
        }
    }
}
