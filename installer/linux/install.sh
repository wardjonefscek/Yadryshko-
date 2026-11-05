#!/bin/sh
# Установка Cad+ для текущего пользователя (без sudo): программа, значок и пункт в меню.
set -e
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/256x256/apps"
install -m 755 "$here/cadplus" "$HOME/.local/bin/cadplus"
install -m 644 "$here/cadplus.png" "$HOME/.local/share/icons/hicolor/256x256/apps/cadplus.png"
sed "s|^Exec=cadplus|Exec=$HOME/.local/bin/cadplus|" "$here/cadplus.desktop" > "$HOME/.local/share/applications/cadplus.desktop"
if [ -d "$HOME/Desktop" ]; then
  cp "$HOME/.local/share/applications/cadplus.desktop" "$HOME/Desktop/cadplus.desktop"
  chmod +x "$HOME/Desktop/cadplus.desktop"
fi
echo "Готово: Cad+ установлен в $HOME/.local/bin, ярлык добавлен в меню приложений."
