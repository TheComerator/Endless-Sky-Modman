//! A plugin's own `plugin.txt`, parsed the way the game's `PluginManager::Load` does.

use std::collections::BTreeSet;

use crate::datanode::{self, Node};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dependencies {
    pub game_version: Option<String>,
    pub requires: BTreeSet<String>,
    pub optional: BTreeSet<String>,
    pub conflicts: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginMeta {
    /// The `name` field. When absent the game identifies the plugin by its folder name.
    pub name: Option<String>,
    /// All `about` lines joined with newlines, as the game builds it.
    pub about: String,
    pub version: Option<String>,
    pub authors: BTreeSet<String>,
    pub tags: BTreeSet<String>,
    pub dependencies: Dependencies,
}

impl PluginMeta {
    pub fn parse(text: &str) -> Self {
        let mut meta = Self::default();
        for node in datanode::parse(text) {
            let value = node.token(1).map(str::to_string);
            match (node.key(), value) {
                ("name", Some(v)) => meta.name = Some(v),
                ("about", Some(v)) => {
                    meta.about.push_str(&v);
                    meta.about.push('\n');
                }
                ("version", Some(v)) => meta.version = Some(v),
                ("authors", _) => meta.authors.extend(first_tokens(&node.children)),
                ("tags", _) => meta.tags.extend(first_tokens(&node.children)),
                ("dependencies", _) => meta.dependencies = parse_dependencies(&node.children),
                _ => {}
            }
        }
        meta
    }

    /// The name the game uses for this plugin in `plugins.txt` and in other plugins' dependencies.
    pub fn identity<'a>(&'a self, folder_name: &'a str) -> &'a str {
        self.name.as_deref().unwrap_or(folder_name)
    }
}

fn first_tokens(nodes: &[Node]) -> impl Iterator<Item = String> + '_ {
    nodes.iter().filter_map(|n| n.token(0).map(str::to_string))
}

fn parse_dependencies(nodes: &[Node]) -> Dependencies {
    let mut deps = Dependencies::default();
    for node in nodes {
        match node.key() {
            "game version" => deps.game_version = node.token(1).map(str::to_string),
            "requires" => deps.requires.extend(first_tokens(&node.children)),
            "optional" => deps.optional.extend(first_tokens(&node.children)),
            "conflicts" => deps.conflicts.extend(first_tokens(&node.children)),
            _ => {}
        }
    }
    deps
}
