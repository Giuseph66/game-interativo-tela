//! Configuracao do aplicativo (TOML via serde).
//!
//! Todos os campos possuem valor padrao: um arquivo parcial (ou ausente) nunca impede
//! o aplicativo de iniciar. Valores absurdos sao corrigidos por [`Config::sanitize`].

use std::path::Path;

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "erro de leitura: {e}"),
            ConfigError::Parse(e) => write!(f, "TOML invalido: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Backend grafico solicitado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackendChoice {
    /// DX12 no Windows (unico com transparencia confiavel), automatico nos demais.
    #[default]
    Auto,
    Dx12,
    Vulkan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    /// Limite de FPS enquanto ha movimento.
    pub fps_limit: u32,
    /// Taxa de atualizacao quando a aranha esta parada e o cursor esta longe.
    pub idle_update_hz: f32,
    /// Taxa de atualizacao quando a aranha esta invisivel (morta/aguardando).
    pub hidden_update_hz: f32,
    /// Pixels removidos da altura do overlay (evita o modo "fullscreen otimizado").
    pub bottom_inset_px: u32,
    /// Atalho global de encerramento de emergencia. Ex.: "ctrl+alt+shift+q".
    pub emergency_hotkey: String,
    /// Se o laco principal ficar travado por mais que isso, o processo encerra.
    pub watchdog_secs: f32,
    pub backend: BackendChoice,
    /// Amostras MSAA (1, 2 ou 4). 1 = desligado (menor custo de GPU).
    pub msaa_samples: u32,
    /// Raio, em px, em que o hit-test liga; desliga a `hit_radius * hysteresis`.
    pub hit_hysteresis: f32,
    /// Windows: aplica alfa constante 255 (SetLayeredWindowAttributes) quando o winit liga
    /// WS_EX_LAYERED. Remedio hipotetico caso o overlay suma ao passar o cursor sobre a aranha.
    pub layered_alpha_fix: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            fps_limit: 60,
            idle_update_hz: 20.0,
            hidden_update_hz: 4.0,
            bottom_inset_px: 1,
            emergency_hotkey: "ctrl+alt+shift+q".to_string(),
            watchdog_secs: 5.0,
            backend: BackendChoice::Auto,
            msaa_samples: 1,
            hit_hysteresis: 1.1,
            layered_alpha_fix: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpiderConfig {
    /// Caminho relativo a `assets/` do modelo GLB. Se ausente, usa a aranha procedural.
    pub model: String,
    /// Escala de exibicao (1.0 = ~64 px de envergadura).
    pub scale: f32,
    /// Raio do circulo clicavel, em px (antes de `scale`).
    pub hit_radius_px: f32,
    /// Velocidades em px/s.
    pub walk_speed: f32,
    pub run_speed: f32,
    pub climb_speed: f32,
    /// Inclinacao do dorso em direcao a camera (graus) para dar sensacao 3D.
    pub view_tilt_deg: f32,
    /// Rotacao extra aplicada ao modelo GLB (graus, eixo Y) para alinhar a frente do modelo ao eixo +X.
    pub glb_yaw_deg: f32,
    /// Escala extra aplicada ao modelo GLB (multiplica o auto-fit).
    pub glb_scale: f32,
    /// Mede o GLB depois de carregado e o reescala/centraliza automaticamente (~3 unidades).
    pub glb_auto_fit: bool,
}

impl Default for SpiderConfig {
    fn default() -> Self {
        Self {
            model: "models/spider.glb".to_string(),
            scale: 1.0,
            hit_radius_px: 34.0,
            walk_speed: 90.0,
            run_speed: 260.0,
            climb_speed: 60.0,
            view_tilt_deg: 32.0,
            glb_yaw_deg: 0.0,
            glb_scale: 1.0,
            glb_auto_fit: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    /// Distancia (px) em que o cursor e percebido.
    pub detect_radius_px: f32,
    /// Chance de reagir cada vez que o cursor entra no raio.
    pub react_probability: f32,
    /// Pesos relativos das reacoes ao cursor.
    pub weight_flee: f32,
    pub weight_freeze: f32,
    pub weight_speedup: f32,
    pub weight_reroute: f32,
    /// Intervalo minimo entre reacoes (s).
    pub react_cooldown_secs: f32,
    pub idle_min_secs: f32,
    pub idle_max_secs: f32,
    /// Chance por segundo de uma micro-pausa durante a caminhada.
    pub pause_chance_per_sec: f32,
    pub flee_duration_secs: f32,
    /// Intervalo ate reaparecer apos a morte.
    pub respawn_delay_secs: f32,
    pub death_anim_secs: f32,
    /// Semente do gerador (0 = aleatoria a cada execucao).
    pub seed: u64,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            detect_radius_px: 140.0,
            react_probability: 0.65,
            weight_flee: 5.0,
            weight_freeze: 2.0,
            weight_speedup: 2.0,
            weight_reroute: 2.0,
            react_cooldown_secs: 2.5,
            idle_min_secs: 0.8,
            idle_max_secs: 3.5,
            pause_chance_per_sec: 0.12,
            flee_duration_secs: 2.2,
            respawn_delay_secs: 15.0,
            death_anim_secs: 1.1,
            seed: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NavigationConfig {
    /// Periodo da consulta de janelas (ms). Nunca a cada frame.
    pub window_poll_ms: u64,
    /// Distancia maxima (px) para ligar segmentos de janelas diferentes.
    pub link_distance_px: f32,
    /// Janelas menores que isso (largura ou altura) sao ignoradas.
    pub min_window_size_px: f32,
    /// Trechos de borda menores que isso sao descartados.
    pub min_segment_px: f32,
    /// Usa o perimetro da tela como caminho.
    pub screen_edges: bool,
    /// Desenha o grafo (gizmos) para depuracao.
    pub debug_draw: bool,
}

impl Default for NavigationConfig {
    fn default() -> Self {
        Self {
            window_poll_ms: 300,
            link_distance_px: 14.0,
            min_window_size_px: 120.0,
            min_segment_px: 24.0,
            screen_edges: true,
            debug_draw: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GameConfig {
    pub points_per_kill: u32,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            points_per_kill: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiagnosticsConfig {
    /// Intervalo do log de metricas (s). 0 = desligado.
    pub metrics_interval_secs: f32,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            metrics_interval_secs: 30.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Resource)]
#[serde(default)]
pub struct Config {
    pub overlay: OverlayConfig,
    pub spider: SpiderConfig,
    pub behavior: BehaviorConfig,
    pub navigation: NavigationConfig,
    pub game: GameConfig,
    pub diagnostics: DiagnosticsConfig,
}

impl Config {
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let mut cfg: Config = toml::from_str(text).map_err(ConfigError::Parse)?;
        cfg.sanitize();
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::from_toml_str(&text)
    }

    /// Carrega o arquivo; em qualquer falha devolve o padrao e a mensagem de aviso.
    pub fn load_or_default(path: &Path) -> (Self, Option<String>) {
        match Self::load(path) {
            Ok(cfg) => (cfg, None),
            Err(ConfigError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => (
                Self::default(),
                Some(format!("{} nao encontrado; usando padroes", path.display())),
            ),
            Err(e) => (
                Self::default(),
                Some(format!("{}: {e}; usando padroes", path.display())),
            ),
        }
    }

    /// Configuracao efetiva para um monitor com escala `dpi` (1.0 = 100%, 1.5 = 150%...):
    /// tudo que e medido em pixels fisicos cresce junto com a interface do sistema, para a
    /// aranha manter o mesmo tamanho aparente em relacao aos aplicativos.
    pub fn scaled(&self, dpi: f32) -> Config {
        let k = if dpi.is_finite() {
            dpi.clamp(0.5, 4.0)
        } else {
            1.0
        };
        let mut c = self.clone();
        c.spider.scale *= k;
        c.spider.walk_speed *= k;
        c.spider.run_speed *= k;
        c.spider.climb_speed *= k;
        c.behavior.detect_radius_px *= k;
        c.navigation.link_distance_px *= k;
        c.navigation.min_window_size_px *= k;
        c.navigation.min_segment_px *= k;
        c
    }

    /// Mantem valores dentro de faixas seguras.
    pub fn sanitize(&mut self) {
        let o = &mut self.overlay;
        o.fps_limit = o.fps_limit.clamp(5, 240);
        o.idle_update_hz = o.idle_update_hz.clamp(1.0, 60.0);
        o.hidden_update_hz = o.hidden_update_hz.clamp(1.0, 30.0);
        o.watchdog_secs = o.watchdog_secs.clamp(2.0, 120.0);
        o.bottom_inset_px = o.bottom_inset_px.min(64);
        o.hit_hysteresis = o.hit_hysteresis.clamp(1.0, 3.0);
        o.msaa_samples = match o.msaa_samples {
            0 | 1 => 1,
            2 | 3 => 2,
            _ => 4,
        };

        let s = &mut self.spider;
        s.scale = s.scale.clamp(0.25, 8.0);
        s.hit_radius_px = s.hit_radius_px.clamp(8.0, 400.0);
        s.walk_speed = s.walk_speed.clamp(5.0, 2000.0);
        s.run_speed = s.run_speed.clamp(s.walk_speed, 4000.0);
        s.climb_speed = s.climb_speed.clamp(5.0, 2000.0);
        s.view_tilt_deg = s.view_tilt_deg.clamp(0.0, 80.0);
        s.glb_scale = s.glb_scale.clamp(0.001, 1000.0);

        let b = &mut self.behavior;
        b.detect_radius_px = b.detect_radius_px.clamp(10.0, 2000.0);
        b.react_probability = b.react_probability.clamp(0.0, 1.0);
        b.idle_min_secs = b.idle_min_secs.clamp(0.05, 600.0);
        b.idle_max_secs = b.idle_max_secs.clamp(b.idle_min_secs, 600.0);
        b.pause_chance_per_sec = b.pause_chance_per_sec.clamp(0.0, 5.0);
        b.flee_duration_secs = b.flee_duration_secs.clamp(0.2, 60.0);
        b.respawn_delay_secs = b.respawn_delay_secs.clamp(0.5, 86_400.0);
        b.death_anim_secs = b.death_anim_secs.clamp(0.1, 10.0);
        b.react_cooldown_secs = b.react_cooldown_secs.clamp(0.0, 600.0);
        for w in [
            &mut b.weight_flee,
            &mut b.weight_freeze,
            &mut b.weight_speedup,
            &mut b.weight_reroute,
        ] {
            *w = w.clamp(0.0, 1000.0);
        }

        let n = &mut self.navigation;
        n.window_poll_ms = n.window_poll_ms.clamp(50, 10_000);
        n.link_distance_px = n.link_distance_px.clamp(1.0, 200.0);
        n.min_window_size_px = n.min_window_size_px.clamp(0.0, 4000.0);
        n.min_segment_px = n.min_segment_px.clamp(1.0, 1000.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_stable_under_sanitize() {
        let mut cfg = Config::default();
        let before = format!("{cfg:?}");
        cfg.sanitize();
        assert_eq!(before, format!("{cfg:?}"));
    }

    #[test]
    fn partial_toml_keeps_defaults() {
        let cfg = Config::from_toml_str("[spider]\nscale = 2.0\n").unwrap();
        assert_eq!(cfg.spider.scale, 2.0);
        assert_eq!(cfg.overlay.fps_limit, 60);
    }

    #[test]
    fn absurd_values_are_clamped() {
        let cfg = Config::from_toml_str(
            "[overlay]\nfps_limit = 100000\nmsaa_samples = 9\n[behavior]\nidle_min_secs = 50.0\nidle_max_secs = 1.0\n",
        )
        .unwrap();
        assert_eq!(cfg.overlay.fps_limit, 240);
        assert_eq!(cfg.overlay.msaa_samples, 4);
        assert!(cfg.behavior.idle_max_secs >= cfg.behavior.idle_min_secs);
    }

    #[test]
    fn invalid_toml_is_error_and_load_or_default_recovers() {
        assert!(Config::from_toml_str("[spider\nscale=").is_err());
        let (cfg, warn) = Config::load_or_default(Path::new("/definitely/not/here.toml"));
        assert!(warn.is_some());
        assert_eq!(cfg.overlay.fps_limit, 60);
    }

    #[test]
    fn dpi_scaling_multiplies_pixel_quantities_only() {
        let base = Config::default();
        let same = base.scaled(1.0);
        assert_eq!(format!("{base:?}"), format!("{same:?}"));

        let s150 = base.scaled(1.5);
        assert!((s150.spider.scale - 1.5).abs() < 1e-6);
        assert!((s150.spider.walk_speed - base.spider.walk_speed * 1.5).abs() < 1e-3);
        assert!(
            (s150.navigation.link_distance_px - base.navigation.link_distance_px * 1.5).abs()
                < 1e-3
        );
        assert!(
            (s150.behavior.detect_radius_px - base.behavior.detect_radius_px * 1.5).abs() < 1e-3
        );
        // grandezas que nao sao pixels ficam intactas
        assert_eq!(
            s150.behavior.respawn_delay_secs,
            base.behavior.respawn_delay_secs
        );
        assert_eq!(s150.overlay.fps_limit, base.overlay.fps_limit);
        // valores invalidos nao quebram
        assert_eq!(base.scaled(f32::NAN).spider.scale, 1.0);
        assert_eq!(base.scaled(0.0).spider.scale, 0.5);
        assert_eq!(base.scaled(100.0).spider.scale, 4.0);
    }

    #[test]
    fn shipped_default_toml_parses_and_matches_defaults() {
        let text = include_str!("../config/default.toml");
        let cfg = Config::from_toml_str(text).expect("config/default.toml deve ser valido");
        let def = Config::default();
        assert_eq!(format!("{cfg:?}"), format!("{def:?}"));
    }
}
