# SpiderDesk

Uma pequena aranha 3D que **vive sobre a área de trabalho do Windows**: anda pelas bordas das
janelas reais, escala as laterais, contorna cantos, foge do cursor e morre se você clicar nela
(e reaparece depois). Escrito em **Rust** com **Bevy 0.20** (wgpu), sem Electron/WebView.

> **Estado honesto deste protótipo.** O código foi desenvolvido e testado em **Linux**. Tudo que
> é lógica (navegação, comportamento, configuração, pontuação) tem testes automatizados que
> passam. O backend Win32 **compila e passa no clippy para o alvo Windows**, mas **nunca rodou em
> um Windows real**: transparência verdadeira, click-through seletivo e os 10 cenários nativos
> estão **pendentes** — veja [docs/TESTES_WINDOWS.md](docs/TESTES_WINDOWS.md) e a seção
> [Resultados dos testes](#resultados-dos-testes).

## Como rodar (forma mais eficiente)

### Uso diário no Windows 10/11 (recomendado)

1. Instale o Rust uma vez: <https://rustup.rs> (toolchain *stable* ≥ 1.97; o padrão MSVC serve).
2. Na pasta do projeto, **compile em release uma única vez** (a primeira compilação do Bevy leva
   ~10–15 min; as seguintes, segundos):
   ```powershell
   cargo build --release
   ```
3. Daí em diante **rode o executável direto**, sem passar pelo `cargo` (sem checagem de build,
   abre na hora). Rode **a partir da pasta do projeto**, para ele achar `assets/` e `config/`:
   ```powershell
   .\target\release\spiderdesk.exe
   .\target\release\spiderdesk.exe --spawn wolf_spider,rat,ant   # ja abre com esses mobs
   ```
   Ou gere uma pasta portátil (exe + `config/` + `assets/`) e rode de qualquer lugar:
   `scripts/package-windows.sh` (do Linux) — no Windows basta copiar `target\release\spiderdesk.exe`
   para junto de `config\` e `assets\`.
4. Para iniciar com o Windows: atalho para o `.exe` em `shell:startup`, com "Executar: Minimizada"
   (o console fica minimizado; ele mostra o log).

Na primeira vez, valide a transparência com o cubo de teste: `spiderdesk.exe --poc`.

**Não** use `cargo run` sem `--release` para uso diário: o build de debug é mais lento e maior.
Use-o só enquanto mexe no código (recompila mais rápido).

Para gastar menos CPU/GPU:

| Ajuste | Onde |
|---|---|
| Menos mobs simultâneos | `[mobs] max` (padrão 30) |
| Sem painel quando não precisa | `--no-panel` ou `[mobs] panel = false` |
| FPS menor em movimento | `[overlay] fps_limit` (padrão 60) |
| Sem antisserrilhado | `[overlay] msaa_samples = 1` (padrão) |
| Log mais enxuto | `set RUST_LOG=warn` antes de rodar |
| Modelos leves | os bichos procedurais não carregam arquivo nenhum; GLBs (`include_glb`) de 30–70 MB levam alguns segundos no **primeiro** spawn do tipo |

O app já reduz o próprio ritmo: 60 FPS com algo se mexendo, 20 Hz com tudo parado e o cursor
longe, 4 Hz quando não há nada visível.

### Encerrar

**Ctrl+Alt+Shift+Q** (configurável; funciona com qualquer janela em foco), o botão
**Sair do SpiderDesk** no painel, ou `taskkill /IM spiderdesk.exe /F`.

## Painel de mobs (janela flutuante)

Ao iniciar abre uma janela pequena, **transparente, sem bordas e sempre no topo**, no canto
superior direito:

* **`+ <tipo>`**: spawna um mob daquele tipo num ponto aleatório das bordas de janelas/tela;
* **Matar todos** / **Remover todos**; contador de mobs, abates e pontos;
* arraste pelo **título**; **`_`** minimiza (volta pela barra de tarefas).

Os tipos são **seis bichos feitos em código** (sem arquivo de modelo): **Aranha** (viúva-negra),
**Aranha-lobo**, **Formiga**, **Barata**, **Gafanhoto** e **Rato**. Cada um tem corpo próprio,
marcha própria e hitbox que acompanha o corpo (veja *Bichos procedurais* abaixo). O tipo dos mobs
iniciais vem de `[mobs] initial_kind` (`misto` sorteia um por mob).

Modelos `.glb` de terceiros continuam suportados, mas desligados: com `[mobs] include_glb = true`
cada `.glb` de `assets/models/` vira mais um tipo; se ele andar de lado/de costas, crie um preset
em `config/models/<nome>.toml` com `glb_yaw_deg` (veja [assets/models/README.md](assets/models/README.md)).

```powershell
spiderdesk.exe --list-mobs                        # tipos disponiveis
spiderdesk.exe --spawn rato,formiga,barata,lobo   # spawna ao iniciar (nome, apelido em ingles ou indice)
```

Cada mob tem cérebro, rota e animação próprios; clicar mata só o mob sob o cursor. Clicar no
painel nunca mata um mob que esteja atrás dele (só contam cliques recebidos pelo overlay).

## Ninhos e sacudir a janela

As janelas reais viram terreno de jogo:

* **Ninho.** Uma janela **parada** por `nests.calm_secs` (45 s) aceita ninho: um mob em repouso
  numa borda dela constrói um casulo ali. O ninho cresce (`build_secs`), depois choca um filhote
  a cada `hatch_interval_secs` (enquanto couber em `mobs.max`). Ninho velho
  (`elite_after_secs`) muda de cor e passa a chocar **elites**.
* **Espada.** Clicar no ninho é um golpe com a espada atual (barra de vida própria). Destruir
  rende `xp_per_nest` além do XP normal — vale mais que vários mobs, mas exige espada boa ou
  paciência.
* **Sacudir.** Arraste a janela **de um lado para o outro** rápido (`shake_reversals` inversões
  de sentido em `shake_window_secs`, cada trecho com pelo menos `shake_min_px`): os mobs dela
  perdem a aderência, **caem** até o chão ou o topo de outra janela, levam `fall_damage` e ficam
  atordoados por `stun_secs`; o ninho **racha** (`shake_nest_damage`), mas nunca morre só com
  isso — o golpe final é da espada. Um arrasto comum, redimensionar ou tremer a janela não
  conta. Mover a janela também zera a "calma" dela.
* **Elite.** Mais vida (`elite_hp_multiplier`), maior, e **morde**: se o cursor ficar a menos de
  `bite_radius_px` dele por `bite_windup_secs`, rouba `bite_xp` de XP (a espada já conquistada
  nunca é perdida). Bata e saia.
* Filhotes de ninho **não reaparecem** depois de mortos: quem repõe a ninhada é o ninho. Deixar
  um ninho vivo é uma fazenda de XP — até ele virar ninho de elite.
* O ninho viaja com a janela, some se ela for fechada/minimizada e fica pausado (invisível, não
  clicável, sem chocar) enquanto a borda dele estiver coberta por outra janela. "Remover todos"
  no painel também remove os ninhos. Tudo em `[nests]` de `config/default.toml`
  (`enabled = false` desliga).

Enquanto alguma janela está se movendo, a consulta de janelas passa de `window_poll_ms` (300 ms)
para `shake_poll_ms` (50 ms) — só nesse intervalo — para a sacudida não passar entre duas
amostras.

**Estado honesto:** a lógica (detecção de sacudida, regras do ninho, queda/dano/atordoamento,
mordida) tem testes automatizados, e o ciclo completo rodou no app real em Linux com janelas
simuladas (`SPIDERDESK_FAKE_SHAKE=10` sacode a janela simulada A a cada 10 s; `--auto-hit N`
também golpeia o primeiro ninho exposto). **Sacudir uma janela de verdade no Windows nunca foi
testado** — veja a seção 4 de [docs/TESTES_WINDOWS.md](docs/TESTES_WINDOWS.md). A mordida do
elite e a eclosão de elite só foram exercitadas nos testes, não no app.

## Roubo do mouse (teia no canto)

Uma teia fica desenhada num canto da tela (`[web] corner`).

**Regra: a aranha só rouba mouse PARADO.** Com uma **aranha** viva (Aranha ou Aranha-lobo) e a
teia inteira, o painel mostra `Aranha rouba o mouse se parar 20s`. Parou de mexer: vira contagem
regressiva (`Mouse parado! Aranha vem em 0:12`). Qualquer movimento zera a contagem
(`[web] idle_secs`). Ao zerar:

1. **Fisgada** — a aranha mira e lança um fio de seda até o cursor. Mexer o mouse enquanto o
   fio voa faz ela errar o bote (`Errou!`).
2. **Passeio** — recolhe o fio e sai andando pelas superfícies com o **cursor real** pendurado:
   ele balança como pêndulo, arrasta no chão e sente os seus puxões.
3. **Brincadeira** — em cada parada (`play_stops`) faz o cursor de ioiô.
4. **Teia** — leva o cursor para a teia do canto, onde ele fica preso (só se move dentro dela).

Como se soltar:

* **Clicar** = golpe de espada: no passeio corta o fio (`thread_hp`), na teia rasga a teia
  (`web_hp`). O painel mostra a vida que falta.
* **Sacudir o mouse** também desgasta o fio (`struggle_damage` por 1000 px).
* Matar a aranha que está com o fio solta o cursor na hora.
* **Segurança:** a teia solta sozinha após `max_trap_secs` (30 s), cada trecho do passeio tem
  tempo máximo, e o atalho de emergência encerra o app. O teclado nunca é afetado.

**Arrancar a teia.** A teia do canto não é fixa: clique nela (golpes de espada, `web_hp`) a
qualquer hora para arrancá-la. Sem teia não há roubo. Depois de `rebuild_secs` uma aranha vai até
o canto e tece outra (`build_secs`; o painel mostra `Aranha tecendo a teia: 40%`), e dá para
arrancar de novo no meio do trabalho.

Durante a caçada o overlay captura o mouse na tela toda (os cliques não vazam para os
aplicativos enquanto o cursor está sendo arrastado); fora dela, tudo continua atravessando.

Como o cursor real é movido: no Windows, `SetCursorPos`; no Linux, um mouse virtual absoluto em
`/dev/uinput` (funciona em Wayland e X11). Sem permissão de escrita em `/dev/uinput` o painel
mostra `Roubo do mouse: indisponivel` e nada mais muda.

**"Mouse parado" no Linux sem a extensão do GNOME:** em Wayland o app não enxerga o ponteiro
sobre apps nativos, então usa o tempo ocioso informado pelo sistema (mouse **ou teclado**):
digitar também adia a aranha. Com a extensão (ou no Windows) vale só o movimento do mouse.

**Estado honesto:** as regras têm testes automatizados ([src/game/heist.rs](src/game/heist.rs),
[tests/heist.rs](tests/heist.rs)) e rodaram no app real em **Linux/GNOME Wayland**, movendo o
cursor de verdade: fisgada, passeio, teia, soltura por tempo, fio cortado, e a regra do mouse
parado (17 s mexendo: nada; 6 s parado com `idle_secs = 6`: caçada). **Arrancar a teia com
cliques e a aranha refazendo só foram exercitados nos testes automatizados**, não no app. No
**Windows nada disso foi executado** (só compilado). O arquivo antigo
[src/game/trap.rs](src/game/trap.rs) (teia perto do cursor após mouse parado) não é usado pelo
app; dele só se aproveita a função `confine`.

## Herói (WASD): ande por dentro dos aplicativos

Um cavaleirinho que você controla com o teclado, luta com os bichos e anda **por dentro das
janelas**: ele pisa nas linhas de texto do VS Code, nos parágrafos, botões e imagens de uma
página do Chrome, nos ícones da área de trabalho, no topo das janelas.

**Para ele receber o teclado, clique no painel do SpiderDesk** (as teclas vão para a janela com
foco; assim WASD não escreve nos seus aplicativos). O painel avisa quando falta o clique.

| Tecla | Ação |
|---|---|
| `A` / `D` (ou setas) | andar |
| `Ctrl` | correr |
| `Shift` | andar devagar **sem cair das pontas** (para na beira de uma linha, janela ou ícone; vãos pequenos, como entre palavras, passam). Vale mais que `Ctrl` |
| `W` (ou ↑) | pular; de novo no ar = pulo duplo; soltar cedo = pulo curto |
| `W` segurado perto de lateral de janela, parede da tela ou fio de seda | no alto do pulo agarra e escala; `A`/`D` solta, `A`/`D` + `W` salta da parede |
| `S` (ou ↓) | descer da escada / atravessar o apoio para baixo |
| `Espaço` (ou `J`, `F`) | golpe de espada |

Clicar num botão do painel e depois apertar `Espaço` não aciona o botão (o foco dos botões é
limpo enquanto o herói está ligado).

**Como ele enxerga os aplicativos.** A plataforma entrega a imagem de cada janela e a "visão"
([src/navigation/vision.rs](src/navigation/vision.rs)) acha as bordas horizontais visíveis: topo
de cada linha de texto, lados de botões e caixas, divisórias, imagens. Cada uma vira um apoio de
mão única (sobe-se pulando por baixo). Funciona com qualquer aplicativo, sem ele cooperar; uma
imagem de 1920x1080 é analisada em ~3 ms. `--ledges entrada.png saida.png` mostra o que ele vê.

**Ele identifica onde está.** Ao pousar, o nome do lugar aparece em cima dele e no painel:
`Texto em: main.rs - Visual Studio Code`, `Borda em: Google Chrome`, `Janela: …`,
`Icone: Creality-V7`, `Chao da tela`. O que ele sabe: de qual janela é e se é texto ou borda.
O que ele **não** sabe: o papel do elemento ("botão Enviar", "link"). Chrome e VS Code só expõem
isso pela acessibilidade se forem abertos com uma opção especial, então preferi o caminho que
funciona sempre.

**O que fazer com ele**

* **Lutar.** O golpe vale 2x o dano da espada atual; pisar num bicho (cair em cima) vale 3x e
  quica. Encostar num bicho tira um coração (elite: dois) e empurra. 5 corações; sem nenhum, ele
  volta em 3 s. O XP é o mesmo da espada do mouse: a espada evolui para os dois.
* **Moedas** espalhadas pelos apoios (+2 XP cada) e **corações** quando falta vida.
* **Missão da estrela.** Sempre há uma estrela longe, de preferência dentro de um aplicativo; uma
  bolinha dourada ao lado do herói aponta o caminho e o painel diz onde ela está. +12 XP.
* **Lugares novos.** Primeira vez em cima de cada janela ou ícone: +3 XP.
* **Ninhos, fio e teia.** A espada dele destrói ninhos, corta o fio que prende o cursor e arranca
  a teia do canto. Com o mouse sequestrado, o teclado ainda funciona: ele é o resgate.
* Os bichos reagem a ele como reagem ao cursor (`mobs_notice`).

Tudo em `[hero]` no TOML; o botão **"Heroi (WASD): liga/desliga"** do painel liga e desliga.

**Requisitos e privacidade (Linux/GNOME).** Andar *dentro* dos aplicativos precisa da extensão
do GNOME (a mesma das janelas reais). Enquanto o herói está ligado, ela salva a imagem das 3
janelas da frente em `$XDG_RUNTIME_DIR/spiderdesk/cap-<id>.png` (pasta 0700, em memória, arquivos
0600), cerca de uma vez por segundo cada, e apaga tudo quando o SpiderDesk fecha ou o herói é
desligado. Nada sai do computador. `[hero] inside_apps = false` desliga as imagens (ele continua
andando em janelas e ícones).

**Windows.** Não precisa de nada instalado: uma thread copia as 3 janelas de aplicativo da frente
com `PrintWindow(PW_RENDERFULLCONTENT)` cerca de uma vez por segundo, recorta a parte visível
(limites do DWM), converte para brilho e roda a mesma visão. As imagens ficam só em memória
(nada é gravado em disco) e a cópia para quando o herói é desligado.

**Estado honesto**

* Regras: testes automatizados ([src/game/hero.rs](src/game/hero.rs),
  [src/navigation/vision.rs](src/navigation/vision.rs), [src/hero.rs](src/hero.rs)).
* Ponta a ponta num **GNOME Shell 49 isolado, sem tela** (não a sua sessão): extensão capturando
  um Chromium e um editor de texto reais, 87 apoios achados dentro deles, o herói subindo pelo fio
  e pela lateral da janela e pousando numa linha de código; teclado real (com o mapa de teclas
  antigo: pulo e corrida respondendo).
* Combate: rodou no app com teclas programadas (7 abates em 36 s, espada até Ouro, 1 morte).
* **Não verificado:** a extensão na sua sessão real; clicar no painel para dar o foco (no teste o
  foco já estava na janela do jogo); páginas rolando/vídeos; telas com escala diferente de 100%.
* **Windows:** captura implementada (`PrintWindow`, [src/platform/windows.rs](src/platform/windows.rs))
  e compila sem avisos, mas **nunca foi executada** num Windows real. Apps que não respondem ao
  `PrintWindow` (alguns jogos, vídeos protegidos) só ficam sem apoios internos.

## Barras do sistema (barra superior, barra de tarefas, dock)

O SpiderDesk pergunta ao sistema a **área útil** de cada tela (o espaço que os aplicativos podem
ocupar). O que fica fora dela — a barra superior do GNOME, a barra de tarefas do Windows, um dock
fixo — vira um bloco sólido na frente de tudo:

* os bichos não andam por cima nem por trás da barra; andam na beira dela (a base da barra
  superior é um teto, o topo da barra de tarefas é um chão);
* a teia fica no canto da área útil (no exemplo: 29 px abaixo do topo da tela principal);
* o herói bate a cabeça na barra superior e fica em pé na barra de tarefas; nada de ninho ou
  moeda sobre elas.

Fontes: Linux, `_GTK_WORKAREAS_D<n>` (uma área por tela, publicada pelo GNOME/Mutter), ou
`_NET_WORKAREA`; Windows, `GetMonitorInfo` (`rcWork`) de cada monitor. Com o desktop simulado,
`SPIDERDESK_FAKE_TOPBAR=29` simula uma barra. Barras que se escondem sozinhas (dock automático)
não reservam espaço e não contam.

## Linha de comando

| Opção | O que faz |
|---|---|
| `--spawn a,b,...` | Spawna mobs ao iniciar (rótulo do painel, nome do arquivo ou índice) |
| `--list-mobs` | Lista os tipos de mob e sai |
| `--no-panel` | Não abre o painel flutuante |
| `--poc` | Prova de conceito: cubo 3D transparente com click-through, sem mobs |
| `--list-windows` | Lista as janelas detectadas (bordas visíveis × brutas) e sai — não precisa de GPU |
| `--screenshot arq.png` | Salva um quadro do overlay (e `arq.panel.png` do painel) + estatística de alfa e sai |
| `--auto-kill N` / `--panel-selftest` | Testes: clique simulado num mob a cada N s / aciona os botões do painel |
| `--hero-demo seq` | Teste/demonstração: teclas programadas para o herói, ex.: `right:2,jump+right:0.5,attack` |
| `--ledges in.png out.png` | Marca numa imagem os apoios que o herói enxerga (verde = borda, laranja = texto) e sai |
| `--timeout N` | Encerra sozinho após N segundos |
| `--config arq.toml` | Usa outro arquivo de configuração |
| `--print-config` | Mostra a configuração efetiva |

### Compilar para Windows a partir do Linux

```bash
rustup target add x86_64-pc-windows-gnu      # + pacote mingw-w64
scripts/package-windows.sh                   # gera dist/spiderdesk-windows/ (exe + config + assets)
```
O linker do alvo está em [.cargo/config.toml](.cargo/config.toml).

### Linux (GNOME/KDE, Wayland ou X11)

```bash
cargo build --release && ./target/release/spiderdesk --spawn wolf_spider,rat
```

* **Sempre no topo:** o Wayland não permite que um app fique acima dos outros (o GNOME não tem
  protocolo para isso). Por isso, numa sessão Wayland com XWayland o SpiderDesk usa **X11
  automaticamente**, onde o gerenciador de janelas respeita `_NET_WM_STATE_ABOVE`; o app reafirma
  isso a cada 1 s para o overlay e o painel. Para forçar Wayland nativo (perde o "sempre no topo"):
  `SPIDERDESK_WAYLAND=1`.
* **Click-through no X11:** o winit não suporta `hit_test` no X11; o SpiderDesk define a **região de
  entrada** da janela (extensão SHAPE) como os quadrados dos mobs. O resto da tela atravessa
  para os apps; só o mob recebe o clique. O cursor global vem de `QueryPointer`, então a fuga do
  cursor funciona.
* **Janelas reais no Linux.** Os mobs andam nas bordas das janelas de verdade, de duas fontes
  ([src/platform/fallback.rs](src/platform/fallback.rs)); `--list-windows` mostra qual está em uso:
  * **Extensão do GNOME Shell** (necessária em Wayland, onde nenhum app pode perguntar onde estão
    as janelas dos outros): `./tools/install-gnome-extension.sh`, depois sair da sessão e entrar de
    novo. Ela publica retângulo, PID e título das janelas e a posição do ponteiro, num arquivo
    em `$XDG_RUNTIME_DIR/spiderdesk/`, e só enquanto o SpiderDesk está aberto; com o herói ligado
    também salva ali a imagem das 3 janelas da frente (veja a seção do herói). Remover:
    `gnome-extensions uninstall spiderdesk-windows@neurelix`.
  * **Servidor X**: sem a extensão, só aparecem apps X11/XWayland. Numa sessão Wayland em que
    todos os apps são nativos, a lista fica vazia e os mobs andam só nas bordas da tela.
  * `SPIDERDESK_FAKE_WINDOWS=1` volta ao desktop simulado com 3 janelas fictícias (invisíveis),
    usado nos testes de captura. Antes era o padrão — por isso os mobs pareciam andar em
    "caixas invisíveis".
  * O painel mostra o botão **"Andar nas janelas (instalar)"** quando a extensão falta (faz o
    mesmo que o script) e avisa quando falta só sair e entrar na sessão.
* **Elementos além de janelas.** Tudo em que um mob anda é um retângulo na mesma lista
  (`DesktopWindow`; ids a partir de `ELEMENT_ID_BASE` são elementos e podem ser pequenos). Hoje:
  * **Ícones da área de trabalho** (extensão Desktop Icons NG): a posição vem dos metadados dos
    arquivos (`gio info`). Só entram quando a lista de janelas é confiável (extensão ativa ou
    sessão X11): sem saber que janela cobre um ícone, ele viraria caixa invisível.
    `SPIDERDESK_ICONS=1` força, `=0` desliga.
  * **Barra superior do GNOME**, publicada pela extensão.
  * Não feito: ícones da área de trabalho no Windows. (Elementos *dentro* dos apps vêm da visão do herói, nos dois sistemas.)
* **Pontes de seda** (`[navigation] silk_threads`): cada janela ganha fios verticais dos cantos de
  baixo até a superfície logo abaixo (outra janela ou o chão), desenhados na tela. Ícones ganham
  um fio só, para baixo ou pendurado de cima, o que for mais curto. Sem eles uma janela solta no
  meio da tela é uma ilha de onde o mob não sai andando.
* `cargo test`, `cargo clippy` e `cargo fmt` rodam em qualquer SO.

## Como funciona

```
src/
├── app.rs            monta o App Bevy + CLI
├── config.rs         TOML (serde) com padrões, validação e escala por DPI
├── desktop.rs        consulta periódica das janelas → reconstrói o grafo só se algo mudou
├── overlay/          janela transparente, click-through seletivo, emergência, ritmo do laço
├── platform/         TUDO que depende do SO (Win32 `unsafe` isolado em windows.rs)
├── navigation/       geometria, grafo de caminhos, A*, locomoção (lógica pura)
├── creature/         comportamento (máquina de estados), animação, bichos procedurais
│                     (species.rs = plano do corpo de cada espécie), GLB opcional, efeitos
├── game/             pontuação
├── mobs.rs           catálogo de tipos, spawn/matar/remover (vários mobs)
├── panel.rs          janela flutuante de spawn (Bevy UI)
├── capture.rs        --screenshot       diagnostics.rs  métricas       poc.rs  modo PoC
config/models/        presets (yaw) para os GLBs de animais testados
tools/make_test_glb.py  gera um GLB sintético de teste
tests/                cenários de desktop + simulação de 10 min simulados, sem GPU
```

### Overlay transparente

* Janela sem bordas, `transparent`, `AlwaysOnTop`, `skip_taskbar`, não-focável; cobre **todos os
  monitores** (retângulo que envolve as telas; `overlay.all_monitors = false` volta a usar só o
  principal) **menos 1 px de altura** (evita o modo "fullscreen otimizado" do Windows). Os mobs
  andam livremente de uma tela para a outra; o painel fica no monitor principal.
* **O Bevy 0.20 só lê o tipo de swapchain do DX12 por variável de ambiente.** O padrão
  (`DxgiFromHwnd`) **não suporta transparência**; por isso `platform::prepare_graphics_env()`
  define `WGPU_DX12_PRESENTATION_SYSTEM=Visual` (DirectComposition) e o backend DX12 é forçado
  no Windows. Alfa: `PreMultiplied`, `ClearColor = NONE`, sem tonemapping/HDR.
* Estilos nativos: `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` (sem roubar foco, fora do Alt+Tab);
  "sempre no topo" é reafirmado a cada 2 s.

### Click-through seletivo

A janela **nasce** com `hit_test = false` (cliques atravessam). A cada quadro o cursor global
(`GetCursorPos`) é comparado com o círculo clicável da aranha (já em pixels de tela):

* dentro do raio → `hit_test = true` (a janela recebe o clique; a aranha morre);
* fora de `raio × 1,1` → volta a `false` (histerese). **Atenção:** enquanto ligado, a janela inteira
  captura o mouse, então cliques no anel entre `raio` e `raio × 1,1` (~3 px) são engolidos sem
  efeito; por isso a histerese é pequena. O círculo clicável é só aproximação do modelo;
* nunca desliga com o botão pressionado (estado **físico** via `GetAsyncKeyState`, trava de 4 s);
* cursor desconhecido → click-through.

O winit recalcula o *ex-style* inteiro a cada troca de `hit_test` (verificado em
`winit-0.30.13/.../window_state.rs`: `SetWindowLongW(GWL_EXSTYLE, ...)`), o que apagaria
`WS_EX_NOACTIVATE|WS_EX_TOOLWINDOW`; por isso `platform::ensure_overlay_styles` devolve esses bits
**a cada quadro** (leitura-modificação-escrita, preserva `TRANSPARENT/LAYERED`).

Não se usa `WM_NCHITTEST/HTTRANSPARENT` (não atravessa para outros processos).

### Segurança (nunca bloquear o computador)

Thread própria (`platform::Emergency`): `RegisterHotKey` + polling de reserva para o atalho;
**watchdog** que encerra o processo se o laço principal ficar > `watchdog_secs` sem bater;
saída forçada 3 s depois de qualquer pedido de encerramento que não complete.

### Janelas e caminhos

* Enumeração (`EnumWindows`) a cada `window_poll_ms` (300 ms), **nunca por quadro**; filtra
  invisíveis, minimizadas, *cloaked* (DWM), `TOOLWINDOW`, janelas com dono, sem título, shell
  (barra de tarefas/desktop) e o próprio processo. Limites visíveis via
  `DWMWA_EXTENDED_FRAME_BOUNDS` (reserva: `GetWindowRect`). O processo é DPI *per-monitor v2*;
  tudo é em **pixels físicos**.
* `PathGraph`: bordas visíveis das janelas (recortadas pelas janelas **na frente** na ordem Z),
  perímetro da tela, cantos compartilhados (contornar cantos) e *links* entre bordas de janelas
  a `< link_distance_px` (pontos de encontro). Reconstruído **só quando o conjunto muda**.
* A* sobre o grafo; destinos sorteados por comprimento de aresta; fuga escolhe o destino mais
  distante do cursor.
* Janela movida/redimensionada: a aranha **viaja junto** (mapeamento proporcional ao retângulo);
  a rota é revalidada; se a superfície sumiu (minimizada), **cai** até a de baixo.

### Comportamento

Máquina de estados `Idle · Walking · Climbing · Fleeing · Attacking (reservado) · Hit · Dead ·
Respawning`. Velocidade com variação suave e micro-pausas; ao cursor **entrar** no raio de
detecção (gatilho por entrada + *cooldown* + probabilidade) ela foge, congela, acelera ou muda
de rota. Clique: *Hit* → *Dead* (animação + explosão) → pontuação → some → reaparece.

### Render e animação

Câmera ortográfica 1 unidade = 1 px físico; o corpo é inclinado em direção à câmera
(`view_tilt_deg`) e a orientação acompanha a **normal** da superfície (escalando parede, pendurada
no teto) com `slerp` suave.

**Bichos procedurais** (`creature/species.rs` + `creature/procedural.rs`). Cada espécie é um
`BodyPlan` de dados puros: partes rígidas (elipsoides/cones) presas a três pivôs (corpo, cabeça,
traseira), pernas e cadeias flexíveis (antenas, cauda, bigodes, palpos, cercos), paleta PBR e um
`Profile` (tamanho, passo, fração de apoio, multiplicador de velocidade, giro de morte, hitbox).

| Bicho | Marcha | Detalhes animados |
|---|---|---|
| Aranha / Aranha-lobo | tetrápodes alternados (8 patas) | palpos remando, abdômen respirando e balançando |
| Formiga, Barata | tripé alternado (6 patas) | antenas ondulando (varrem para trás ao correr), cabeça olhando em volta |
| Gafanhoto | tripé; correndo, as patas saltadoras empurram juntas e o corpo dá pulinhos | antenas, abdômen respirando |
| Rato | andar em sequência lateral → meio-galope ao correr (coluna flexiona) | cauda em onda encostando no chão, bigodes e focinho farejando quando parado, morre de lado |

Pernas: IK de duas juntas + tarso apoiado; a fase avança pela distância percorrida e o passo cresce
com a velocidade, então o pé apoiado fica parado no chão (há teste garantindo alcance em todo o
ciclo para cada perna de cada espécie). **Hitbox**: até 3 círculos ao longo do corpo (cabeça, tronco,
traseira), recalculados pela posição/direção a cada passo; cauda, antenas e pernas não contam.
`spider.hit_radius_px` (padrão 34) escala os círculos de todos.

Com GLB (`include_glb = true`): **auto-fit** de escala/posição (mede os ossos de modelos
com esqueleto), animações por nome com seleção que evita variantes ("walking backward", "turn left"),
clipe único genérico vira `walk`, clipes ausentes usam *fallback* e repouso/golpe/morte sem clipe
**congelam** o andar em vez de andar no lugar. Veja [assets/models/README.md](assets/models/README.md)
e os presets de `config/models/`.

## Configuração

[config/default.toml](config/default.toml) documenta tudo (todos os campos são opcionais).
Principais: `overlay.fps_limit`, `overlay.emergency_hotkey`, `spider.scale/speeds/model`,
`behavior.detect_radius_px/respawn_delay_secs/...`, `navigation.window_poll_ms/link_distance_px`,
`diagnostics.metrics_interval_secs`. Valores em px escalam automaticamente com o DPI do monitor.

## Desempenho

* Laço dirigido por tempo (`reactive_low_power`): `fps_limit` (60) em movimento,
  `idle_update_hz` (20) parada com cursor longe, `hidden_update_hz` (4) morta/invisível
  (a lógica usa o tempo **real** decorrido, então o intervalo de respawn vale mesmo a 4 Hz;
  há teste de regressão para isso).
* Sem consulta de janelas por quadro; grafo reconstruído só em mudança; sem sombras;
  MSAA desligado por padrão; sem alocações por quadro no caminho quente.
* Métricas no log a cada `metrics_interval_secs`: FPS, tempo de quadro, atividade, consultas,
  reconstruções (tempo máx.), nós/arestas, memória. Medidas feitas **só no Linux** (consulta de
  janelas ≈ 10–18 µs, reconstrução do grafo ≈ 60 µs, ~265 MiB residentes com Vulkan); **não há
  medição de CPU/GPU no Windows ainda**.
* Custo conhecido: a janela cobre o monitor inteiro (superfície transparente de tela cheia).
  Alternativa futura: janela pequena que segue a aranha.

## Resultados dos testes

### Executados (Linux, fora do Windows)

| Verificação | Resultado |
|---|---|
| Bichos procedurais (`--spawn` das 6 espécies, `spider.scale = 3`, `--screenshot`) | todos renderizam e andam no chão, paredes e teto; sem erros/avisos no log |
| `cargo test` | **120 testes passam** (105 unitários + 7 cenários de desktop + 7 simulações + 1 de configuração) |
| `cargo fmt --check`, `cargo clippy --all-targets` (Linux) e `--target x86_64-pc-windows-gnu` | `Finished`, **0 avisos** nos dois alvos, sobre o código final |
| `cargo check` e `cargo build --release --target x86_64-pc-windows-gnu` | OK (gera `spiderdesk.exe`, ~64 MB) — a única validação de compilação do módulo Win32 |
| **Wine** (somente *smoke*, **não** é validação do Windows) | `spiderdesk.exe --help` e `--print-config` OK; `--list-windows` executa `EnumWindows`/DPI sem falhar (0 janelas, esperado num prefixo vazio) |
| Execução real no Linux (Wayland/XWayland, Vulkan) | janela dimensionada, aranha posicionada e animada, saída limpa por `--timeout`; 5/5 capturas com aranha |
| Transparência **do render** (`--screenshot`) | fundo com alfa 0 em ~99,9 % dos pixels, aranha/cubo com alfa 255, **sem pixels parciais** |
| GLB sintético (`tools/make_test_glb.py`) e GLB corrompido | carga + fallback de clipes OK; corrompido → erro registrado e aranha procedural assume |
| **5 animais reais** (lobo-aranha rigado, formiga, barata, gafanhoto, rato) | todos carregam, renderizam texturizados, com `AnimationPlayer` ligado, escala/posição automáticas e orientação correta com o preset de `config/models/` (veja [assets/models/README.md](assets/models/README.md)) |
| Ciclo morte→respawn no **app real** (`--auto-kill`, lobo-aranha) | clique → pontos com bônus de sequência (+100, +125, +150) → atividade `Hidden` (4 Hz) → reaparece no tempo configurado |
| **Vários mobs** no app real (`--spawn` com 7 tipos, 8 mobs simultâneos) | todos andam/animam ao mesmo tempo, cada GLB carregado uma vez por tipo, sem erros de ECS |
| **Painel flutuante** (`--screenshot` + `--panel-selftest`) | renderiza transparente/semitransparente; cada botão aciona o comando certo (6 spawns → "Matar todos" = 7 abates → "Remover todos"). **Não testado:** o clique físico do mouse no painel — nesta sessão Wayland o `xdotool` move o cursor mas não entrega cliques a nenhuma janela |
| Modelos com textura **JPEG** (Meshy/Hitem3d) | falhavam (`invalid image mime type: image/jpeg`); corrigido com a feature `jpeg` do Bevy |
| Simulação de 10 min simulados | aranha sempre sobre uma superfície, sem NaN, dentro da tela |

Cobertura dos cenários obrigatórios **no nível da lógica** (automatizada): 5 (mover janela →
rebuild + aranha acompanha), 6 (minimizar → caminhos somem → queda), 7 (maximizada),
8 (grafo invariante à escala DPI 100/125/150 %), 9 (mudança de ordem Z detectada e oclusão
recalculada), morte/respawn e fuga do cursor.

### **Pendentes — exigem Windows real** (nenhum foi executado)

Os 10 cenários da especificação *como experiência nativa*: 1 Chrome, 2 VS Code, 3 clique fora
chegando ao app certo, 4 clique matando a aranha com o clique **não** vazando, 5/6/7 com janelas
reais, 8 DPI real, 9 Alt+Tab real, 10 encerramento — mais **transparência real sobre o DWM** e
**click-through real**. Roteiro passo a passo em [docs/TESTES_WINDOWS.md](docs/TESTES_WINDOWS.md).

## Limitações conhecidas

* **Risco nº 1 (não validado):** ao alternar o click-through o winit liga `WS_EX_LAYERED` via
  `SetWindowLongW` e **nunca chama** `SetLayeredWindowAttributes`/`UpdateLayeredWindow` (conferido no
  código-fonte do winit 0.30.13); o Win32 documenta que uma janela *layered* assim pode não ser
  exibida. Se o overlay sumir/piscar ao passar o cursor sobre a aranha: 1) ligue
  `overlay.layered_alpha_fix = true` (aplica alfa constante 255, sem mudar a aparência — remédio
  hipotético, também não testado); 2) plano B: `SetWindowRgn` ou janela pequena que segue a aranha.
* **Vários monitores.** O overlay cobre o retângulo que envolve todas as telas (testado no Linux
  com duas telas 1920x1080 lado a lado: janela 3840x1079 em (0,0), mobs renderizados nas duas).
  Se as telas tiverem alturas ou alinhamentos diferentes, sobram "cantos mortos" fora de qualquer
  monitor onde um mob pode andar sem ser visto (o chão é a borda do retângulo, não de cada tela).
  O SpiderDesk também lê a posição **real** da janela a cada 0,5 s e ajusta cursor, cliques e
  espada, caso o gerenciador de janelas a desloque. Não testado no Windows com várias telas,
  nem com escalas de DPI diferentes entre os monitores.
* Jogos em tela cheia exclusiva e janelas elevadas (UAC) ficam acima do overlay.
* O watchdog pode encerrar o app ao retomar de suspensão (ajuste `watchdog_secs`).
* Em Linux/X11 o `hit_test` não é suportado pelo winit (só afeta o desenvolvimento).
* A pontuação não é persistida.
* Uma captura (`--screenshot`) veio vazia uma única vez, durante compilações em paralelo com a
  máquina a load average ~20; 5 repetições seguidas e outras capturas posteriores saíram normais.
  Não consegui reproduzir (provável atraso de quadros sob carga), mas fica registrado.

## Evolução futura

* **Linhas internas** (painéis, separadores, barras de ferramentas): novas fontes de `Surface`
  em `PathGraph::build`, via **Windows UI Automation** (nem todo app expõe a geometria);
  sem visão computacional.
* Estado `Attacking`, persistência de pontos, chão por monitor (sem cantos mortos), janela que segue a aranha,
  ícone de bandeja.
