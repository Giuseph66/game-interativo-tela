//! O `config/default.toml` distribuido precisa ser sempre valido.

use spiderdesk::config::Config;

#[test]
fn shipped_default_config_loads() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config/default.toml");
    let cfg = Config::load(&path).expect("config/default.toml invalido");
    assert_eq!(cfg.overlay.fps_limit, 60);
    assert_eq!(cfg.overlay.emergency_hotkey, "ctrl+alt+shift+q");
    assert!(spiderdesk::platform::Hotkey::parse(&cfg.overlay.emergency_hotkey).is_ok());
}
