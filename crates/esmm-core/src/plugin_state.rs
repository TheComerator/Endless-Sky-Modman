//! The game's `<config>/plugins.txt`: which installed plugins are enabled.
//!
//! The game writes each state as `1`/`0` and reads it back as a number, so
//! writing `true`/`false` would make the game treat every plugin as disabled.
//! A plugin missing from the file defaults to enabled.

use std::collections::BTreeMap;

use crate::datanode::{self, Node};

/// Plugin identity -> enabled. Sorted by name, matching the order the game writes.
pub type PluginStates = BTreeMap<String, bool>;

pub fn parse(text: &str) -> PluginStates {
    let mut states = PluginStates::new();
    for node in datanode::parse(text).iter().filter(|n| n.key() == "state") {
        for child in node.children.iter().filter(|c| c.tokens.len() == 2) {
            states.insert(
                child.tokens[0].clone(),
                game_number(&child.tokens[1]) != 0.0,
            );
        }
    }
    states
}

pub fn write(states: &PluginStates) -> String {
    let children = states
        .iter()
        .map(|(name, &on)| Node::new([name.as_str(), if on { "1" } else { "0" }]))
        .collect();
    datanode::write(&[Node::new(["state"]).with_children(children)])
}

/// Mirrors `DataNode::Value`: anything that isn't a plain number reads as 0.
fn game_number(token: &str) -> f64 {
    let numeric_chars = token
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'));
    if numeric_chars {
        token.parse().unwrap_or(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_numbers_not_booleans() {
        let states = PluginStates::from([
            ("Mega Freight".into(), true),
            ("ship.merging".into(), false),
        ]);
        assert_eq!(
            write(&states),
            "state\n\t\"Mega Freight\" 1\n\tship.merging 0\n"
        );
    }

    #[test]
    fn round_trips() {
        let states = PluginStates::from([
            ("Jimmy's Ship Emporium".into(), true),
            ("a b".into(), false),
        ]);
        assert_eq!(parse(&write(&states)), states);
    }

    #[test]
    fn reads_like_the_game() {
        let states = parse("state\n\tA 1\n\tB 0\n\tC true\n\tD 2\n\tE\n");
        assert_eq!(states.get("A"), Some(&true));
        assert_eq!(states.get("B"), Some(&false));
        assert_eq!(
            states.get("C"),
            Some(&false),
            "non-numeric reads as 0 in the game"
        );
        assert_eq!(states.get("D"), Some(&true));
        assert_eq!(
            states.get("E"),
            None,
            "entries without exactly two tokens are ignored"
        );
    }

    #[test]
    fn later_state_blocks_override_earlier() {
        assert_eq!(parse("state\n\tA 1\nstate\n\tA 0\n").get("A"), Some(&false));
    }
}
