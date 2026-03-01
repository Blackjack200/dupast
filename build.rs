use rustc_hash::FxHashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, wincode::SchemaWrite, wincode::SchemaRead,
)]
struct SerializedSynonymGraph {
    graph: FxHashMap<String, Vec<(String, f32)>>,
}

fn build_graph_from_simple_format(data: &str) -> FxHashMap<String, Vec<(String, f32)>> {
    let mut graph: FxHashMap<String, Vec<(String, f32)>> = FxHashMap::default();

    for line in data.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 2 {
            continue;
        }

        let word = parts[0].trim().to_lowercase();
        let synonyms_str = parts[1].trim();

        if word.is_empty() {
            continue;
        }

        let synonyms: Vec<String> = synonyms_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty() && s != &word)
            .collect();

        if synonyms.is_empty() {
            continue;
        }

        for syn in &synonyms {
            graph
                .entry(word.clone())
                .or_default()
                .push((syn.clone(), 1.0));
            graph
                .entry(syn.clone())
                .or_default()
                .push((word.clone(), 1.0));
        }
    }

    graph
}

fn main() {
    println!("cargo:rerun-if-changed=synonyms.txt");

    let source_path = PathBuf::from("synonyms.txt");
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", source_path.display(), e));

    let serialized = SerializedSynonymGraph {
        graph: build_graph_from_simple_format(&source),
    };

    let bytes = wincode::serialize(&serialized)
        .unwrap_or_else(|e| panic!("failed to serialize embedded synonym graph: {}", e));

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is not set"));
    let out_file = out_dir.join("embedded_synonyms.wincode");
    fs::write(&out_file, bytes)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", out_file.display(), e));
}
