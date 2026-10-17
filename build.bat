@echo off
set PATH=C:\Users\Lenovo\AppData\Local\Programs\AutoClaw2\resources\vendor\node;%PATH%
cd /d z:\IdeaProjects\at-stwitch-all\at-switch
rmdir /s /q node_modules
call npm install
call npm run tauri:build
pause
