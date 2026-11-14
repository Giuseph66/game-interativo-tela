# Roteiro de testes nativos no Windows

O desenvolvimento deste protótipo foi feito em Linux. **Nada abaixo foi executado em um Windows
real**: este roteiro existe para você validar. Anote ✅/❌ e, se algo falhar, envie o log
(o app imprime no terminal; use `RUST_LOG=spiderdesk=debug` para mais detalhe).

Antes de começar, tenha o encerramento de emergência em mente: **Ctrl+Alt+Shift+Q** (ou
`taskkill /IM spiderdesk.exe /F` em outro terminal).

## 0. Diagnóstico rápido (sem janela/GPU)

```powershell
spiderdesk.exe --list-windows     # deve listar suas janelas abertas com bordas visiveis
spiderdesk.exe --print-config
```
Confira que Chrome/VS Code aparecem, que minimizadas **não** aparecem e que `visivel` (bordas
DWM) difere de `bruto` por ~7-8 px nas laterais/base em janelas comuns do Windows 10/11.

## 1. Etapa 1 — prova de conceito do overlay (`--poc`)

```powershell
spiderdesk.exe --poc
```
Um cubo laranja deve atravessar a tela sobre os seus aplicativos.

| # | Verificação | Esperado |
|---|---|---|
| 1.1 | Fundo | **Transparente**: você vê o desktop/apps normalmente (não preto, não cinza). Se ficar preto/opaco: o app já força DX12 + `WGPU_DX12_PRESENTATION_SYSTEM=Visual` (DirectComposition); veja no log a linha `overlay pronto`, rode `--poc --screenshot cubo.png` (mostra se o *render* tem alfa 0) e envie o log. `backend = "vulkan"` no Windows costuma não ter alfa. |
| 1.2 | Sem bordas/barra de título/ícone na barra de tarefas/Alt+Tab | ✔ |
| 1.3 | Clique **fora** do cubo (ícone, link no Chrome, botão no VS Code) | O app abaixo recebe o clique normalmente |
| 1.4 | Rolagem com a roda, digitação, arrastar uma janela pela barra de título, selecionar texto | Tudo funciona normalmente com o overlay ligado |
| 1.5 | Clique **no** cubo | Cubo troca de cor e **nenhum** clique chega ao app embaixo |
| 1.6 | Foco | Ao clicar no cubo, a janela ativa continua ativa (barra de título não "apaga") |
| 1.7 | Arrastar uma janela passando por baixo do cubo | A janela continua arrastável; cubo fica por cima |
| 1.8 | Ctrl+Alt+Shift+Q | App fecha em < 1 s; mouse/teclado normais depois |
| 1.9 | Ponto de atenção #1 | Ao passar o cursor sobre o cubo (liga o hit-test) o overlay **não pode ficar em branco/piscar**: o winit liga `WS_EX_LAYERED` ao alternar o click-through (sem chamar `SetLayeredWindowAttributes`) e isso precisa conviver com o swapchain DirectComposition. Se sumir: teste `overlay.layered_alpha_fix = true`. |
| 1.10 | Foco após o hit-test | O winit apaga `NOACTIVATE/TOOLWINDOW` ao alternar o hit-test e o app os devolve a cada quadro: confirme que clicar na aranha/cubo **não** tira o foco do app ativo e que o overlay **não** aparece no Alt+Tab mesmo depois de passar o cursor sobre ele. |
| 1.11 | Anel de histerese | Clicar a ~2-3 px da borda do círculo clicável pode ser engolido (por projeto); clicar a 10+ px deve chegar ao app abaixo. |

Também útil: `spiderdesk.exe --poc --screenshot cubo.png` imprime a % de pixels com alfa 0
(deve ser ~99%) — confirma a transparência **do render** (não do compositor).

## 2. Cenários obrigatórios (aranha)

```powershell
spiderdesk.exe
```
Observe também as métricas no log (a cada 30 s): FPS, tempo de consulta/reconstrução, memória.

| # | Cenário | Como testar | Esperado |
|---|---|---|---|
| 1 | Chrome com a aranha por cima | Abra o Chrome maximizado e restaurado | Aranha anda no topo/laterais/base das janelas e no perímetro da tela, sempre por cima |
| 2 | VS Code utilizável | Digite, rode o terminal integrado, use menus | Zero interferência |
| 3 | Clique fora chega ao app certo | Clique em abas/botões longe da aranha, e **bem perto** dela | App correto recebe |
| 4 | Clique na aranha mata | Clique nela | Animação de morte + explosão verde; `+100 pontos` no log; some; reaparece após `respawn_delay_secs` (15 s) |
| 5 | Mover janela recalcula | Arraste uma janela com a aranha sobre ela | Aranha viaja junto; ao parar, escolhe nova rota |
| 6 | Minimizar remove trajetórias | Minimize a janela onde ela está | Aranha cai até a superfície abaixo (queda), caminhos da janela somem |
| 7 | Maximizar | Maximize uma janela | Sem bugs; aranha usa a base da janela (acima da barra de tarefas) e o perímetro da tela |
| 8 | DPI | Repita com escala 100%, 125% e 150% (Configurações → Tela) | Tamanho da aranha proporcional à interface; bordas alinhadas com as janelas (sem deslocamento) |
| 9 | Alt+Tab | Alterne entre apps | Overlay não aparece na lista; ordem Z muda os trechos visíveis; overlay continua no topo |
| 10 | Encerramento | Ctrl+Alt+Shift+Q; depois `taskkill` com o app travado de propósito | Área de trabalho nunca fica bloqueada |

Casos extras que valem a pena: monitor secundário (só o principal é coberto), jogo em tela cheia
exclusiva (overlay não aparece), suspender/retomar o PC (o watchdog pode encerrar o app —
aumente `watchdog_secs` se incomodar), UAC/janelas elevadas (o overlay não cobre a mesa segura).

## 3. Painel de mobs (janela flutuante)

| # | Verificação | Esperado |
|---|---|---|
| 3.1 | Ao abrir | Painel no canto superior direito, fundo escuro semitransparente, cantos arredondados, sem bordas do Windows |
| 3.2 | Clique em `+ rat`, `+ wolf_spider`... | Mob aparece numa borda; contador "Mobs" sobe; log `spawn: ...` |
| 3.3 | Mob passando **atrás** do painel + clique num botão | O botão funciona e o mob **não** morre |
| 3.4 | Arrastar pelo título | Painel acompanha o mouse |
| 3.5 | `_` | Minimiza; volta pela barra de tarefas |
| 3.6 | Matar todos / Remover todos / Sair | Todos morrem (pontos) / somem / app fecha |
| 3.7 | Com 30 mobs (`[mobs] max`) | Painel avisa no log ao passar do limite; desktop continua fluido |
| 3.8 | DPI 125%/150% | Painel proporcional (texto legível, botões cabem) |

## 4. Ninhos e sacudir a janela

Para não esperar os tempos padrão, use um arquivo de configuração de teste:

```toml
[nests]
calm_secs = 5.0
build_chance_per_sec = 5.0
build_secs = 2.0
hatch_interval_secs = 6.0
elite_after_secs = 20.0
```

```powershell
spiderdesk.exe --config teste-ninhos.toml --spawn aranha,aranha,aranha,aranha
```

| # | Passo | Esperado |
|---|---|---|
| N1 | Deixe o Bloco de Notas parado ~10 s com mobs andando nele | Aparece "Ninho!" e um casulo claro numa borda da janela; ele cresce por ~2 s |
| N2 | Espere | A cada ~6 s nasce um filhote no ninho; depois de ~20 s o casulo fica roxo e aparece "Elite!" |
| N3 | Arraste a janela devagar para outro lugar | O casulo vai junto; **nenhum** mob cai; não aparece "Sacudida!" |
| N4 | **Sacuda** a janela (vai-e-vem rápido, 3+ inversões em ~1 s) | "Sacudida!"; os mobs da janela caem até o chão/outra janela, piscam, mostram "-3" e ficam ~1,5 s parados; a barra de vida do ninho aparece menor |
| N5 | Continue sacudindo sem parar por 5 s | Conta de novo só a cada ~2,5 s; o ninho para em 1 de vida e **não** some |
| N6 | Clique no casulo | Número de dano, barra de vida; ao destruir: explosão, "+XP" grande; o clique **não** vaza para a janela de baixo |
| N7 | Clique num mob parado em cima do casulo | Quem leva o golpe é o mob, não o ninho |
| N8 | Cubra a borda do ninho com outra janela | O casulo some e para de chocar; descobrindo, volta |
| N9 | Minimize ou feche a janela do ninho | O ninho some de vez (restaurar não o traz de volta) |
| N10 | Deixe o cursor parado ~1,5 s a menos de ~90 px de um elite | "Mordida! -5 XP" em vermelho; o XP do painel cai, a espada não muda |
| N11 | Redimensione a janela em "sanfona" rápido | Não conta como sacudida |

Pontos de atenção no Windows real (nada disto foi medido): se a sacudida **não** for
reconhecida, rode com `RUST_LOG=debug` e confira se aparecem várias linhas `grafo vN` por
segundo durante o arrasto (a consulta rápida de 50 ms precisa estar ativa); se o gesto natural
for mais curto/lento que o padrão, ajuste `shake_min_px` / `shake_window_secs`. Confira também
o uso de CPU **durante** o arrasto (o laço vai a `fps_limit` e o grafo é reconstruído a cada
amostra em que a janela mudou).

## 5. Roubo do mouse (teia no canto)

Nada desta seção foi executado no Windows; no Linux o ciclo completo rodou. Para testar rápido,
use um TOML com `[web] idle_secs = 8` e deixe ao menos uma aranha viva.

| # | Cenário | Esperado |
|---|---|---|
| M1 | Abrir o app e olhar o canto superior esquerdo | Teia desenhada no canto; painel mostra `Aranha rouba o mouse se parar 8s` |
| M2 | Mexer o mouse sem parar por 1 min | Nenhuma caçada; o painel não sai da frase acima |
| M2b | Parar o mouse | Painel vira `Mouse parado! Aranha vem em 0:08` e desce; um toque no mouse volta ao início |
| M2c | Matar todas as aranhas (deixar outros bichos) | Painel: `Roubo do mouse: sem aranha` |
| M3 | Deixar o mouse parado até zerar | Fio de seda vai da aranha até o cursor; o cursor passa a acompanhar a aranha, pendurado e balançando |
| M3b | Mexer o mouse no instante em que o fio é lançado | `Errou!`; nada de captura; a contagem recomeça após 3 s |
| M4 | Durante o passeio, clicar em qualquer lugar | Nenhum clique chega aos apps; cada clique tira vida do fio (painel `Fio no mouse! Clique: x/8`); ao zerar: `Fio cortado! +15 XP`, cursor livre, aranha atordoada |
| M5 | Durante o passeio, sacudir o mouse com força | O cursor balança com os puxões; o fio fica vermelho e rompe |
| M6 | Não reagir | A aranha para, faz o cursor de ioiô, e depois o leva para a teia do canto; painel `Mouse na teia! Clique: 12/12 (30s)` |
| M7 | Preso, tentar tirar o cursor da teia | O cursor volta para dentro da teia |
| M8 | Preso, clicar na teia | Vida da teia cai; ao zerar: `Teia rasgada! +15 XP`, cursor livre |
| M9 | Preso, esperar `max_trap_secs` | `A teia soltou`; cursor livre; contagem recomeça |
| M10 | A aranha que carrega o cursor morre (por exemplo, queda após sacudir a janela dela antes da fisgada terminar) | O cursor é solto na hora: `Livre!` |
| M11 | Atalho de emergência no meio da caçada | App encerra; cursor livre imediatamente |
| M12 | Dois monitores | A aranha consegue levar o cursor de uma tela para a outra; o cursor nunca some |
| M13 | Com o mouse livre, clicar na teia do canto até a vida zerar | `Teia arrancada! +15 XP`; a teia some; painel `Sem teia: aranha refaz em 0:25`; nenhuma caçada acontece sem teia |
| M14 | Esperar `rebuild_secs` | Uma aranha anda até o canto; a teia cresce do canto (`Aranha tecendo a teia: N%`); depois a regra do mouse parado volta a valer |
| M15 | Clicar na teia enquanto a aranha tece | A teia some de novo; a aranha volta à vida normal |

## 6. Herói (WASD)

Nada desta seção foi executado no Windows. No Windows o herói anda em janelas, mas **não por
dentro delas** (as imagens das janelas não estão implementadas lá): H6–H8 valem só para
Linux/GNOME com a extensão.

| # | Cenário | Esperado |
|---|---|---|
| H1 | Abrir o app | Cavaleirinho no meio do chão da tela, 5 corações em cima; painel `Heroi: vida 5/5` |
| H2 | Apertar `D` sem clicar no painel | Nada (e a letra vai para o app em foco); painel mostra `>> clique aqui p/ usar o teclado <<` |
| H3 | Clicar no painel; `A`/`D`, `Ctrl`, `W`, `W` no ar | Anda, corre, pula, pulo duplo; para nas bordas da tela; nenhuma letra aparece nos apps |
| H3b | Em cima de uma janela, `Shift`+`D` até a ponta | Anda devagar e para na beira, sem cair; soltar `Shift`, cai |
| H3c | Clicar em "+ Aranha" no painel e depois apertar `Espaço` várias vezes | Só golpes de espada; nenhuma aranha extra aparece |
| H4 | Andar até a lateral de uma janela ou um fio de seda e segurar `W` | Pula e agarra no alto do pulo; escala; no topo, sobe em cima da janela; aparece `Janela: <título>` e `Lugar novo! +3 XP` |
| H5 | Em cima de uma janela, `S` | Atravessa e cai até o próximo apoio; no chão da tela `S` não faz nada |
| H6 | (GNOME) Abrir um editor com texto; pular para dentro da janela | Pousa numa linha de texto: `Texto em: <título>`; moedas aparecem sobre linhas e botões |
| H7 | (GNOME) Rolar o texto devagar com o herói em cima de uma linha | Ele acompanha a linha ou cai para a próxima; nunca fica flutuando |
| H8 | (GNOME) Fechar o app; olhar `$XDG_RUNTIME_DIR/spiderdesk/` | Nenhum `cap-*.png` sobra |
| H9 | `Espaço` perto de um bicho | Golpe de cima para baixo; dano = 2x a espada; barra de vida do bicho aparece |
| H10 | Cair em cima de um bicho | Pisão: dano 3x e o herói quica |
| H11 | Encostar num bicho | `-1`, empurrão, pisca por 1,6 s sem levar dano; 5 vezes: `Derrotado!`, volta em 3 s |
| H12 | Seguir a bolinha dourada até a estrela | `Estrela! +12 XP`; outra estrela aparece longe; painel diz onde |
| H13 | Com o cursor sequestrado pela aranha, `Espaço` | O golpe do herói corta o fio (`Fio cortado!`) |
| H14 | Botão `Heroi (WASD): liga/desliga` | Herói, moedas e corações somem/voltam |
| H15 | Arrastar a janela em que o herói está | Ele vai junto |

## 7. Barras do sistema (barra de tarefas, barra superior)

| # | Cenário | Esperado |
|---|---|---|
| B1 | Barra de tarefas embaixo (padrão do Windows) | Bichos e herói andam no topo da barra, nunca por trás dela; nada de teia, ninho ou moeda em cima dela |
| B2 | Barra de tarefas no topo ou na lateral | Mesma coisa naquela borda; a teia do canto fica dentro da área útil |
| B3 | Dois monitores, barra só no principal | No outro monitor os bichos usam a borda inteira da tela |
