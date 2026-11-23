# Pakiet narzędzi IFC dla agentów

Agent przechodzi od wskazania obiektu i wymiarów do podglądu, kontroli
kolizji i nowej rewizji IFC. Ten sam wynik służy do wyboru elementu
w Blenderze/Bonsai, generowania rzutu i raportu dla wykonawcy.
Pakiet rozwija fazy SC-08 i SC-09. Oddziela odczyty, operacje edycji
oraz publikację kandydata, także wtedy, gdy zmiana obejmuje kilka branż.

## Katalog funkcji

| Funkcja | Proponowany interfejs MCP | Pierwsze obsługiwane przypadki |
|---|---|---|
| Lokalizacja, pomiary i relacje | `ifc_find_elements`, `ifc_inspect_geometry`, `ifc_measure` | GlobalId, klasa, typ, kondygnacja, osie, wymiary, bbox, odległości i połączenia. Lokalne i modelowe współrzędne w metrach. |
| Kolizje i prześwity | `ifc_check_clashes` | Przecięcie brył, kontakt, zbyt mały prześwit; filtry branż, klas, kondygnacji i zaznaczenia. Sprawdzenie zmiany względem bazy. |
| Kontrola otworów i jakości bryły | `ifc_check_openings`, `ifc_check_geometry` | Kontrola gospodarza, wypełnienia i trafienia w płaszczyznę. Brak otworu, otwór w starym miejscu, wycięcie utrwalone w bryle, błędy topologii i brak reprezentacji. |
| Nanoszenie i przekształcenia | `ifc_plan_changes` z typami `create`, `translate`, `rotate`, `copy`, `remove`, `update_properties` | Ściana, słup, belka i prosty element zagospodarowania. Obrót wokół jawnego punktu i osi. Nowe wystąpienia z odrębnym GlobalId. |
| Zmiana wymiarów | `ifc_plan_changes` z typami `set_length`, `set_height`, `set_section`, `trim`, `split`, `join` | Proste ściany i profile. Ustalany koniec zakotwiczony, zmiana lokalnej osi i zgodność przekroju. Inne reprezentacje mają status obsługi per typ. |
| Edycja otworów | `ifc_plan_changes` z typami `add_opening`, `move_opening`, `resize_opening`, `remove_opening`, `fill_opening` | Przejście rury, drzwi lub okna, wycięcie pod komin. Osobna kontrola bryły bazowej i geometrii po odjęciu otworów. |
| Instalacje | `ifc_trace_system` oraz operacje `create_segment`, `connect_ports`, `edit_run` | Odcinki kanalizacji i elektryki, kształtki, złączki i porty. Punkty końcowe, średnica lub przekrój, rzędna dna i spadek. |
| Uwaga i przekazanie do wykonania | `ifc_create_issue`, `ifc_preview_changes`, `ifc_write_candidate`, `ifc_compare_revisions` | Znacznik obserwacji, zrzut lub rzut, lista zmienionych identyfikatorów, raport QA i IFC kandydata. Problemy mogą być eksportowane do BCF. |

Nazwy są kontraktem roboczym dla Gemini. Uzgodnij końcowe nazwy i istniejące
koperty MCP przed wdrożeniem. Obsługa wymaga testów IFC4, a pozostałe
schematy i reprezentacje raportują zweryfikowane zdolności. Funkcje analizy
i generowania IFC działają z IfcOpenShell, a interaktywny widok korzysta
z Blender/Bonsai. Weryfikuj zgodność wersji i używaj otwartych narzędzi.

## Jedna operacja na jeden uzgodniony zakres

- Każda zmiana określa rewizję wejściową, `GlobalId` lub nowy element,
  układ i jednostkę, listę zależności oraz wynik, który da się porównać.
  Dla obrotu podaj oś, kąt i punkt obrotu. Dla wydłużania wskaż koniec,
  który ma pozostać w miejscu. Dla otworu podaj gospodarza i wypełnienie.
- Przed publikacją kontroluj orientację, geometrię, właściwości i relacje
  po ponownym odczycie IFC. Zmiana reprezentacji współdzielonej wymaga
  wydzielenia reprezentacji edytowanego wystąpienia. Dla świadomej zmiany
  całego typu podaj wszystkie wystąpienia objęte planem.
- Do raportowania kolizji używaj przekształconych brył z uwzględnieniem
  otworów. Obwiednia wybiera kandydatów. Dopiero test geometrii określa
  przecięcie lub odległość. Raport rozróżnia wynik potwierdzony,
  kandydata do sprawdzenia i brak geometrii. Uwzględnia tolerancję.
- Jeśli projekt dotyczy wielu plików, tożsamość obejmuje źródło, rewizję
  i GlobalId. Ustal wspólny układ i jednostki przed porównaniem geometrii.
  Wyjątki dla połączeń i znanych kontaktów zapisz jako jawne pary lub reguły.
- Zgłoszenie głosowe lub zdjęcie z nieznaną skalą może utworzyć uwagę,
  kandydatów i podgląd. Wymiary i położenie wiąż z pomiarem, osią albo
  innym ustalonym odniesieniem. Rzut i widok 3D muszą używać tej samej
  rewizji co IFC.

## Przykład z budowy

„Dodaj przejście dla rury, przesuń oś przewodu o 12 cm i przedłuż odcinek
do studzienki”. Agent wskazuje obiekty, gospodarza otworu i punkty końcowe,
pokazuje plan zmiany z wymiarami, a potem zapisuje kopię IFC. Porównanie
kontroluje, czy przejście faktycznie usuwa kolizję z gospodarzem,
czy przewód utrzymuje zadany spadek oraz czy końcówka jest połączona
i zachowuje prześwit od innych instalacji. W razie rozbieżności wynik
pokazuje konflikt przypięty do rewizji i GlobalId.

## Zweryfikowane próby

`uv run --no-sync python tools/probe-ifc-operations.py` uruchamia próbę
na tymczasowym IFC4 z IfcOpenShell 0.8.5.

- Przecięcie ściany jest wykrywane. Przejście przez otwór przechodzi
  obok materiału, a usunięcie otworu przywraca kolizję. Sprawdzono również
  geometrię gospodarza z wyłączonym odjęciem otworów.
- Rozdzielono styk od przecięcia oraz zmierzono prześwit 40 mm.
  Wynik API jednoznacznie zależy od trybu. Przeprowadzaj test styku
  osobno przed interpretacją odległości.
- Ściana po wydłużeniu z 4 do 5 m i obrocie o 90° wokół punktu
  początkowego zachowała przekrój, wysokość i GlobalId. Geometria
  drugiej ściany współdzielącej wcześniej reprezentację się zgadza.
  Wymiary i macierz sprawdzono po ponownym otwarciu zapisanego IFC.
- [Próba komina](IFC-BLENDER.md) sprawdza dodatkowo otwór po ruchu,
  położenie dachu i raport z próby nieoczekiwanego obrotu.

To próby podstawowych operacji IfcOpenShell. Macierz wsparcia,
narzędzia MCP, typowe przypadki wielobranżowe i odbiór w Bonsai
są przedmiotem etapów SC-08, SC-09, SC-11–SC-14.
