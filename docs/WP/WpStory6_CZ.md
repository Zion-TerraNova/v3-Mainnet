# ZION
## Tři proudy jedné řeky — od Genesis k Trinity

*Kanonický dodatek k příběhovému whitepaperu · v3.0.1 → v3.0.6-beta · červenec 2026*

> *Strom neroste tak, že jednou vyrazí a pak čeká. Roste v letokruzích —*
> *rok po roce, oprava po opravě, plod po plodu.*
> *Tento dodatek je zápis toho, co se do kůry stromu vrylo od jeho zasazení*
> *až po nejnovější, nejmladší větev: Trinity.*

---

## Jak číst tento dodatek

Tento text nenahrazuje hlavní vyprávěcí whitepaper (`WpStory.md` a jeho další
verze). Je to jeho **kronika růstu** — shrnutí, co se stalo mezi verzemi
**3.0.1** a **3.0.6-beta**, a vysvětlení nejnovější větve stromu: **Triple
Stream**, **Zion Grow** a **Zion Liquidity**.

Platí zde stejné pravidlo jako v celém příběhu ZIONu: co je **ŽIVÉ**, je
ověřitelné na běžící síti. Co je **BONUS**, je funkce, kterou dostáváš navíc,
aniž bys o ni musel prosit nebo ji nastavovat. Co je **HORIZONT**, je směr,
kterým se strom natahuje, ale ještě ho nedosáhl.

---

# I. Kronika letokruhů — cesta od semene k Trinity

Než přejdeme k nejnovější větvi, podívejme se, jak strom rostl.

| Verze | Jméno | Co přinesla | Stav |
|---|---|---|---|
| **v3.0.1** | Zasazení | První mainnetový kořen: Rust L1, Ekam Deeksha jádro, Fair Launch bez ICO, první vytěžené bloky | ŽIVÉ (historie) |
| **v3.0.3** | Desetinný řez | Přechod na jednotku 1 ZION = 1 000 000 flowers, sjednocení RPC měřítka | ŽIVÉ |
| **v3.0.4** | Noc hada a nový kořen | Bezpečnostní incident zveřejněn a opraven, hard genesis reset, nová ústavní kotva, první DeFi mosty (wZION na šesti EVM sítích, staking, farming, DAO) | ŽIVÉ |
| **v3.0.5** | Všechno zelené | Mainnet Beta stabilizace, veřejné vydání komunitního CLI (`zion`), 12/12 služeb aktivních, whitepaper kanonizován | ŽIVÉ |
| **v3.0.6-beta** | Tři proudy jedné řeky | **Trinity** těžební jádro — Zion Grow, Zion Liquidity | ŽIVÉ (Beta) |

Každý řádek této tabulky je letokruh. Žádný z nich nevznikl beze práce a
žádný nevznikl beze zkoušky. Nejtěžší zkouška — bezpečnostní incident v roce
2026 — je už popsána v hlavním vyprávěcím whitepaperu (kapitola o hadovi
v zahradě): síť chybu zveřejnila, spálila napadené dřevo až ke kořeni a
zasadila znovu, se stejným semenem, ale tvrdší kůrou. Tento dodatek na to
nenavazuje omluvou, ale výsledkem: ze spáleného kořene vyrostly během
následujících tří letokruhů (3.0.4 → 3.0.6) mosty, veřejný miner a nakonec
i nejchytřejší větev, jakou strom dosud vypustil.

---

# II. Nová větev — Tři proudy jedné řeky

## 1. Co strom dělal doteď

Od prvního bloku platí jednoduché pravidlo: kdo chce ZION, musí ho vytěžit
nebo ho získat od někoho, kdo ho vytěžil. Těžař zapojí GPU nebo CPU, stroj
řeší Ekam Deeksha rovnici, síť ověří práci a odmění těžaře přímo v ZIONu.

Tohle pravidlo se nemění. Je to kořen a kořen se nehýbe.

## 2. Co je nové ve v3.0.6

Těžařský software ZION (od verze **v3.0.6-beta**) v sobě nese novou
schopnost: **Trinity**.

> **Mine ZION. Earn ZION. Grow ZION.**

Trinity je chytrý těžební engine zabudovaný přímo do oficiálního
ZION mineru. Zapíná se automaticky — není potřeba žádné nastavení,
žádný druhý wallet, žádná druhá aplikace. Jeho úkolem je jediná věc:
**vytěžit z tvého hardware víc hodnoty, než dokáže samotné jádro Ekam
Deeksha, a celou tuto hodnotu ti vyplatit v ZIONu.**

Zjednodušeně: tvůj počítač (GPU i CPU) pracuje na plný výkon, síť interně
rozloží práci tak, aby nezůstal nevyužitý žádný takt, a výsledek dostaneš
jako jednu jedinou položku ve tvé peněžence — **ZION, který roste**.

Jak přesně engine rozděluje práci mezi GPU a CPU je součástí proprietární
architektury ZION mineru (podobně jako je uzavřený kód mnoha optimalizovaných
těžebních jader v jiných sítích). Zdrojový kód jádra a poolu ZION zůstává
plně otevřený pod MIT licencí — Trinity engine je bonusová vrstva nad
ním, dostupná zdarma každému, kdo používá oficiální ZION miner.

## 3. Zion Grow — tvoje pozice, která neustále roste

**Zion Grow** je jméno pro to, co se stane tvé peněžence, když těžíš
s Trinity engine:

- Každý blok, na kterém se tvůj hardware podílí, zvyšuje tvůj ZION zůstatek.
- Nemusíš nic prodávat, nic směňovat, nic sledovat na burze.
- Čím déle těžíš, tím víc ZION držíš — pozice se kumuluje sama, jen tím,
  že necháš miner běžet.

Žádný slib o zisku v tom není. Je v tom jen jednoduchý mechanismus: práce,
kterou tvůj hardware odvede, se beze zbytku promítne do tvého zůstatku
v jedné měně, ne v pěti různých tokenech, které bys musel sám prodávat.

## 4. Zion Liquidity — proč to posiluje celou síť

Většina těžařských sítí má stejný problém: těžař vytěží minci, prodá ji na
burze, aby zaplatil elektřinu, a tím tlačí cenu dolů. Čím víc lidí těží, tím
větší je tlak na prodej.

**Zion Liquidity** tento vzorec obrací:

- Trinity engine interně přemění veškerou vytěženou hodnotu na ZION
  ještě dřív, než se cokoli dostane na burzu.
- Těžař nikdy nemusí nic prodávat — dostává přímo ZION.
- Výsledkem je, že těžba **nevytváří prodejní tlak** na cenu ZIONu. Naopak —
  s každým dalším hashem roste hloubka likvidity, ze které síť čerpá.

To je obrácený vzorec vůči starému těžařskému modelu, který jsme popsali
v kapitole o Kvantové revoluci hlavního whitepaperu: řeka, která tekla do
kopce, teď teče dolů, k těm, kdo pracují.

## 5. Proč je to bonus, ne podmínka

Trinity nic nevyžaduje navíc. Není to nový poplatek, není to nová
registrace, není to nutnost cokoliv nastavovat. Je to vlastnost oficiálního
ZION mineru v3.0.6 a novějšího — pokud těžíš pomocí něj, bonus dostáváš
automaticky. Pokud používáš starší verzi nebo vlastní řešení, těžíš pořád
podle původního, plně otevřeného pravidla Ekam Deeksha — jen bez tohoto
zrychlení.

---

# III. Kotevní fakta (ověřitelná)

| Vlastnost | Hodnota |
|---|---|
| Protokol | `zion-v3-node/3.0.6` |
| Genesis hash | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Celková nabídka | 144 000 000 000 ZION |
| Rozdělení odměny | 89 % miner / 5 % humanitární fond / 5 % fond Issobella / 1 % spáleno |
| Blok | ~60 sekund |
| Stav sítě | Mainnet Beta, veřejný launch cíl 31. 12. 2026 |
| Oficiální miner | ZION v3.0.6-beta — Trinity engine, Zion Grow, Zion Liquidity |
| Licence jádra a poolu | MIT, otevřený zdrojový kód |
| Zdrojový kód | https://github.com/Zion-TerraNova/v3-Mainnet |
| Pool | `62.171.141.136:8444` |
| Web | https://zionterranova.com |

---

# IV. Cesta dál — co strom teprve vyžene

- **v3.0.7 (blízký horizont):** Zion Grow dashboard — uvidíš přímo v aplikaci,
  jak tvoje ZION pozice roste v čase.
- **v3.0.8 (blízký horizont):** Zion Liquidity metriky — uvidíš, jak tvoje
  těžba přispívá k hloubce likvidity celé sítě.
- **v3.1.0 (horizont):** veřejný launch, širší dostupnost Trinity
  enginu na dalších platformách.

Nic z toho není slib termínu. Je to směr, kterým se větev natahuje — stejný
princip, jaký platí pro celý Strom života ZIONu: nejdřív práce, pak plod.

---

*ZION TerraNova · MIT Licence pro jádro a pool · Trinity je bonusová vrstva oficiálního mineru*
*Gate, Gate, Paragate, Parasamgate, Bodhi Svaha.*
