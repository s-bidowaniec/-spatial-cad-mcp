# IFC i Blender na budowie

Agent powinien powiązać obserwację z elementem i konkretną rewizją modelu.
Przed zmianą użytkownik ma dostać identyfikację obiektu, kierunek i podgląd.
Po zmianie IFC, widok 3D i rzut powinny wskazywać tę samą rewizję.

## Dlaczego przesunięcie komina mogło zmienić dach

Obecny `mcp-ifc` odczytuje podsumowanie, klasy, nazwy, `GlobalId` i psety.
Nie zwraca położenia, obwiedni i relacji potrzebnych do jednoznacznego
wskazania komina. Brakuje też narzędzia edycji z kontrolą różnic. Nakładanie
się brył w Blenderze, wybór rodzica sceny, współdzielone umiejscowienie IFC
i eksport sceny po ponownej konwersji to hipotezy do sprawdzenia w plikach.
Potrzebne są wersje domu sprzed zmiany i po niej.

Dla danego `GlobalId` trzeba odczytać klasę, typ, reprezentację, światową
macierz, jednostkę, kontener i zależności. Komin może wystąpić jako
`IfcChimney`, zestaw części albo proxy. W pliku projektowym należy sprawdzić
również, czy komin i dach mają oddzielne reprezentacje. Jeśli geometrię
połączono w jedną bryłę, potrzebna jest wcześniejsza korekta modelu.

Otwór w połaci, obróbka, przewód i połączenia to osobne zależności. Otwór
powiązany z dachem może pozostać w starym miejscu po przesunięciu komina.
Taka zmiana jest szkicem do oceny. Operacja nie powinna sama dodawać do
zbioru edytowanych elementów dachu ani otworów.

## Wynik próby syntetycznej

W IFC4 w milimetrach dwa obiekty `IfcChimney` miały tę samą nazwę „Komin”.
IfcOpenShell 0.8.5 przesunął wybrany `GlobalId` z nasadą o 0,400 m
wzdłuż osi +X modelu. Po ponownym otwarciu pliku tylko te dwa obiekty
miały nową macierz. Orientacja i siatka dachu, drugi komin, otwór oraz
bryła budynku pozostały takie same. Zapis był do osobnego IFC, a suma
źródła się zgadzała. Kontrola wykryła celowo wprowadzony obrót dachu.
Blender 4.3.2 otworzył przygotowaną geometrię obu stanów z zachowanymi
właściwościami `GlobalId`. Rzut syntetyczny powstał z tych samych danych.

Próba Blendera użyła siatek wygenerowanych przez IfcOpenShell. Bonsai nie
był zainstalowany na hoście, a sprawdzona paczka Bonsai dla Blender 4.2+
używa CPython 3.11, podczas gdy systemowy Blender używa CPython 3.13.
Na właściwym hoście dobierz zgodną parę i przetestuj natywne otwarcie IFC,
wybór po `GlobalId` oraz ponowne otwarcie kandydata w Bonsai.

## Organizacja modelu

| Obszar | Źródło geometrii i identyfikacja | Widoki i kontrole |
|---|---|---|
| Budynek | Natywny IFC. Kontener kondygnacji, klasy produktów, typy, trwałe `GlobalId` dla elementów. | Bonsai/Blender, rzuty kondygnacji i przekroje ze wskazanej rewizji. Otwory i elementy nośne mają własny zakres zmiany. |
| Działka i teren | Zweryfikowane granice i punkty pomiarowe w GeoPackage. Układ poziomy, wysokościowy i transformacja do lokalnych współrzędnych IFC. | QGIS. Podkład działki i mapa sytuacyjna. Fotografia może wskazać miejsce obserwacji, a granice pochodzą z danych pomiarowych. |
| Ogrodzenie, bramy i mała architektura | Linie przebiegu i obiekty w danych lokalnych, następnie modele IFC z trwałymi identyfikatorami. Klasy zależą od schematu i funkcji. | Rzut sytuacyjny, odległości od granic, osie słupków i prześwity. |
| Kanalizacja i deszczówka | Odcinki, kształtki, studzienki i jawne połączenia. Rzędne dna, średnica, spadek, kierunek przepływu i źródło pomiaru. | Profil podłużny, schemat sieci, kolizje i odcinki bez danych. Niewidoczne trasy pozostają oznaczone jako niezweryfikowane. |
| Instalacje elektryczne | Odcinki, osprzęt i przynależność do systemów lub obwodów. Rozróżnij trasę kabla od rury osłonowej. | Rzut, schemat, ciągłość połączeń i kolizje z pozostałymi instalacjami. |

Georeferencja IFC4 może używać `IfcProjectedCRS` i `IfcMapConversion`.
Przed łączeniem działki z budynkiem sprawdź EPSG, skale, osie, punkt
odniesienia wysokości i zgodność co najmniej na punktach kontrolnych.
Położenie w scenie pomocniczej Blendera jest oddzielone od współrzędnych
IFC służących do zapisu.

## Przepływ ze zdjęcia lub głosu do IFC i planu

1. Z telefonu wysyłasz zdjęcia i krótką notatkę głosową z identyfikatorem
   projektu lub miejsca. Mostek może dostarczyć media. Wpis budowy trzyma
   oryginały prywatnie, transkrypt, datę, autora i referencję do rewizji.
2. Agent wyłuskuje przedmiot, stan i wymiary. Zaznacza, czy wymiar pochodzi
   z pomiaru, dokumentacji czy oszacowania. Ze zdjęcia może przygotować
   oznaczenie i kandydatów. Przy nieznanym punkcie odniesienia prosi o
   konkret, np. oś budynku, wymiar z miarki lub drugi kadr.
3. Po wskazaniu elementu po `GlobalId` agent przygotowuje zmianę i podgląd
   w 3D oraz na rzucie. Zapis wynikowy zawiera listę zmienionych elementów
   i ostrzeżenia o kolizjach lub osieroconych zależnościach.
4. Projekt i pomiar stanu wykonanego zachowują osobne rewizje. Korekty
   dotyczące konstrukcji, drogi spalin i instalacji mogą otrzymać podgląd
   od razu. Przyjęcie ich jako podstawy wykonania wymaga właściwej
   weryfikacji branżowej. Zatwierdzona rewizja daje IFC i rzuty
   z tym samym ID i metryką pochodzenia.

Początek wdrożenia to lokalizacja i przestawienie jednego komina z
kontrolą dachu i otworu. Następnie ten sam mechanizm rozszerzamy o
zgłoszenia terenowe, zasilanie warstwami działki i odcinki instalacji.

Źródła: [IfcOpenShell edit_object_placement](https://docs.ifcopenshell.org/autoapi/ifcopenshell/api/geometry/edit_object_placement/index.html) · [Bonsai i IFC](https://docs.bonsaibim.org/) · [manifest Bonsai 0.8.0](https://github.com/IfcOpenShell/IfcOpenShell/blob/v0.8.0/src/bonsai/bonsai/blender_manifest.toml) · [QGIS](https://qgis.org/)
