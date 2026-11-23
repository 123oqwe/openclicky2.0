//! Minimal HTML to text: no external parser, deterministic, capped.

pub fn to_text(html: &str, limit: usize) -> String {
    let mut out = String::with_capacity(html.len().min(limit));
    let mut in_tag = false;
    let mut in_script = false;
    let mut tag_buf = String::new();
    let mut last_space = false;

    for c in html.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let tag = tag_buf.to_ascii_lowercase();
                in_script = tag.starts_with("script") || tag.starts_with("style");
                tag_buf.clear();
            } else {
                tag_buf.push(c);
            }
            continue;
        }
        if c == '<' {
            in_tag = true;
            continue;
        }
        if in_script {
            continue;
        }
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
        if out.chars().count() >= limit {
            break;
        }
    }

    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    decoded.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_scripts() {
        let html = "<html><head><style>p{}</style></head><body><h1>Hi</h1><script>evil()</script><p>there &amp; back</p></body></html>";
        let text = to_text(html, 1000);
        assert!(text.contains("Hi"));
        assert!(text.contains("there & back"));
        assert!(!text.contains("evil"));
        assert!(!text.contains('<'));
    }

    #[test]
    fn collapses_whitespace_and_caps() {
        let text = to_text("<p>a       b</p>", 3);
        assert_eq!(text, "a b");
    }
}
