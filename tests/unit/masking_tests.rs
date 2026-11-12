use super::mask_url_credentials;

#[test]
fn a_secret_query_parameter_is_masked_however_its_key_is_spelled() {
    for key in [
        "password",
        "pass%77ord",
        "%70%61%73%73%77%6F%72%64",
        "PassWord",
        "sslkey",
        "ssl%2Dkey",
    ] {
        let shown =
            mask_url_credentials(&format!("postgres://db/app?{key}=s3cret&sslmode=require"));
        assert_eq!(
            shown,
            format!("postgres://db/app?{key}=***&sslmode=require")
        );
    }
    // other keys, values that look like a key, and broken escapes stay
    for query in [
        "application_name=pass%77ord",
        "pass%7=x",
        "pass%=x",
        "p%é=x",
        "%+1=x",
    ] {
        let url = format!("postgres://db/app?{query}");
        assert_eq!(mask_url_credentials(&url), url);
    }
}
