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
| Modelos leves | GLBs de poucos MB carregam na hora; arquivos de 30–70 MB levam alguns segundos no **primeiro** spawn do tipo |

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

Os tipos são montados automaticamente: **aranha procedural** + **cada `.glb` em
`assets/models/`**. Para adicionar um bicho novo, coloque o `.glb` em `assets/models/`; se ele
andar de lado/de costas, crie um preset em `config/models/<nome>.toml` com `glb_yaw_deg`
(veja [assets/models/README.md](assets/models/README.md)). O nome do preset vira o rótulo.

```powershell
spiderdesk.exe --list-mobs                      # tipos disponiveis
spiderdesk.exe --spawn rat,rat,wolf_spider      # spawna ao iniciar (nome, arquivo ou indice)
```

Cada mob tem cérebro, rota e animação próprios; clicar mata só o mob sob o cursor. Clicar no
painel nunca mata um mob que esteja atrás dele (só contam cliques recebidos pelo overlay).

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
* Sem o Windows, a lista de janelas é simulada ([src/platform/fallback.rs](src/platform/fallback.rs)):
  os mobs andam pelas bordas da tela e de 3 janelas fictícias, não pelas suas janelas reais.
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
├── creature/         comportamento (máquina de estados), animação, aranha procedural, GLB, efeitos
├── game/             pontuação
├── mobs.rs           catálogo de tipos, spawn/matar/remover (vários mobs)
├── panel.rs          janela flutuante de spawn (Bevy UI)
├── capture.rs        --screenshot       diagnostics.rs  métricas       poc.rs  modo PoC
config/models/        presets (yaw) para os GLBs de animais testados
tools/make_test_glb.py  gera um GLB sintético de teste
tests/                cenários de desktop + simulação de 10 min simulados, sem GPU
```

### Overlay transparente

* Janela sem bordas, `transparent`, `AlwaysOnTop`, `skip_taskbar`, não-focável; cobre o monitor
  principal **menos 1 px de altura** (evita o modo "fullscreen otimizado" do Windows).
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
no teto) com `slerp` suave. Sem `spider.glb`, usa-se uma aranha procedural com marcha de
tetrápodes e IK de duas juntas (pés sem "patinar"). Com GLB
(`assets/models/spider.glb` ou `spider.model`): **auto-fit** de escala/posição (mede os ossos de modelos
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
* Só o **monitor principal** é coberto; janelas em outros monitores são ignoradas.
* Jogos em tela cheia exclusiva e janelas elevadas (UAC) ficam acima do overlay.
* O watchdog pode encerrar o app ao retomar de suspensão (ajuste `watchdog_secs`).
* Em Linux/X11 o `hit_test` não é suportado pelo winit (só afeta o desenvolvimento).
* A pontuação não é persistida; só o monitor principal e uma aranha.
* Uma captura (`--screenshot`) veio vazia uma única vez, durante compilações em paralelo com a
  máquina a load average ~20; 5 repetições seguidas e outras capturas posteriores saíram normais.
  Não consegui reproduzir (provável atraso de quadros sob carga), mas fica registrado.

## Evolução futura

* **Linhas internas** (painéis, separadores, barras de ferramentas): novas fontes de `Surface`
  em `PathGraph::build`, via **Windows UI Automation** (nem todo app expõe a geometria);
  sem visão computacional.
* Estado `Attacking`, persistência de pontos, vários monitores, janela que segue a aranha,
  ícone de bandeja.
