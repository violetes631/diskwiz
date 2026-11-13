<h1 align="center">DiskWiz</h1>

<p align="center">
  Быстрая очистка диска и анализатор занятого места в стиле WizTree<br>
  для Linux и Windows (macOS — по мере сил). GTK4 + libadwaita, написано на Rust.
</p>

<p align="center"><a href="README.md">English</a></p>

## Возможности

**Анализатор диска** — многопоточный сканер (≈260 000 файлов / 210 ГиБ за ~0,2 с), реальные размеры
на диске, дерево папок, интерактивная карта (treemap) с цветом по типу файлов, список крупных файлов,
отмена сканирования, подключение несмонтированных разделов только для чтения (Linux).

**Очистка** — перед нажатием кнопки видно, сколько места освободит каждый пункт: кэши программ и
браузеров, корзина, временные файлы, кэш пакетного менеджера (dnf, apt, pacman, zypper, Windows Update,
Homebrew), старые ядра и неиспользуемые пакеты, журнал systemd, дампы падений, среды Flatpak, старые
версии snap, отчёты об ошибках Windows и хранилище компонентов (DISM).

Системные пункты запускаются через **polkit** (Linux) или **UAC** (Windows) — само приложение с правами
администратора не работает. Показываются только те пункты, которые есть в вашей системе.

Язык интерфейса выбирается по локали системы, принудительно: `DISKWIZ_LANG=ru`.

## Установка

```sh
sudo dnf install gtk4-devel libadwaita-devel      # Fedora (apt/pacman — см. README.md)
git clone https://github.com/violetes631/diskwiz && cd diskwiz
make && sudo make install                         # или: make install PREFIX=$HOME/.local
```

Для Windows — архив из раздела Releases, либо сборка через MSYS2 (см. README.md).

## Командная строка

```text
diskwiz                                  запустить GUI
diskwiz scan [ПУТЬ] [--top N] [--threads N]
diskwiz list                             пункты очистки и сколько каждый освободит
diskwiz clean ID... | --all              выполнить очистку
```

## Свои кэши

Файл `~/.config/diskwiz/caches.txt` (в Windows — `%APPDATA%\diskwiz\caches.txt`): по одному пути в
строке. Относительные пути считаются от `~/.cache`, `~/` — домашняя папка. Записи, которые удалили бы
корень ФС или домашнюю папку, игнорируются.

## Лицензия

GPL-3.0-or-later.
