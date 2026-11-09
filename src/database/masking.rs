/// `url` with its credentials masked: everything between the scheme and the
/// `@` that ends the user and password (see [`userinfo_end`]), and the value
/// of a query parameter that carries a password, which the PostgreSQL driver
/// reads as `?password=..`. Connection errors quote URLs through it.
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
            Some((key, _))
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "password" | "sslpassword" | "passwd" | "pwd"
                ) =>
            {
                format!("{key}=***")
            }
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
