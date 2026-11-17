@echo off
cd /d "%~dp0"
echo Teste alternativo (Vulkan em vez de DirectX 12). O fundo pode ficar preto: tudo bem,
echo o que importa e se o cubo aparece. Fecha sozinho em 20 segundos.
spiderdesk.exe --poc --timeout 20 --backend vulkan
echo.
echo SpiderDesk fechou. Mande o spiderdesk-log.txt desta pasta (e um print desta janela).
pause
