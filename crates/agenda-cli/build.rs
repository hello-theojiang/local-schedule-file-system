//! Embarque les fichiers de `ui/` dans le binaire (table générée, sans dépendance).
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            out.push(p.strip_prefix(base).unwrap_or(&p).to_path_buf());
        }
    }
}

fn main() {
    let ui = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../ui")
        .canonicalize()
        .expect("dossier ui/ introuvable");
    println!("cargo:rerun-if-changed={}", ui.display());
    let mut files = Vec::new();
    walk(&ui, &ui, &mut files);
    let mut s =
        String::from("/// Fichiers de l'interface : (chemin, contenu).\npub static ASSETS: &[(&str, &[u8])] = &[\n");
    for f in &files {
        let rel = f.to_string_lossy().replace('\\', "/");
        println!("cargo:rerun-if-changed={}", ui.join(f).display());
        let _ = writeln!(s, "    ({rel:?}, include_bytes!({:?})),", ui.join(f).to_string_lossy());
    }
    s.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("assets.rs");
    std::fs::write(out, s).unwrap();
}
