//! Cursor global e click-through seletivo.
//!
//! O Windows nao entrega eventos de mouse para uma janela em click-through, entao a posicao do
//! cursor vem de `GetCursorPos` (global). A cada frame comparamos o cursor com as regioes
//! clicaveis (circulos em torno da aranha, ja em pixels de tela) e ligamos/desligamos
//! `CursorOptions::hit_test` (o winit alterna `WS_EX_TRANSPARENT`):
//!
//! * cursor fora de todas as regioes  -> `hit_test = false`: cliques passam para o app de baixo;
//! * cursor dentro                    -> `hit_test = true` : a janela recebe o clique (a aranha morre);
//! * histerese: liga em `r`, so desliga acima de `r * hysteresis`, e nunca desliga com o botao
//!   pressionado (para o "soltar" do clique nao vazar para outro app).

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow, RawHandleWrapper};

use super::window::OverlayGeometry;
use crate::config::Config;

/// Posicao do cursor, em pixels fisicos.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct CursorState {
    /// No desktop virtual (global).
    pub screen: Option<Vec2>,
    /// Relativa ao canto superior esquerdo do overlay (Y para baixo).
    pub local: Option<Vec2>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitCircle {
    /// Centro em coordenadas locais do overlay (px).
    pub center: Vec2,
    pub radius: f32,
}

/// Regioes clicaveis publicadas por quem desenha algo interativo (aranha, cubo de teste).
#[derive(Resource, Debug, Default)]
pub struct HitTargets {
    pub circles: Vec<HitCircle>,
    /// Captura o mouse na tela inteira (a aranha esta com o cursor): nada atravessa o overlay.
    pub capture_all: bool,
}

impl HitTargets {
    /// O ponto esta dentro de algum alvo (raio multiplicado por `scale`)?
    pub fn contains(&self, p: Vec2, scale: f32) -> bool {
        self.capture_all
            || self
                .circles
                .iter()
                .any(|c| c.center.distance(p) <= c.radius * scale)
    }
}

/// Estado do click-through por regiao nativa (X11).
#[derive(Default)]
pub struct NativeRegion {
    last: Option<Vec<(i32, i32, u32, u32)>>,
    unsupported: bool,
}

/// Quadrados (x, y, largura, altura) que cobrem cada circulo clicavel, em px locais.
pub fn region_rects(targets: &HitTargets) -> Vec<(i32, i32, u32, u32)> {
    if targets.capture_all {
        return vec![(0, 0, 32_000, 32_000)];
    }
    targets
        .circles
        .iter()
        .map(|c| {
            let r = c.radius.max(1.0);
            let side = (2.0 * r).round() as u32;
            (
                (c.center.x - r).round() as i32,
                (c.center.y - r).round() as i32,
                side,
                side,
            )
        })
        .collect()
}

/// Decisao pura do click-through (testavel sem Bevy/Win32).
pub fn want_hit_test(
    currently_on: bool,
    cursor: Option<Vec2>,
    targets: &HitTargets,
    hysteresis: f32,
    button_held: bool,
) -> bool {
    let Some(p) = cursor else {
        return false;
    };
    if currently_on {
        button_held || targets.contains(p, hysteresis)
    } else {
        targets.contains(p, 1.0)
    }
}

pub fn update_cursor(
    geo: Res<OverlayGeometry>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cursor: ResMut<CursorState>,
) {
    let global = crate::platform::cursor_position();
    let local = match global {
        Some(g) => Some(g - geo.origin.as_vec2()),
        // fora do Windows (desenvolvimento): usa o cursor da propria janela
        None => windows
            .single()
            .ok()
            .and_then(|w| w.physical_cursor_position()),
    };
    // evita marcar o recurso como alterado sem necessidade
    if cursor.screen != global || cursor.local != local {
        cursor.screen = global;
        cursor.local = local;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_click_through(
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    cursor: Res<CursorState>,
    targets: Res<HitTargets>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    mut options: Query<(&mut CursorOptions, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    mut held_since: Local<Option<f32>>,
    mut native: Local<NativeRegion>,
) {
    let Ok((mut opts, handle)) = options.single_mut() else {
        return;
    };

    // X11 (Linux): regiao de entrada nativa = exatamente os circulos dos mobs. O servidor X faz o
    // teste de clique sozinho; o `hit_test` do winit (sem suporte no X11) nao e usado.
    if let Some(handle) = handle
        && !native.unsupported
    {
        let rects = if geo.ready {
            region_rects(&targets)
        } else {
            Vec::new()
        };
        if native.last.as_ref() != Some(&rects) {
            if crate::platform::set_input_region(handle, &rects) {
                native.last = Some(rects);
            } else {
                native.unsupported = true; // Windows / Wayland nativo: usa hit_test abaixo
            }
        }
        if !native.unsupported {
            return;
        }
    }

    // estado FISICO dos botoes (nao depende de eventos da janela, que podem se perder)
    let mut held = crate::platform::mouse_button_down().unwrap_or_else(|| {
        buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right)
    });
    // trava de seguranca: segurar o botao nunca mantem o overlay capturando por mais de 4 s
    if held {
        let t0 = *held_since.get_or_insert(time.elapsed_secs());
        if time.elapsed_secs() - t0 > 4.0 {
            held = false;
        }
    } else {
        *held_since = None;
    }

    // antes de a janela estar pronta, mantem click-through total
    let want = geo.ready
        && want_hit_test(
            opts.hit_test,
            cursor.local,
            &targets,
            cfg.overlay.hit_hysteresis,
            held,
        );
    if opts.hit_test != want {
        opts.hit_test = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> HitTargets {
        HitTargets {
            circles: vec![HitCircle {
                center: Vec2::new(500.0, 400.0),
                radius: 30.0,
            }],
            capture_all: false,
        }
    }

    #[test]
    fn capture_all_takes_the_whole_screen() {
        let t = HitTargets {
            capture_all: true,
            ..Default::default()
        };
        assert_eq!(region_rects(&t), vec![(0, 0, 32_000, 32_000)]);
        assert!(want_hit_test(
            false,
            Some(Vec2::new(5.0, 900.0)),
            &t,
            1.3,
            false
        ));
        assert!(
            !want_hit_test(false, None, &t, 1.3, false),
            "sem cursor: atravessa"
        );
    }

    #[test]
    fn region_rects_cover_each_circle() {
        assert!(
            region_rects(&HitTargets::default()).is_empty(),
            "sem mobs: tudo atravessa"
        );
        let r = region_rects(&targets());
        assert_eq!(r, vec![(470, 370, 60, 60)]);
    }

    #[test]
    fn click_through_by_default() {
        let t = targets();
        assert!(!want_hit_test(
            false,
            Some(Vec2::new(100.0, 100.0)),
            &t,
            1.3,
            false
        ));
        assert!(!want_hit_test(false, None, &t, 1.3, false));
        assert!(!want_hit_test(
            false,
            Some(Vec2::new(500.0, 400.0)),
            &HitTargets::default(),
            1.3,
            false
        ));
    }

    #[test]
    fn turns_on_inside_radius_only() {
        let t = targets();
        assert!(want_hit_test(
            false,
            Some(Vec2::new(500.0, 429.0)),
            &t,
            1.3,
            false
        ));
        // dentro da faixa de histerese, mas ainda desligado: nao liga
        assert!(!want_hit_test(
            false,
            Some(Vec2::new(500.0, 435.0)),
            &t,
            1.3,
            false
        ));
    }

    #[test]
    fn hysteresis_keeps_on_until_well_outside() {
        let t = targets();
        // ligado e dentro de r*1.3 (39 px): continua ligado
        assert!(want_hit_test(
            true,
            Some(Vec2::new(500.0, 437.0)),
            &t,
            1.3,
            false
        ));
        // fora de r*1.3: desliga
        assert!(!want_hit_test(
            true,
            Some(Vec2::new(500.0, 445.0)),
            &t,
            1.3,
            false
        ));
    }

    #[test]
    fn button_held_never_turns_off_but_unknown_cursor_always_does() {
        let t = targets();
        assert!(want_hit_test(
            true,
            Some(Vec2::new(900.0, 900.0)),
            &t,
            1.3,
            true
        ));
        // sem posicao de cursor: click-through (seguranca)
        assert!(!want_hit_test(true, None, &t, 1.3, true));
        // botao pressionado nao LIGA o hit-test (so mantem)
        assert!(!want_hit_test(
            false,
            Some(Vec2::new(900.0, 900.0)),
            &t,
            1.3,
            true
        ));
    }

    #[test]
    fn no_targets_means_always_click_through() {
        let t = HitTargets::default();
        assert!(!want_hit_test(true, Some(Vec2::ZERO), &t, 1.3, false));
    }
}
