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
