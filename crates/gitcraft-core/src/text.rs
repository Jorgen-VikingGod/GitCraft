//! Small text helpers shared by the parsers, and credential masking.

/// Lines separated by `\n` only. Subjects and messages can contain form feeds
/// and other separators, which must stay inside their line.
pub fn lines(text: &str) -> impl Iterator<Item = &str> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n')
        .filter(move |_| !text.is_empty())
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// Replaces the password in `scheme://user:password@host` URLs with `***`.
pub fn mask_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("://") {
        let (before, after) = rest.split_at(start + 3);
        out.push_str(before);
        let end = after
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '\'' | '"' | ')' | '>'))
            .unwrap_or(after.len());
        let authority = &after[..end];
        match authority.rfind('@') {
            Some(at) => {
                let userinfo = &authority[..at];
                match userinfo.split_once(':') {
                    Some((user, _)) => out.push_str(&format!("{user}:***")),
                    // A lone token as the user name is a credential too, except for ssh's `git@`.
                    None if userinfo.len() >= 20 => out.push_str("***"),
                    None => out.push_str(userinfo),
                }
                out.push_str(&authority[at..]);
            }
            None => out.push_str(authority),
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_keep_other_separators_inside_the_line() {
        assert_eq!(
            lines("a\x0cb\nc\x1cd\n").collect::<Vec<_>>(),
            ["a\x0cb", "c\x1cd"]
        );
        assert_eq!(lines("one\r\ntwo").collect::<Vec<_>>(), ["one", "two"]);
        assert_eq!(lines("").count(), 0);
    }

    #[test]
    fn masks_passwords_and_long_tokens_only() {
        assert_eq!(
            mask_urls("https://user:s3cret@host/x.git"),
            "https://user:***@host/x.git"
        );
        assert_eq!(
            mask_urls("fatal: 'https://ghp_abcdefghijklmnopqrstuvwxyz@host/x' failed"),
            "fatal: 'https://***@host/x' failed"
        );
        assert_eq!(mask_urls("ssh://git@host/x.git"), "ssh://git@host/x.git");
        assert_eq!(
            mask_urls("https://host/x.git and git@host:x"),
            "https://host/x.git and git@host:x"
        );
        assert_eq!(mask_urls("no urls here"), "no urls here");
    }
}
