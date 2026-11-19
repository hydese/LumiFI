<div align="center">

# LumiFi

**Un music player piccolo, leggero e in alta qualità per macOS e Windows.**<br />
La tua musica, come è stata registrata.

[![CI](https://github.com/hydese/lumifi/actions/workflows/ci.yml/badge.svg)](https://github.com/hydese/lumifi/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/license-GPL%20v3-3d6bf0)](LICENSE)

</div>

> [!NOTE]
> LumiFi è all'inizio. Oggi c'è il **mini player**: suona la tua musica locale, con copertina,
> coda, casuale, ripeti, volume e due visualizzazioni. La libreria completa, l'equalizzatore e
> Spotify arrivano nelle prossime versioni.

## L'idea

- 🎧 **Alta qualità.** FLAC, ALAC, WAV, AIFF, MP3, AAC, Vorbis e Opus, anche hi-res. Il
  dispositivo audio passa alla frequenza del brano, così il suono arriva al DAC senza ritocchi.
- 🪶 **Leggero.** Scritto in Rust, poca memoria e avvio istantaneo.
- 🟦 **Piccolo quando serve.** Un mini player che sta in un angolo dello schermo, come il
  vecchio Winamp, e si apre in una libreria completa quando vuoi cercare e organizzare.
- 🎚️ **Equalizzatore vero.** Dieci bande per tutti, curva parametrica e profili AutoEq per chi
  vuole di più.
- 🔌 **Servizi a scelta.** Spotify, Apple Music e gli altri si attivano dalle impostazioni; se
  non li accendi, non li vedi.

## Provare LumiFi

Scarica `LumiFi.app` (macOS) o `LumiFi.exe` (Windows) dalla pagina
[Releases](https://github.com/hydese/LumiFI/releases), oppure compilalo (serve
[Rust](https://rustup.rs)):

```sh
cargo run --release -p lumifi                      # finestra vuota: trascinaci la musica
cargo run --release -p lumifi -- ~/Music/Album     # parte subito con una cartella o dei file
```

> [!TIP]
> **macOS:** l'app non è firmata da Apple. La prima volta aprila con tasto destro › Apri,
> oppure esegui `xattr -dr com.apple.quarantine LumiFi.app`.

| Tasto | Cosa fa |
| --- | --- |
| Spazio | Riproduci / pausa |
| → / ← | Brano successivo / precedente |
| Maiusc + → / ← | Avanti / indietro di 10 secondi |
| ↑ / ↓ e rotella | Volume |
| M | Silenzia |
| S / R | Casuale / ripeti (tutto, un brano) |
| V | Mini player: avanzamento A (barra) o B (forma d'onda) |
| T | Mini player sempre in primo piano |
| ⌘L / Ctrl+L | Libreria ⇄ mini player |
| ⌘, / Ctrl+, | Impostazioni |
| ⌘O / Ctrl+O | Apri file o cartelle |

LumiFi si apre sulla **libreria**: album, artisti e brani, con la barra di riproduzione in
basso. Il pulsante **Mini player** in alto (o Ctrl+L) la sostituisce con il mini player, a
**striscia** o **quadrato** (si sceglie in Impostazioni › Aspetto). Il mini player si sposta
trascinandolo da un punto libero, si ridimensiona dal bordo mantenendo le proporzioni, e la
puntina lo tiene sopra le altre finestre. La musica non si ferma quando cambi finestra, e
all'avvio LumiFi riprende dall'ultimo brano, in pausa.

Un clic sul logo LumiFi (o sull'ingranaggio) apre le **Impostazioni**: Generale, Audio (anche
la scelta dell'uscita), Libreria, Equalizzatore, Servizi, Aspetto, Informazioni. Quello che non
c'è ancora è segnato "In arrivo".

La libreria legge la cartella Musica del sistema; altre cartelle si aggiungono dalle
Impostazioni. LumiFi tiene le sue cose in:

| | Windows | macOS |
| --- | --- | --- |
| Impostazioni | `%APPDATA%\LumiFi\settings.json` | `~/Library/Application Support/LumiFi/` |
| Libreria | `%LOCALAPPDATA%\LumiFi\library.db` | `~/Library/Application Support/LumiFi/` |
| Copertine | `%LOCALAPPDATA%\LumiFi\covers` | `~/Library/Caches/LumiFi/covers` |

### Il motore da riga di comando

```sh
cargo run --release -p lumifi-cli -- info  "brano.flac"
cargo run --release -p lumifi-cli -- play  "brano.flac" --volume 60
cargo run --release -p lumifi-cli -- devices
```

## Struttura

| Cartella | Contenuto |
| --- | --- |
| `crates/app` | l'app LumiFi (interfaccia in [gpui](https://gpui.rs)) |
| `crates/engine` | il motore audio: decodifica (Symphonia), uscita (cpal), forma d'onda |
| `crates/cli` | `lumifi-cli`, per provare il motore da terminale |
| `packaging/` | come si impacchetta l'app per macOS |
| `patches/` | dipendenze corrette in locale (vedi il README lì dentro) |

## Roadmap

1. **v0.1 — Fondamenta:** motore, CI e release per macOS e Windows.
2. **v0.2 — Mini player:** musica locale, coda, viste A/B.
3. **v0.3 — Libreria e interfaccia:** app completa, scansione delle cartelle, album e artisti,
   impostazioni *(ora)*; poi playlist, ricerca, testi sincronizzati.
4. **v0.6 — Qualità ed EQ:** modalità esclusiva, gapless, ReplayGain, equalizzatore e AutoEq.
5. **v1.0 — Servizi:** Spotify, telecomando di Music.app, Last.fm, aggiornamenti automatici.

## Licenza

LumiFi è software libero, distribuito con la [GNU General Public License v3](LICENSE) o
successiva.

LumiFi non è affiliato a Spotify, Apple o Google.

Dedicato a mio babbo, Giuliano.
