/// `url` with its credentials masked: everything between the scheme and the
/// `@` that ends the user and password (see [`userinfo_end`]), and the value
/// of a query parameter that carries a password or a client key, which the
/// PostgreSQL driver reads as `?password=..` and `?sslkey=..`. Connection
/// errors quote URLs through it.
#[doc(hidden)]
pub fn mask_url_credentials(url: &str) -> String {
    let (scheme, rest) = match url.find("://") {
        Some(index) => url.split_at(index + 3),
        None => ("", url),
    };
    let rest = match userinfo_end(rest) {
        Some(at) => format!("***{}", &rest[at..]),
        None => rest.to_string(),
    };
    let (base, query) = match rest.split_once('?') {
        Some((base, query)) => (format!("{scheme}{base}"), Some(query)),
        None => (format!("{scheme}{rest}"), None),
    };
    let Some(query) = query else {
        return base;
    };
    let query: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, _)) if carries_secret(key) => format!("{key}=***"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{base}?{}", query.join("&"))
}

/// Where the user and password of `rest`, a URL after its `://`, end: at the
/// last `@` of the authority, or, when a password holding an unencoded `/`,
/// `?` or `#` ended the authority early, at the last `@` a host name follows
/// once a `:` has begun a password. Masking a little more than the
/// credentials is better than printing part of them.
fn userinfo_end(rest: &str) -> Option<usize> {
    let authority_end = |text: &str| text.find(['/', '?', '#']).unwrap_or(text.len());
    if let Some(at) = rest[..authority_end(rest)].rfind('@') {
        return Some(at);
    }
    let host_follows = |text: &str| {
        text[..authority_end(text)].chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '-' | '.' | '_' | ':' | '[' | ']' | '%')
        })
    };
    rest.rmatch_indices('@')
        .map(|(at, _)| at)
        .find(|&at| rest[..at].contains(':') && host_follows(&rest[at + 1..]))
}

/// Whether the query parameter `key` names a secret. The key is compared as
/// the driver reads it, percent-decoded: `pass%77ord` is a password too.
fn carries_secret(key: &str) -> bool {
    let bytes = key.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%')
            .then(|| key.get(at + 1..at + 3))
            .flatten()
            .filter(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                at += 3;
            }
            None => {
                decoded.push(if bytes[at] == b'+' { b' ' } else { bytes[at] });
                at += 1;
            }
        }
    }
    let key = String::from_utf8_lossy(&decoded).to_ascii_lowercase();
    matches!(
        key.as_str(),
        "password" | "sslpassword" | "passwd" | "pwd" | "sslkey" | "ssl-key"
    )
}

#[cfg(test)]
#[path = "../../tests/unit/masking_tests.rs"]
mod tests;
