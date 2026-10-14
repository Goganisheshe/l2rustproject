//! Minimal reader for L2 NPC dialog HTML: paragraphs of colored text and
//! bypass links / buttons. Tables are flattened, images and inputs dropped.

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    /// `LEVEL` (quest yellow) or an `RRGGBB` / `BBGGRR`-ish hex as written.
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text(Vec<Span>),
    Link { label: String, bypass: String },
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // Must be a whole attribute name.
        if at > 0 && !lower.as_bytes()[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[at + name.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        return Some(match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or("").to_string(),
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string(),
        });
    }
    None
}

fn decode(s: &str) -> String {
    s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

/// Strips `bypass` and `-h` from a link action.
fn bypass_of(action: &str) -> Option<String> {
    let a = action.trim();
    let a = a.strip_prefix("bypass").or_else(|| a.strip_prefix("Bypass"))?.trim_start();
    let a = a.strip_prefix("-h").unwrap_or(a).trim_start();
    Some(a.to_string())
}

struct Builder {
    blocks: Vec<Block>,
    spans: Vec<Span>,
    colors: Vec<Option<String>>,
}

impl Builder {
    fn text(&mut self, raw: &str) {
        let t = decode(&raw.split_whitespace().collect::<Vec<_>>().join(" "));
        let lead = raw.starts_with(char::is_whitespace);
        let trail = raw.ends_with(char::is_whitespace);
        if t.is_empty() {
            if (lead || trail) && self.spans.last().is_some_and(|s| !s.text.ends_with([' ', '\n'])) {
                self.push(" ");
            }
            return;
        }
        let mut s = String::new();
        if lead && self.spans.last().is_some_and(|s| !s.text.ends_with([' ', '\n'])) {
            s.push(' ');
        }
        s.push_str(&t);
        if trail {
            s.push(' ');
        }
        self.push(&s);
    }
    fn push(&mut self, s: &str) {
        let color = self.colors.last().cloned().flatten();
        match self.spans.last_mut() {
            Some(last) if last.color == color => last.text.push_str(s),
            _ => self.spans.push(Span { text: s.to_string(), color }),
        }
    }
    fn newline(&mut self) {
        if !self.spans.is_empty() {
            self.push("\n");
        }
    }
    fn flush(&mut self) {
        while let Some(last) = self.spans.last_mut() {
            let trimmed = last.text.trim_end().to_string();
            if trimmed.is_empty() {
                self.spans.pop();
            } else {
                last.text = trimmed;
                break;
            }
        }
        if !self.spans.is_empty() {
            self.blocks.push(Block::Text(std::mem::take(&mut self.spans)));
        }
    }
}

/// Parses a dialog page into blocks.
pub fn parse(src: &str) -> Vec<Block> {
    let mut b = Builder { blocks: Vec::new(), spans: Vec::new(), colors: Vec::new() };
    // Pending <a>/<Button> link: (bypass, label so far).
    let mut link: Option<(String, String)> = None;
    let mut rest = src;
    let mut skip_title = false;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("<!--") {
            rest = stripped.split_once("-->").map_or("", |(_, r)| r);
            continue;
        }
        let Some(lt) = rest.find('<') else {
            match &mut link {
                Some((_, label)) => label.push_str(rest),
                None => b.text(rest),
            }
            break;
        };
        let (text, after) = rest.split_at(lt);
        if !skip_title {
            match &mut link {
                Some((_, label)) => label.push_str(text),
                None => b.text(text),
            }
        }
        let Some(gt) = after.find('>') else { break };
        let tag = &after[1..gt];
        rest = &after[gt + 1..];
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace()).next().unwrap_or("").to_ascii_lowercase();
        let closing = tag.starts_with('/');
        match (name.as_str(), closing) {
            ("title", c) => skip_title = !c,
            ("br" | "br1" | "p", _) | ("tr", true) | ("center", _) => b.newline(),
            ("td", true) => b.push(" "),
            ("font" | "fond", false) => {
                let c = attr(tag, "color");
                b.colors.push(c);
            }
            ("font" | "fond", true) => {
                b.colors.pop();
            }
            ("a" | "button", false) => {
                let Some(bypass) = attr(tag, "action").and_then(|a| bypass_of(&a)) else { continue };
                if let Some(value) = attr(tag, "value") {
                    // <button value="Label" action="..."> has no body.
                    b.flush();
                    b.blocks.push(Block::Link { label: decode(value.trim()), bypass });
                } else {
                    link = Some((bypass, String::new()));
                }
            }
            ("a" | "button", true) => {
                if let Some((bypass, label)) = link.take() {
                    b.flush();
                    let label = decode(&label.split_whitespace().collect::<Vec<_>>().join(" "));
                    let label = strip_tags(&label);
                    b.blocks.push(Block::Link { label, bypass });
                }
            }
            _ => {}
        }
        if let Some((_, label)) = &mut link {
            // Tags inside a link label (fonts) are dropped from the label.
            let _ = label;
        }
    }
    b.flush();
    b.blocks
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_and_text() {
        let b = parse(
            r#"<html><body>Guard Bathis:<br>Know <font color=LEVEL>auto-hunting</font>...<br>
<Button ALIGN=LEFT ICON="NORMAL" action="bypass npc_%objectId%_Chat 1"><font color=LEVEL>Ask</font></Button>
<button value="Go" action="bypass -h Script Q1 go" width=80>
</body></html>"#,
        );
        assert_eq!(b.len(), 3);
        let Block::Text(spans) = &b[0] else { panic!() };
        assert_eq!(spans[1].color.as_deref(), Some("LEVEL"));
        assert_eq!(b[1], Block::Link { label: "Ask".into(), bypass: "npc_%objectId%_Chat 1".into() });
        assert_eq!(b[2], Block::Link { label: "Go".into(), bypass: "Script Q1 go".into() });
    }
}
