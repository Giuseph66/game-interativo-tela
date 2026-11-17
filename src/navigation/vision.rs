//! "Visao" do heroi: encontra, na imagem de uma janela, as bordas horizontais visiveis onde da
//! para pisar: linhas de texto, bordas de botoes e caixas, divisorias, abas, imagens.
//!
//! Logica pura (so recebe pixels), entao funciona com qualquer aplicativo e pode ser testada sem
//! tela. Quem captura a imagem e a plataforma (no GNOME, a extensao em `tools/gnome-extension`).
//!
//! Como funciona, linha a linha de cima para baixo:
//!
//! 1. um pixel e "borda" quando o brilho muda bastante em relacao ao pixel de cima;
//! 2. bordas vizinhas na mesma linha sao unidas, tolerando os vaos entre letras e palavras;
//! 3. um trecho comprido e bem preenchido vira um **apoio**, desde que nao haja outro logo acima
//!    (assim cada linha de texto rende um apoio so, no topo das letras, e nao um por pixel).

/// Um apoio horizontal encontrado na imagem (coordenadas em pixels da imagem).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ledge {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub kind: LedgeKind,
}

impl Ledge {
    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LedgeKind {
    /// Borda continua: divisoria, lado de botao/caixa/imagem.
    Line,
    /// Topo de uma linha de texto (ou de algo "picotado" como icones lado a lado).
    Text,
}

impl LedgeKind {
    pub fn label(self) -> &'static str {
        match self {
            LedgeKind::Line => "borda",
            LedgeKind::Text => "texto",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VisionParams {
    /// Diferenca de brilho (0..255) entre um pixel e o de cima para contar como borda.
    pub contrast: u8,
    /// Vao maximo (px) entre bordas da mesma linha para uni-las (espaco entre palavras).
    pub max_gap: usize,
    /// Comprimento minimo (px) de um apoio.
    pub min_len: usize,
    /// Fracao minima do trecho que precisa ser borda de verdade.
    pub min_fill: f32,
    /// Distancia vertical minima (px) entre dois apoios sobrepostos.
    pub min_spacing: usize,
    /// Maximo de apoios por imagem (ficam os mais compridos).
    pub max_ledges: usize,
}

impl Default for VisionParams {
    fn default() -> Self {
        Self {
            contrast: 26,
            max_gap: 14,
            min_len: 26,
            min_fill: 0.22,
            min_spacing: 12,
            max_ledges: 500,
        }
    }
}

/// Brilho (0..255) de cada pixel de uma imagem RGBA de 8 bits.
pub fn luma_from_rgba(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8)
        .collect()
}

/// Brilho de um recorte `(x, y, w, h)` de uma imagem BGRA de 8 bits com `width` pixels por linha
/// (o formato que o GDI do Windows entrega). Mesmos pesos de [`luma_from_rgba`]. O recorte e
/// limitado a imagem; vazio se nao sobrar nada.
pub fn luma_from_bgra_crop(
    bgra: &[u8],
    width: usize,
    (x, y, w, h): (usize, usize, usize, usize),
) -> Vec<u8> {
    let height = if width == 0 {
        0
    } else {
        bgra.len() / (width * 4)
    };
    let w = w.min(width.saturating_sub(x));
    let h = h.min(height.saturating_sub(y));
    let mut out = Vec::with_capacity(w * h);
    for row in y..y + h {
        let start = (row * width + x) * 4;
        out.extend(
            bgra[start..start + w * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| ((p[2] as u32 * 77 + p[1] as u32 * 150 + p[0] as u32 * 29) >> 8) as u8),
        );
    }
    out
}

/// Encontra os apoios numa imagem de brilho `width` x `height` (linha a linha, de cima para baixo).
pub fn find_ledges(luma: &[u8], width: usize, height: usize, p: &VisionParams) -> Vec<Ledge> {
    if width < 4 || height < 4 || luma.len() < width * height {
        return Vec::new();
    }
    let mut out: Vec<Ledge> = Vec::new();
    // ultima linha em que cada coluna ganhou um apoio (para nao empilhar apoios colados)
    let mut last = vec![i64::MIN / 2; width];
    let mut edge = vec![false; width];
    let spacing = p.min_spacing as i64;

    for y in 1..height {
        let (above, row) = (
            &luma[(y - 1) * width..y * width],
            &luma[y * width..(y + 1) * width],
        );
        for x in 0..width {
            edge[x] = row[x].abs_diff(above[x]) >= p.contrast;
        }
        // trechos de borda, unindo vaos de ate `max_gap`
        let mut x = 0;
        while x < width {
            if !edge[x] {
                x += 1;
                continue;
            }
            let start = x;
            let mut end = x; // ultimo pixel de borda do trecho
            let mut filled = 0usize;
            let mut gap = 0usize;
            while x < width && gap <= p.max_gap {
                if edge[x] {
                    end = x;
                    filled += 1;
                    gap = 0;
                } else {
                    gap += 1;
                }
                x += 1;
            }
            let len = end - start + 1;
            if len < p.min_len {
                continue;
            }
            let fill = filled as f32 / len as f32;
            if fill < p.min_fill {
                continue;
            }
            // ja ha um apoio logo acima cobrindo a maior parte deste trecho?
            let crowded = last[start..=end]
                .iter()
                .filter(|&&ly| (y as i64 - ly) < spacing)
                .count();
            if crowded * 2 > len {
                continue;
            }
            for l in &mut last[start..=end] {
                *l = y as i64;
            }
            out.push(Ledge {
                x0: start as f32,
                x1: (end + 1) as f32,
                y: y as f32,
                kind: if fill >= 0.9 {
                    LedgeKind::Line
                } else {
                    LedgeKind::Text
                },
            });
        }
    }
    if out.len() > p.max_ledges {
        out.sort_by(|a, b| b.width().total_cmp(&a.width()));
        out.truncate(p.max_ledges);
        out.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));
    }
    out
}

/// Pinta os apoios por cima de uma imagem RGBA (depuracao: `--ledges entrada.png saida.png`).
pub fn draw_ledges(rgba: &mut [u8], width: usize, height: usize, ledges: &[Ledge]) {
    for l in ledges {
        let color: [u8; 4] = match l.kind {
            LedgeKind::Line => [40, 255, 90, 255],
            LedgeKind::Text => [255, 150, 20, 255],
        };
        let y = l.y as usize;
        for dy in 0..2 {
            let yy = y.saturating_sub(dy);
            if yy >= height {
                continue;
            }
            for x in (l.x0 as usize)..(l.x1 as usize).min(width) {
                let i = (yy * width + x) * 4;
                rgba[i..i + 4].copy_from_slice(&color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 400;
    const H: usize = 200;

    fn canvas(bg: u8) -> Vec<u8> {
        vec![bg; W * H]
    }

    fn fill(img: &mut [u8], x0: usize, y0: usize, x1: usize, y1: usize, v: u8) {
        for y in y0..y1 {
            for x in x0..x1 {
                img[y * W + x] = v;
            }
        }
    }

    /// "Texto": letras de 6 px de largura com 3 px entre elas e 9 px entre palavras de 5 letras.
    fn text_line(img: &mut [u8], x0: usize, y0: usize, letters: usize, height: usize, v: u8) {
        let mut x = x0;
        for i in 0..letters {
            fill(img, x, y0, x + 6, y0 + height, v);
            x += if i % 5 == 4 { 15 } else { 9 };
        }
    }

    #[test]
    fn blank_image_has_no_ledges() {
        assert!(find_ledges(&canvas(255), W, H, &VisionParams::default()).is_empty());
        assert!(find_ledges(&[], 0, 0, &VisionParams::default()).is_empty());
    }

    #[test]
    fn a_box_gives_solid_ledges_at_its_top_and_bottom() {
        let mut img = canvas(250);
        fill(&mut img, 50, 40, 250, 100, 60); // caixa escura de 200x60
        let l = find_ledges(&img, W, H, &VisionParams::default());
        assert_eq!(l.len(), 2, "{l:?}");
        assert_eq!((l[0].x0, l[0].x1, l[0].y), (50.0, 250.0, 40.0));
        assert_eq!(l[1].y, 100.0);
        assert!(l.iter().all(|l| l.kind == LedgeKind::Line));
    }

    #[test]
    fn each_text_line_gives_one_ledge_at_the_top_of_the_letters() {
        let mut img = canvas(20); // tema escuro, letras claras
        for row in 0..4 {
            text_line(&mut img, 30, 30 + row * 20, 24, 9, 220);
        }
        let l = find_ledges(&img, W, H, &VisionParams::default());
        assert_eq!(l.len(), 4, "uma por linha de texto: {l:?}");
        for (row, ledge) in l.iter().enumerate() {
            assert_eq!(ledge.y, (30 + row * 20) as f32);
            assert_eq!(ledge.kind, LedgeKind::Text);
            assert_eq!(ledge.x0, 30.0);
            assert!(ledge.width() > 200.0, "a linha inteira, palavras unidas");
        }
    }

    #[test]
    fn far_apart_words_are_separate_ledges_and_specks_are_ignored() {
        let mut img = canvas(255);
        text_line(&mut img, 20, 50, 5, 9, 0); // palavra de ~42 px
        text_line(&mut img, 250, 50, 5, 9, 0); // outra, longe
        fill(&mut img, 150, 120, 160, 123, 0); // sujeira de 10 px
        let l = find_ledges(&img, W, H, &VisionParams::default());
        assert_eq!(l.len(), 2, "{l:?}");
        assert!(l[0].x1 < 100.0 && l[1].x0 >= 250.0);
    }

    #[test]
    fn low_contrast_texture_is_not_a_ledge() {
        let mut img = canvas(128);
        for y in 0..H {
            for x in 0..W {
                img[y * W + x] = 120 + ((x * 7 + y * 13) % 16) as u8; // ruido suave
            }
        }
        assert!(find_ledges(&img, W, H, &VisionParams::default()).is_empty());
    }

    #[test]
    fn ledge_count_is_capped_keeping_the_longest() {
        let mut img = canvas(255);
        for i in 0..8 {
            fill(&mut img, 10, 10 + i * 22, 60 + i * 40, 12 + i * 22, 0);
        }
        let p = VisionParams {
            max_ledges: 3,
            ..VisionParams::default()
        };
        let l = find_ledges(&img, W, H, &p);
        assert_eq!(l.len(), 3);
        assert!(l.iter().all(|l| l.width() >= 250.0), "{l:?}");
        assert!(
            l.windows(2).all(|w| w[0].y <= w[1].y),
            "ordenados por altura"
        );
    }

    #[test]
    fn rgba_to_luma_weights_green_most() {
        let l = luma_from_rgba(&[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
        ]);
        assert!(l[1] > l[0] && l[0] > l[2]);
        assert_eq!(l[3], 255);
    }

    #[test]
    fn bgra_crop_matches_rgba_and_stays_inside_the_image() {
        // 3x2 BGRA: linha 0 = azul, verde, vermelho; linha 1 = branco, preto, cinza
        let px = |r: u8, g: u8, b: u8| [b, g, r, 255];
        let img: Vec<u8> = [
            px(0, 0, 255),
            px(0, 255, 0),
            px(255, 0, 0),
            px(255, 255, 255),
            px(0, 0, 0),
            px(128, 128, 128),
        ]
        .concat();
        let all = luma_from_bgra_crop(&img, 3, (0, 0, 3, 2));
        let rgba: Vec<u8> = img
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect();
        assert_eq!(all, luma_from_rgba(&rgba));
        // recorte do meio, 2x1 a partir de (1, 1): preto, cinza
        assert_eq!(
            luma_from_bgra_crop(&img, 3, (1, 1, 2, 1)),
            vec![all[4], all[5]]
        );
        // recorte maior que a imagem e cortado; fora dela, vazio
        assert_eq!(luma_from_bgra_crop(&img, 3, (2, 0, 10, 10)).len(), 2);
        assert!(luma_from_bgra_crop(&img, 3, (5, 0, 2, 2)).is_empty());
        assert!(luma_from_bgra_crop(&[], 0, (0, 0, 2, 2)).is_empty());
    }
}
