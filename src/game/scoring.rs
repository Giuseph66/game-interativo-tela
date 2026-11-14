//! Pontuacao: abates e sequencias.

use bevy::prelude::Resource;

/// Janela (s) em que um novo abate continua a sequencia.
const STREAK_WINDOW_SECS: f32 = 60.0;
/// Bonus por abate adicional na sequencia.
const STREAK_BONUS: u32 = 25;

#[derive(Debug, Clone, Default, Resource)]
pub struct Score {
    pub kills: u32,
    pub points: u32,
    pub streak: u32,
    pub best_streak: u32,
    last_kill_at: Option<f32>,
}

impl Score {
    /// Registra um abate em `now` (segundos de relogio do jogo). Devolve os pontos ganhos.
    pub fn register_kill(&mut self, now: f32, base_points: u32) -> u32 {
        let continues = self
            .last_kill_at
            .is_some_and(|t| now >= t && now - t <= STREAK_WINDOW_SECS);
        self.streak = if continues { self.streak + 1 } else { 1 };
        self.best_streak = self.best_streak.max(self.streak);
        self.last_kill_at = Some(now);

        let gained = base_points.saturating_add(STREAK_BONUS.saturating_mul(self.streak - 1));
        self.kills += 1;
        self.points = self.points.saturating_add(gained);
        gained
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_kill_gives_base_points() {
        let mut s = Score::default();
        assert_eq!(s.register_kill(10.0, 100), 100);
        assert_eq!((s.kills, s.points, s.streak), (1, 100, 1));
    }

    #[test]
    fn streak_adds_bonus_and_resets_after_window() {
        let mut s = Score::default();
        s.register_kill(0.0, 100);
        assert_eq!(s.register_kill(30.0, 100), 125);
        assert_eq!(s.register_kill(50.0, 100), 150);
        assert_eq!(s.best_streak, 3);
        // passou da janela: recomeca
        assert_eq!(s.register_kill(200.0, 100), 100);
        assert_eq!(s.streak, 1);
        assert_eq!(s.best_streak, 3);
        assert_eq!(s.kills, 4);
        assert_eq!(s.points, 100 + 125 + 150 + 100);
    }

    #[test]
    fn no_overflow_on_huge_values() {
        let mut s = Score {
            points: u32::MAX - 10,
            ..Default::default()
        };
        s.register_kill(0.0, u32::MAX);
        assert_eq!(s.points, u32::MAX);
    }
}
