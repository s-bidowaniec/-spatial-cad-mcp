# Spatial CAD MCP dla Gemini

[Review](REVIEW.md) opisuje ustalenia z `main`. Komentarze
`spatial-cad-review[SC-XX]` w źródłach prowadzą do zadań. Repo zawiera
wyłącznie kod, dokumentację i syntetyczne dane testowe.

| Faza | Zakres | Zależności |
|---|---|---|
| [SC-01](tasks/SC-01.md) | Sandbox i bezpieczny zapis nowych artefaktów | — |
| [SC-02](tasks/SC-02.md) | Wspólne 2DM, eksport i kontrola importu w QGIS/MDAL | SC-01 |
| [SC-03](tasks/SC-03.md) | Wiarygodność geometrii 3D, DXF, pomiarów i objętości | SC-01, SC-02 |
| [SC-04](tasks/SC-04.md) | GIS, CRS, GeoJSON i zapytania na dużych warstwach | SC-01 |
| [SC-05](tasks/SC-05.md) | Poprawny strumieniowy odczyt Selafin i kontrakt HDM | SC-01 |
| [SC-06](tasks/SC-06.md) | IFC, spójny cache i budżet odpowiedzi | — |
| [SC-07](tasks/SC-07.md) | Odbiór MCP, konfiguracja klienta i regresje między formatami | SC-01–SC-06 |

SC-01 i SC-06 można zacząć równolegle. Każda paczka zawiera `GEMINI.md`,
`TASK.md`, review i źródła z jednego wskazanego commitu z zachowanym drzewem
ścieżek. Paczki zależne należy przegenerować po integracji wymaganych zmian.

## Handoff

1. Vibe dobiera jedną fazę i sprawdza wskazane miejsca przez
   `rg -n -F 'spatial-cad-review[SC-01]' crates packages shared`.
2. Vibe pakuje zatwierdzone pliki z commitu zawierającego zaakceptowane
   zależności. Ścieżki nowych plików potrzebnych w następnej fazie dodaje
   do `inputs.txt` i zatwierdza razem z bazą.
3. Gemini zwraca patch względem tego commitu, wyniki kryteriów odbioru,
   polecenia i wynik testów. Raport odróżnia próbę w realnym czytniku
   formatu w narzędziu open source, próbę z niezależną biblioteką i test wewnętrzny.
4. Mistral składa osobny PR dla fazy, z ID, bazą i wymaganymi PR-ami.
   Codex integruje przyjęte patche w kolejności zależności.

Packer używa Python 3.11+ i Git. Pakuje jawnie wymienione pliki z drzewa
commitu. Polecenie dla wszystkich faz pomija argument `--phase`.

```bash
python3 tools/pack-spatial-cad-review.py --ref HEAD --phase SC-01 --output /tmp/spatial-cad-packs
```

## Wspólne kryteria

- Utrzymuj istniejące nazwy narzędzi i kompatybilność parametrów. Każdą celową
  zmianę wyniku lub kodu błędu opisz w patchu.
- Utrzymuj kopertę `ok/data/meta` lub `ok/error/meta`, stabilną kolejność
  stron i powtarzalne wyniki z pominięciem czasów wykonania. Pliki robocze
  są niezmienne. Wynik zapisuj do nowego artefaktu i zwracaj ścieżkę względną.
- W testach generuj geometrię i dane modelowe. Uwzględnij jednostki, CRS,
  niepoprawne dane i granice formatu. Do paczki trafiają wyłącznie
  pliki kodu, testów, dokumentacji i konfiguracji przykładowej.
- Do implementacji, CI i walidacji formatów używaj narzędzi open source,
  w tym QGIS/MDAL dla plików 2DM oraz GDAL lub PROJ dla GIS i CRS.
- Używaj testów `cargo test --locked --workspace`, `cargo clippy --locked
  --workspace --all-targets`, `uv run pytest` i obu kontroli Ruff.
  Poprawki formatowania całej bazy wymagają osobnego diffu.
