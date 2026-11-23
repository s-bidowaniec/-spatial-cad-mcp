# Review spatial-cad-mcp

Zakres to `main` w `s-bidowaniec/spatial-cad-mcp`, commit
`116e562d6bdbfeac46d5c58f84bb942085c8b4fd`. Przejrzałem pięć serwerów MCP,
wspólne biblioteki, `cadctl`, konfigurację klienta i testy. Próbki były
syntetyczne. Zdalny ref PR #2 wskazywał starszy, rozbieżny commit
`b5c3658` z poprawkami CI. GitHub API zwróciło 403 przy odczycie listy
otwartych PR-ów i issues.

Cel wdrożenia to powtarzalne narzędzia dla agentów pracujących na IFC, DXF,
GIS, HDM i siatkach. Agent powinien dostać krótki, jednoznaczny wynik, móc
sprawdzić jego pochodzenie i otworzyć zapisany artefakt w docelowym programie.

| Priorytet | Ustalenie | Sposób potwierdzenia | Zadanie |
|---|---|---|---|
| P1 | Granica zapisu jest sprawdzana przy budowie ścieżki, a eksportery zapisują do niej niezależnie. `outputs` nie jest wiązany z katalogiem roboczym, a sprawdzenie istnienia nie gwarantuje wyłączności zapisu przy dwóch eksportach. Obie implementacje sandboxu mają ten rozdział. | Analiza `resolve_output` w Rust i Pythonie oraz zapisów w czterech serwerach. | [SC-01](tasks/SC-01.md) |
| P1 | `cadctl inspect` wywołuje parser bez kontroli workspace, chociaż dokumentacja obiecuje ją dla każdego polecenia. Rust przy błędnym rozwiązaniu istniejącego prefiksu może wrócić do niesprawdzonej ścieżki; Python przyjmuje inną składnię dla części ścieżek. | Analiza `cadctl::main`, `cad-sandbox::checked` i Pythonowego `resolve`. | [SC-01](tasks/SC-01.md) |
| P1 | Puste przecięcie bbox zapisuje sam `MESH2D` i zgłasza sukces. Własny `mesh_summary` odrzuca ten plik. | Wywołania stdio `mesh_slice_bbox` i ponowny `mesh_summary` na małym trójkącie. | [SC-02](tasks/SC-02.md) |
| P1 | Mesh i HDM inaczej liczą te same karty 2DM. Część uszkodzonych rekordów jest cicho pomijana, a identyfikatory węzłów nie mają sprawdzenia unikalności. Eksportery zapisują głównie węzły, ściany i materiał, więc karty hydrodynamiczne i identyfikatory źródłowe wymagają jawnej polityki. | Po dopisaniu `ND bad 4 5 6` mesh zgłosił 3 węzły, HDM 4 i inny zasięg. Zakres eksportu wynika z obu writerów. | [SC-02](tasks/SC-02.md) |
| P1 | Algorytm objętości liczy nakładające się ściany wielokrotnie. Wynik ma sens dla poprawnego TIN 2,5D z uzgodnionym układem i jednostkami. Dwie pętle po trójkątach sprawdzają każdą parę. | Trójkąt o polu 0,5 z różnicą Z równą 1 dał 0,5 objętości. Duplikat tej samej ściany podniósł wynik do 1,0. Koszt par wynika z kodu. | [SC-03](tasks/SC-03.md) |
| P1 | `mesh_diff` używa kubełków zamiast rzeczywistego dystansu. Bliskie węzły po dwóch stronach granicy kubełka są rozłączne, a dwa węzły w jednym kubełku mogą zająć ten sam klucz. | Przesunięcie X z 0,049 do 0,051 przy tolerancji 0,1 zwróciło jedną brakującą pozycję z każdej strony. | [SC-03](tasks/SC-03.md) |
| P2 | `mesh_cut_plane` wybiera pierwszy pasujący wariant płaszczyzny przy sprzecznych argumentach. `dxf_labels_to_3d` mierzy dystans do segmentu, a rzędną przypisuje do najbliższego wierzchołka, co przesuwa punkt kotwiczenia wzdłuż linii. | Analiza wyboru parametrów i przypisywania kotwic. | [SC-03](tasks/SC-03.md) |
| P1 | `dxf_export_slice` opisuje eksport jako bezstratny, ale odtwarza tylko uproszczone sekcje i nie przenosi zależności `BLOCKS` dla `INSERT`. Bbox jest filtrem obwiedni. | Analiza par w `export_dxf_slice` i emitowanych sekcji. | [SC-03](tasks/SC-03.md) |
| P1 | Eksport GeoJSON z GeoPackage zapisuje współrzędne źródłowego CRS bez transformacji, po czym podsumowanie GeoJSON oznacza wynik jako WGS 84. Dodatkowo identyfikator SRS z bazy bywa przedstawiany jako kod EPSG bez potwierdzenia organizacji. | Syntetyczny GPKG EPSG:25833 zwrócił sukces i punkt GeoJSON `[400000,5600000]`. Przypisanie EPSG potwierdza kod. | [SC-04](tasks/SC-04.md) |
| P2 | GeoPackage ładuje kandydatów i atrybuty przed paginacją. Błędy dekodowania często trafiają do pustej geometrii, a błąd odczytu wiersza może zakończyć iterację bez informacji w odpowiedzi. | Analiza odczytu, kolekcjonowania i dopiero późniejszej paginacji w `dataset.rs` / `server.rs`. | [SC-04](tasks/SC-04.md) |
| P1 | Niekompletny krok Selafin może zostać pominięty, a szereg czasowy zwrócony jako pełny. Długości i wartości nagłówka trzeba zweryfikować przed alokacją i indeksowaniem. | Poprawny pierwszy krok i drugi krok z rekordem czasu bez rekordu zmiennej. MCP zwróciło `ok=true`, jeden punkt i `has_more=false`. | [SC-05](tasks/SC-05.md) |
| P2 | Selafin jest za każdym razem wczytywany w całości, a szeregi są zbierane przed paginacją. Pozostają do uzgodnienia granice dla wielowarstwowego Selafin i dopasowania nazw zmiennych. | Analiza `summarize_selafin`, `query_selafin_series` i `query_selafin_profile`. | [SC-05](tasks/SC-05.md) |
| P2 | IFC pobiera model dwukrotnie przy równoległym chybieniu. Wpis znaleziony w cache może zostać usunięty między odczytem i przesunięciem na koniec LRU. Mtime i rozmiar same nie rozpoznają podmiany pliku. | Analiza zakresów blokady i klucza walidacji w `IfcModelCache.load`. | [SC-06](tasks/SC-06.md) |
| P2 | IFC materializuje pełną listę opisów przed stronicowaniem. Pole `name_pattern` działa bez określonego budżetu wyszukiwania, a liczba psetów w jednej odpowiedzi jest nieograniczona. | Analiza `ifc_query_elements`, `ifc_get_psets` i `ifc_validate_required_props`. | [SC-06](tasks/SC-06.md) |
| P2 | Istnieją testy funkcji i wrapperów serwerów. Brakuje wspólnego testu `initialize → tools/list → tools/call`, zgodności kopert Python/Rust i ponownego otwarcia wybranych wyników przez niezależne narzędzia. | Analiza testów i konfiguracji CI. | [SC-07](tasks/SC-07.md) |
| P2 | Obietnice o niezmienności wejść, zapisie tylko do outputs i obsługiwanych formatach wymagają dopasowania do wdrożonego kontraktu. Przykład MCP używa lokalizacji źródeł, co wymaga osobnej ścieżki dla zainstalowanych serwerów. | Porównanie `docs/security.md`, `README.md`, `examples/mcp.json` i implementacji. | [SC-01](tasks/SC-01.md), [SC-07](tasks/SC-07.md) |

## Wyniki kontroli

- Z czystego katalogu kompilacji przeszły 26 testów Rust, `cargo check`
  i Clippy. `cargo fmt --check` wskazało różnice na bazowym `main`.
- Przeszło 57 testów Python i obie kontrole Ruff. Test wadliwego IFC
  zgłasza istniejące ostrzeżenie destruktora IfcOpenShell.
- QGIS 3.40.6 z MDAL odczytał syntetyczny trójkąt 2DM jako 3 węzły
  i 1 element. Dla samej karty `MESH2D` zgłosił poprawną warstwę z
  0 węzłów i 0 elementów. Dlatego próba odbioru musi sprawdzać liczniki
  i geometrię obok statusu otwarcia.
- Import w Aquaveo SMS pozostaje do sprawdzenia. Karta `MESH2D` jest
  częścią formatu. Profil eksportu bez komentarzy w SC-02 wymaga
  osobnego odbioru w docelowej wersji SMS.
