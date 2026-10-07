//! Read-only: indexes every plugin folder in a directory and prints what overlaps.
//! Usage: cargo run -p esmm-core --example overlap_scan --release -- <plugins dir>
//! Nothing is written to the plugins folder; no cache is used.

use std::path::PathBuf;
use std::time::Instant;

use esmm_core::overlap::{OverlapItem, PluginEntry, find_overlaps, index_plugin};

fn main() {
    let dir: PathBuf = std::env::args_os()
        .nth(1)
        .expect("usage: overlap_scan <plugins dir>")
        .into();
    let started = Instant::now();
    let mut indexes = Vec::new();
    let mut folders: Vec<String> = std::fs::read_dir(&dir)
        .expect("can't read the plugins dir")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    folders.sort();
    for folder in &folders {
        match index_plugin(&dir.join(folder)) {
            Ok(index) => indexes.push((folder.clone(), index)),
            Err(e) => eprintln!("{folder}: {e}"),
        }
    }
    let indexing = started.elapsed();
    let entries: Vec<PluginEntry> = indexes
        .iter()
        .map(|(folder, index)| PluginEntry { folder, index })
        .collect();
    let overlaps = find_overlaps(&entries);

    println!("{} plugins indexed in {:.2?}", indexes.len(), indexing);
    let (mut objects, mut images, mut sounds) = (0, 0, 0);
    for o in &overlaps {
        match o.item {
            OverlapItem::Object { .. } => objects += 1,
            OverlapItem::Image(_) => images += 1,
            OverlapItem::Sound(_) => sounds += 1,
        }
    }
    println!(
        "{} overlaps: {objects} objects, {images} images, {sounds} sounds\n",
        overlaps.len()
    );
    for o in overlaps
        .iter()
        .filter(|o| matches!(o.item, OverlapItem::Object { .. }))
    {
        if let OverlapItem::Object { kind, name } = &o.item {
            println!(
                "{kind} {name:?}: {} overrides {} ({})",
                o.winner,
                o.overridden.join(", "),
                if o.fields.is_empty() {
                    "overwrite".to_string()
                } else {
                    o.fields.join(", ")
                }
            );
        }
    }
    println!("\nimages (first 12):");
    for o in overlaps
        .iter()
        .filter(|o| matches!(o.item, OverlapItem::Image(_)))
        .take(12)
    {
        if let OverlapItem::Image(name) = &o.item {
            println!("{name}: {} over {}", o.winner, o.overridden.join(", "));
        }
    }
}
