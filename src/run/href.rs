use std::sync::OnceLock;

use regex::Regex;

pub(crate) fn href_regex() -> &'static Regex {
    static HREF_REGEX: OnceLock<Regex> = OnceLock::new();
    HREF_REGEX.get_or_init(|| Regex::new(r#"href="([^"]+/([^"]+))""#).unwrap())
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct Href(pub String);

impl AsRef<str> for Href {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Href {
    pub(crate) fn from_document(html: &str, base: &str) -> Vec<Href> {
        let r = href_regex()
            .captures_iter(html)
            .map(|c| {
                let path = c.get(1).unwrap().as_str().to_string();
                let last = c.get(2).unwrap().as_str();
                if last.contains(['.', '?']) || !path.starts_with(base) {
                    None
                } else {
                    Some(Href(path))
                }
            })
            .flatten()
            .collect();
        r
    }
}
