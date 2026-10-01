//! Endless Sky's DataNode text format (`plugin.txt`, `plugins.txt`, game data).
//!
//! Mirrors the game's own `DataFile::LoadData` tokenizer and `DataWriter` quoting
//! so files we write are read back by the game exactly as intended.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    pub tokens: Vec<String>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new<I, S>(tokens: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            tokens: tokens.into_iter().map(Into::into).collect(),
            children: Vec::new(),
        }
    }

    pub fn with_children(mut self, children: Vec<Node>) -> Self {
        self.children = children;
        self
    }

    pub fn token(&self, index: usize) -> Option<&str> {
        self.tokens.get(index).map(String::as_str)
    }

    pub fn key(&self) -> &str {
        self.token(0).unwrap_or("")
    }
}

/// The game treats every code point at or below ASCII space (except newline) as a separator.
fn is_separator(c: char) -> bool {
    c <= ' ' && c != '\n'
}

/// Parses DataNode text into its top-level nodes. Never fails: like the game,
/// malformed input (e.g. a missing closing quote) still yields best-effort tokens.
pub fn parse(text: &str) -> Vec<Node> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    // Each entry is (indentation depth, node being built). The root sits at -1 so
    // it is never popped; a node is attached to its parent when popped.
    let mut stack: Vec<(i64, Node)> = vec![(-1, Node::default())];

    for line in text.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        let depth = chars.iter().take_while(|&&c| is_separator(c)).count();
        let rest = &chars[depth..];
        if rest.is_empty() || rest[0] == '#' {
            continue;
        }

        let depth = depth as i64;
        while stack.last().is_some_and(|(d, _)| *d >= depth) {
            let (_, done) = stack.pop().expect("root is never popped");
            stack
                .last_mut()
                .expect("root is never popped")
                .1
                .children
                .push(done);
        }
        stack.push((
            depth,
            Node {
                tokens: tokenize(rest),
                children: Vec::new(),
            },
        ));
    }

    while stack.len() > 1 {
        let (_, done) = stack.pop().expect("checked length");
        stack
            .last_mut()
            .expect("checked length")
            .1
            .children
            .push(done);
    }
    stack
        .pop()
        .map(|(_, root)| root.children)
        .unwrap_or_default()
}

fn tokenize(chars: &[char]) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let quote = chars[i];
        if quote == '"' || quote == '`' {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != quote {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect());
            if i < chars.len() {
                i += 1;
            }
        } else {
            let start = i;
            while i < chars.len() && chars[i] > ' ' {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect());
        }

        while i < chars.len() && chars[i] <= ' ' {
            i += 1;
        }
        // A '#' between tokens starts a comment; inside an unquoted token it does not.
        if i < chars.len() && chars[i] == '#' {
            break;
        }
    }
    tokens
}

/// Quotes a token the way the game's `DataWriter::Quote` does.
pub fn quote(token: &str) -> String {
    // Matches C `isspace` in the "C" locale.
    let has_space = token.is_empty()
        || token
            .chars()
            .any(|c| matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'));
    if token.contains('"') {
        format!("`{token}`")
    } else if has_space || token.contains('`') {
        format!("\"{token}\"")
    } else {
        token.to_string()
    }
}

/// Serializes nodes with tab indentation, as the game's `DataWriter` does.
pub fn write(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        write_node(&mut out, node, 0);
    }
    out
}

fn write_node(out: &mut String, node: &Node, depth: usize) {
    out.push_str(&"\t".repeat(depth));
    let line: Vec<String> = node.tokens.iter().map(|t| quote(t)).collect();
    out.push_str(&line.join(" "));
    out.push('\n');
    for child in &node.children {
        write_node(out, child, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nesting_follows_indentation() {
        let nodes = parse("a\n\tb\n\t\tc\n\td\ne\n");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].children.len(), 2);
        assert_eq!(nodes[0].children[0].children[0].key(), "c");
        assert_eq!(nodes[0].children[1].key(), "d");
        assert_eq!(nodes[1].key(), "e");
    }

    #[test]
    fn spaces_work_as_indentation() {
        let nodes = parse("a\n  b\n    c\n");
        assert_eq!(nodes[0].children[0].children[0].key(), "c");
    }

    #[test]
    fn both_quote_styles_and_comments() {
        let nodes = parse("name `Say \"hi\"` \"two words\" bare # trailing comment\n# full line\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].tokens,
            vec!["name", "Say \"hi\"", "two words", "bare"]
        );
    }

    #[test]
    fn hash_inside_unquoted_token_is_kept() {
        assert_eq!(parse("a#b c\n")[0].tokens, vec!["a#b", "c"]);
    }

    #[test]
    fn crlf_bom_and_missing_trailing_newline() {
        let nodes = parse("\u{feff}a 1\r\n\tb \"x y\"\r\nc");
        assert_eq!(nodes[0].tokens, vec!["a", "1"]);
        assert_eq!(nodes[0].children[0].tokens, vec!["b", "x y"]);
        assert_eq!(nodes[1].key(), "c");
    }

    #[test]
    fn unterminated_quote_runs_to_end_of_line() {
        assert_eq!(parse("a \"open\n")[0].tokens, vec!["a", "open"]);
    }

    #[test]
    fn empty_quoted_token() {
        assert_eq!(parse("a \"\" b\n")[0].tokens, vec!["a", "", "b"]);
    }

    #[test]
    fn quoting_matches_game() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("two words"), "\"two words\"");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("Jimmy's"), "Jimmy's");
        assert_eq!(quote("say \"x\""), "`say \"x\"`");
        assert_eq!(quote("tick`"), "\"tick`\"");
    }

    #[test]
    fn write_then_parse_round_trips() {
        let nodes = vec![
            Node::new(["state"]).with_children(vec![
                Node::new(["Mega Freight", "1"]),
                Node::new(["Say \"hi\"", "0"]),
                Node::new(["", "1"]),
            ]),
            Node::new(["other"]),
        ];
        assert_eq!(parse(&write(&nodes)), nodes);
    }
}
