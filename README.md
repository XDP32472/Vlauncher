# Vlauncher

Open-source launcher Minecrafta skupiony na profilach, modach i mierzalnej wydajności.

## Stan projektu

Aplikacja zawiera nowoczesny interfejs desktopowy, profile Vanilla/Fabric, wyszukiwanie/instalację/podgląd modów z Modrinth oraz pobieranie bazowych plików wybranej wersji Minecrafta (klient, biblioteki, natywne pliki Windows i zasoby). Pliki są weryfikowane przez SHA-512 lub SHA-1 i przechowywane w katalogu użytkownika.

Launcher instaluje najnowszy stabilny Fabric dla profilu, pobiera i weryfikuje jego biblioteki oraz przygotowuje zgodny Java Runtime Temurin w katalogu użytkownika. Logowanie Microsoft działa przez kod urządzenia i wymaga publicznego identyfikatora aplikacji Microsoft; token odświeżania jest przechowywany w Windows Credential Manager. Rzeczywiste uruchamianie gry jest jeszcze w budowie. Benchmark FPS ma [protokół v1](docs/benchmark-protocol-v1.md), lecz zbieranie rzeczywistych pomiarów jeszcze nie działa. Launcher nie pokazuje fikcyjnych wyników. Wymagane zależności modów nie są instalowane automatycznie.

Projekt jest niezależny od Mojang i Microsoft. Licencja: MIT.

## Budowanie

Wymagany jest Rust stable i zależności systemowe Tauri 2 dla danego systemu.

```sh
cargo install tauri-cli --version "^2"
cargo tauri dev
cargo tauri build
```

Dane launchera są przechowywane w katalogu użytkownika; instalacja systemowa i uprawnienia administratora nie są wymagane.
