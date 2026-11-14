//! Localizacao da pasta `assets/` (em desenvolvimento e distribuida junto do executavel).

use std::path::{Path, PathBuf};

/// Candidatos em ordem de prioridade: `BEVY_ASSET_ROOT`, pasta do executavel, diretorio atual,
/// `CARGO_MANIFEST_DIR` (somente com `cargo run`).
fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(root) = std::env::var_os("BEVY_ASSET_ROOT") {
        v.push(PathBuf::from(root).join("assets"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        v.push(dir.join("assets"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        v.push(cwd.join("assets"));
    }
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
        v.push(PathBuf::from(manifest).join("assets"));
    }
    v
}

/// Pasta `assets/` preferida: a primeira que contem `rel_probe`; senao a primeira que existe.
pub fn asset_root(rel_probe: &str) -> Option<PathBuf> {
    let c = candidates();
    c.iter()
        .find(|d| d.join(rel_probe).is_file())
        .or_else(|| c.iter().find(|d| d.is_dir()))
        .cloned()
}

/// Caminho completo de um asset, se existir.
pub fn find_asset(rel: &str) -> Option<PathBuf> {
    candidates()
        .into_iter()
        .map(|d| d.join(rel))
        .find(|p| p.is_file())
}

/// Procura `config/default.toml` ao lado do executavel, no diretorio atual ou no projeto.
pub fn find_default_config() -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        v.push(dir.join("config/default.toml"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        v.push(cwd.join("config/default.toml"));
    }
    if let Some(m) = std::env::var_os("CARGO_MANIFEST_DIR") {
        v.push(Path::new(&m).join("config/default.toml"));
    }
    v.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_project_config_and_missing_assets_gracefully() {
        // `cargo test` define CARGO_MANIFEST_DIR
        assert!(find_default_config().is_some());
        assert!(find_asset("models/__nao_existe__.glb").is_none());
    }
}
