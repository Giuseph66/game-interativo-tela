@echo off
cd /d "%~dp0"
echo Teste de transparencia: deve aparecer so um cubo girando sobre a area de trabalho.
echo Fecha sozinho em 20 segundos.
spiderdesk.exe --poc --timeout 20
echo.
echo SpiderDesk fechou. Mande o spiderdesk-log.txt desta pasta (e um print desta janela).
pause
