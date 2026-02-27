use regex::Regex;

pub fn scrub(input: &str) -> String {
    let mut out = input.to_string();

    let aws_key = Regex::new(r"AKIA[0-9A-Z]{16}").unwrap();
    out = aws_key.replace_all(&out, "[REDACTED_AWS_KEY]").to_string();

    let private_key =
        Regex::new(r"-----BEGIN [A-Z ]+ PRIVATE KEY-----[\s\S]+?-----END [A-Z ]+ PRIVATE KEY-----")
            .unwrap();
    out = private_key
        .replace_all(&out, "[REDACTED_PRIVATE_KEY]")
        .to_string();

    let conn_string = Regex::new(r"://([^:@/\s]+):([^@/\s]+)@").unwrap();
    out = conn_string
        .replace_all(&out, "://$1:[REDACTED]@")
        .to_string();

    let generic =
        Regex::new(r"(?i)\b(key|token|secret|password)\b\s*[:=]\s*([A-Za-z0-9+/=_\-]{8,})")
            .unwrap();
    out = generic.replace_all(&out, "$1=[REDACTED_TOKEN]").to_string();

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_aws_key() {
        let input = "access_key=AKIAIOSFODNN7EXAMPLE";
        let out = scrub(input);
        assert!(out.contains("[REDACTED_AWS_KEY]"));
    }

    #[test]
    fn scrub_private_key_block() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nabc\n-----END RSA PRIVATE KEY-----";
        let out = scrub(input);
        assert_eq!(out, "[REDACTED_PRIVATE_KEY]");
    }

    #[test]
    fn scrub_connection_string_password() {
        let input = "postgres://user:pass123@localhost:5432/db";
        let out = scrub(input);
        assert!(out.contains("user:[REDACTED]@"));
    }

    #[test]
    fn scrub_generic_tokens() {
        let input = "token=abcdEFGH1234 secret: supersecret";
        let out = scrub(input);
        assert!(out.contains("token=[REDACTED_TOKEN]"));
        assert!(out.contains("secret=[REDACTED_TOKEN]"));
    }

    #[test]
    fn scrub_preserves_normal_text() {
        let input = "hello world";
        let out = scrub(input);
        assert_eq!(out, input);
    }
}
