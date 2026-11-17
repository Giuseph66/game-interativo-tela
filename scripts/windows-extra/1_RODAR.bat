@echo off
cd /d "%~dp0"
echo SpiderDesk rodando. Para sair: Ctrl+Alt+Shift+Q (ou botao "Sair" no painel).
spiderdesk.exe --spawn aranha,lobo,formiga,barata,gafanhoto,rato %*
echo.
echo SpiderDesk fechou. Mande o spiderdesk-log.txt desta pasta (e um print desta janela).
pause
