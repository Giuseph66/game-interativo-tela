# SpiderDesk no LinkedIn — roteiro

## O QUÊ

**Post 1 (principal): vídeo nativo** `spiderdesk_video_4x5.mp4` (39 s, 1080x1350, sem áudio, legendas embutidas)

| Tempo | Cena | Legenda na tela |
|---|---|---|
| 0–5 s | Bichos grandes andando sobre VS Code + Chrome | Meu desktop virou fase de jogo |
| 5–11 s | 14 bichos nas bordas das janelas, fios de seda | Os bichos andam pelas bordas das janelas de verdade |
| 11–23 s | Herói pulando linha por linha do código no VS Code | Um herói que sobe pelas linhas do código |
| 23–28 s | Saída do `--ledges`: plataformas que ele enxerga | O que ele enxerga: 199 plataformas em 1,7 ms |
| 28–35 s | Ninho na borda da janela, janela sacode, bichos caem | Janela parada vira ninho. Sacode a janela e eles caem. |
| 35–39 s | Close dos bichos | Bichos 3D gerados em código · 21 mil linhas Rust · 232 testes |

**Post 2 (reforço): carrossel PDF** `spiderdesk_carrossel.pdf` (5 slides 4:5, explica a visão computacional)

| Slide | Conteúdo |
|---|---|
| 1 | Gancho: herói em cima do código. "Como um herói anda em cima do código do VS Code?" |
| 2 | Captura crua da janela. "1. Ele só recebe a imagem da janela" |
| 3 | Mesma imagem com as bordas marcadas (antes/depois). "2. Visão computacional acha as bordas horizontais" |
| 4 | "199 plataformas em 1,7 ms", qualquer app, sem API de acessibilidade, nada sai do PC |
| 5 | Herói pousado com o rótulo "Texto em: species.rs". "3. Cada borda vira plataforma e ele sabe onde pisou" |

**Texto do post 1:**

```
Meu desktop virou fase de jogo. 🕷️

Construí o SpiderDesk: bichos 3D que vivem por cima da área de trabalho e usam as janelas abertas como cenário.

O que aparece no vídeo:
🕷️ Aranhas, formigas, baratas, gafanhotos e ratos andam pelas bordas das janelas reais, sobem pelas laterais e descem por fios de seda.
🗡️ Um herói controlado no teclado sobe pelas linhas de código do VS Code como se fossem plataformas.
🪺 Janela parada vira ninho. Sacudiu a janela, os bichos caem.

Por baixo do capô:
• Rust + Bevy, sem Electron nem WebView. Overlay transparente com click-through: só o bicho recebe o clique, o resto passa direto para os apps.
• Nenhum arquivo de modelo 3D. Cada espécie é um "plano de corpo" em código, com pernas em cinemática inversa e marcha própria (tripé alternado nos insetos, meio-galope no rato).
• Os bichos navegam por A* num grafo montado com as bordas visíveis das janelas, refeito só quando alguma janela muda.
• O herói "enxerga" os apps por visão computacional na imagem de cada janela: acha as bordas horizontais (linhas de texto, botões, imagens). Numa janela do VS Code foram 199 plataformas em 1,7 ms. Funciona com qualquer app, sem integração. As capturas ficam numa pasta temporária em memória e nunca saem do computador.
• 21 mil linhas de Rust e 232 testes automatizados.

Hoje roda no Linux (GNOME). O build de Windows já compila; o próximo passo é testar numa máquina real.

Qual app você deixaria eles invadirem primeiro? 👇

#rust #gamedev #bevyengine #computervision #linux
```

**Opcional (escolha antes de postar):**
- **Decisão sua: contar ou não que usou agentes de IA.** O texto acima não diz quanto tempo levou de propósito: "21 mil linhas" + "em dois dias" sem mencionar IA vai gerar desconfiança nos comentários. Se contar, troque por "Em dois dias, pilotando agentes de IA, construí o SpiderDesk:" e acrescente um parágrafo sobre como você dirigiu os agentes. Vira um gancho forte.
- Se gravar a cena do roubo do mouse (ver COMO), adicione antes de "Por baixo do capô":
  `🕸️ Deixou o mouse parado? Uma aranha fisga o cursor e arrasta até a teia no canto da tela. Pra se soltar, só clicando.`

**Primeiro comentário (você mesmo, logo após publicar):** link do repositório, se houver.
Hoje o repo não tem remote. Se for publicar no GitHub, mande o link que eu ajusto. Se não, use o comentário para contar um detalhe técnico (ex.: o truque do click-through no X11 com a extensão SHAPE).

**Texto do post 2 (carrossel), 2 a 3 dias depois:**

```
Como um herói de jogo consegue pisar nas linhas de código do VS Code sem nenhuma integração com o VS Code? 🧵

Sem plugin, sem API de acessibilidade: ele olha a imagem da janela e acha as bordas de cada linha de texto, botão e imagem. 199 plataformas em 1,7 ms.

Em 5 slides, do pixel até o pulo. (Os bichos invadindo o desktop estão no post anterior.)

#rust #gamedev #computervision
```

## COMO

1. **Vídeo**: subir `spiderdesk_video_4x5.mp4` como vídeo nativo (não como link do YouTube). Thumbnail: `brutos/slides_versao_video/carrossel_02_heroi.png`.
   O MCP do LinkedIn daqui só publica texto + 1 imagem (PNG/JPG/GIF), sem vídeo nem PDF. Por isso o vídeo e o carrossel precisam ser subidos por você no app/site. Pelo MCP dá para publicar o texto com uma imagem só (ex.: o slide do herói), mas aí perde o vídeo.
2. **Carrossel**: publicar como "Documento" (PDF) e dar o título "Como o herói enxerga o código".
3. **Cena extra do roubo do mouse (opcional, é a mais compartilhável)**. Ela precisa ser gravada **por você**, porque move o cursor de verdade (via `/dev/uinput`). Gravei tudo num GNOME isolado, onde isso não funciona.
   - Antes, saia e entre de novo na sessão do GNOME, uma vez só, e rode `gnome-extensions enable spiderdesk-windows@neurelix`. A extensão está instalada mas não carregada (`gnome-extensions info` diz que não existe). Sem ela, os bichos andam só nas bordas da tela, não nas janelas. Alternativa: o botão "Andar nas janelas (instalar)" do painel.
   - Abra VS Code/Chrome em janelas não maximizadas e feche o que for privado.
   - `cd ~/Neurelix/game/Game-interativo && ./target/release/spiderdesk --config postar/roubo_do_mouse.toml`
   - Inicie a gravação do GNOME com **Ctrl+Alt+Shift+R** (para com o mesmo atalho; vai para `~/Vídeos/Screencasts`).
   - Deixe o mouse parado por 8 s. A aranha fisga o cursor, passeia com ele e o leva para a teia. Clique para cortar o fio.
   - Para sair: botão "Sair do SpiderDesk" no painel, ou Ctrl+C no terminal. No Linux o atalho Ctrl+Alt+Shift+Q não funciona. A teia solta sozinha em 30 s, e o teclado nunca é bloqueado.
   - Se quiser, me passe o arquivo que eu corto e encaixo no vídeo.
4. Responder aos comentários na primeira hora; o alcance depende disso.

## PRA QUÊ

- Portfólio que chama atenção: mistura game dev, sistemas (overlay, click-through, Wayland/X11) e visão computacional num projeto que se entende em 3 segundos.
- Gancho visual: bicho andando em cima do código é algo que quase todo dev já desejou ou temeu.
- Abrir conversa com quem trabalha com Rust, jogos e automação de desktop.

## QUANDO

- **Post 1:** terça 17/11 ou quarta 18/11, às **8h30 de Brasília** (7h30 em Manaus). Terça a quinta de manhã costuma ser o melhor horário para conteúdo técnico no Brasil.
- **Post 2 (carrossel):** quinta 19/11 ou terça 24/11, mesmo horário.
- Evite sexta à tarde e fim de semana.

## ONDE

- Feed do LinkedIn, no seu perfil pessoal (perfil pessoal alcança mais que página de empresa).
- Repost opcional: comunidades Rust Brasil, r/rust (vídeo curto) e Bevy Discord #showcase. Lá o texto deve ser em inglês.

## O QUE NÃO DIZER (para não ser desmentido nos comentários)

- Não dizer que roda no Windows: só compila, nunca rodou numa máquina Windows.
- O vídeo foi gravado num GNOME isolado. A posição das janelas e a sacudida foram feitas por script, e o herói foi pilotado por uma sequência de teclas (`--hero-demo`). Se perguntarem, conte assim. A detecção da sacudida e os bichos caindo são reais.
- Os números (199 plataformas / 1,7 ms; 232 testes; 21 mil linhas) foram medidos hoje, 16/11/2026.

## ARQUIVOS

- `spiderdesk_video_4x5.mp4`: vídeo final
- `spiderdesk_carrossel.pdf` + `carrossel_01..05_*.png`: carrossel (explicação da visão)
- `brutos/slides_versao_video/`: slides 4:5 tirados do vídeo (capa, herói, visão, sacudida, números) para usar como imagem avulsa
- `brutos/vscode_janela_crua.png` + `brutos/ledges_vscode.png`: antes/depois da visão em tamanho original
- `roubo_do_mouse.toml`: config para gravar a cena do mouse
- `brutos/`: gravações 1920x1080 sem legenda (para recortar de outro jeito ou postar em 16:9)
