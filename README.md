# Vlauncher

Open-source launcher Minecrafta skupiony na profilach, modach i mierzalnej wydajności.

## Stan projektu

Aplikacja zawiera nowoczesny interfejs desktopowy, profile Vanilla/Fabric oraz wyszukiwanie, instalację i podgląd modów z Modrinth w profilach Fabric. Pobrane pliki są sprawdzane przez SHA-512 i przechowywane w katalogu użytkownika.

Instalowanie plików gry Minecraft, Java runtime, logowanie Microsoft i uruchamianie gry są w budowie. Benchmark FPS ma [protokół v1](docs/benchmark-protocol-v1.md), lecz zbieranie rzeczywistych pomiarów jeszcze nie działa. Launcher nie pokazuje fikcyjnych wyników. Wymagane zależności modów nie są instalowane automatycznie.

Projekt jest niezależny od Mojang i Microsoft. Licencja: MIT.

## Budowanie

Wymagany jest Rust stable i zależności systemowe Tauri 2 dla danego systemu.

```sh
cargo install tauri-cli --version "^2"
cargo tauri dev
cargo tauri build
```

Dane launchera są przechowywane w katalogu użytkownika; instalacja systemowa i uprawnienia administratora nie są wymagane.
