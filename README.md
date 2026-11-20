<div align="center">

# LumiFi

**Un music player piccolo, leggero e in alta qualità per macOS e Windows.**<br />
Un music player piccolo e leggero, con audio lossless in alta qualità.

[![CI](https://github.com/hydese/lumifi/actions/workflows/ci.yml/badge.svg)](https://github.com/hydese/lumifi/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/license-GPL%20v3-3d6bf0)](LICENSE)

</div>

> [!NOTE]
> LumiFi è in sviluppo. Oggi ci sono la libreria, le playlist, la ricerca, i testi, il mini
> player, l'equalizzatore a 10 bande e la riproduzione senza pause. Spotify arriva con la 1.0.

## L'idea

- 🎧 **Alta qualità.** FLAC, ALAC, WAV, AIFF, MP3, AAC, Vorbis e Opus, anche hi-res. Il
  dispositivo audio passa alla frequenza del brano, così il suono arriva al DAC senza ritocchi;
  se non può, un ricampionatore di alta qualità fa la conversione. Niente pause tra un brano e
  l'altro (gapless) e volume uniforme con ReplayGain, se lo vuoi.
- 🪶 **Leggero.** Scritto in Rust, poca memoria e avvio istantaneo.
- 🟦 **Piccolo quando serve.** Un mini player che sta in un angolo dello schermo, come il
  vecchio Winamp, e si apre in una libreria completa quando vuoi cercare e organizzare.
- 🎚️ **Equalizzatore vero.** Dieci bande con preamplificazione automatica (niente clipping),
  salvato per ogni uscita audio; la curva parametrica e i profili AutoEq arrivano dopo.
- 🔌 **Servizi a scelta.** Spotify, Apple Music e gli altri si attivano dalle impostazioni; se
  non li accendi, non li vedi.

## Provare LumiFi

Scarica `LumiFi.app` (macOS) o `LumiFi.exe` (Windows) dalla pagina
[Releases](https://github.com/hydese/LumiFI/releases), oppure compilalo (serve
[Rust](https://rustup.rs)):

```sh
cargo run --release -p lumifi                      # si apre sulla libreria
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
| T | Mini player sempre in primo piano |
| ⌘L / Ctrl+L | Libreria ⇄ mini player |
| ⌘, / Ctrl+, | Impostazioni |
| ⌘F / Ctrl+F | Cerca brani, artisti, album e playlist (Invio suona il primo, Esc chiude) |
| ⌘O / Ctrl+O | Apri file o cartelle |

LumiFi si apre sulla **libreria**: album, artisti, brani e playlist, con la ricerca in alto e
la barra di riproduzione in basso. Un clic su un brano lo suona, un altro clic lo mette in
pausa; il tasto destro (o il pulsante ⋯ sulla riga) su brani e album li aggiunge alla coda o
a una playlist. Il cuore su ogni riga li mette nei Preferiti. In Brani, **Riproduci** e
**Casuale** suonano tutta la libreria. In basso a destra si aprono la **coda** e **In riproduzione**, con i
**testi sincronizzati** (da un file .lrc accanto al brano, dai tag, oppure da
[LRCLIB](https://lrclib.net)).

Il pulsante **Mini player** in alto (o Ctrl+L) la sostituisce con il mini player, a
**striscia** o **quadrato**. Il mini player si sposta trascinandolo da un punto libero; con il
**tasto destro** si sceglie la dimensione (striscia 75–150%, quadrato 25–150%: al 25% resta
solo la copertina, con play al passaggio del mouse), la posizione (libera o agganciata in alto
o in basso), la forma e "sempre in primo piano". La musica non si ferma quando cambi finestra,
e all'avvio LumiFi riprende dall'ultimo brano, in pausa.

Un clic sul logo LumiFi (o sull'ingranaggio) apre le **Impostazioni**: Generale, Aspetto,
Audio (anche la scelta dell'uscita), Libreria, Equalizzatore, Servizi, Informazioni. Quello che
non c'è ancora è segnato "In arrivo". All'avvio LumiFi controlla se su GitHub c'è una versione
nuova e, se sì, mostra il pulsante **Aggiorna**. Le playlist sono file .m3u8 nella cartella dei
dati di LumiFi, apribili anche da altri player.

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
| `crates/engine` | il motore audio: decodifica (Symphonia), uscita sempre aperta (cpal), gapless, ricampionatore, equalizzatore, ReplayGain |
| `crates/cli` | `lumifi-cli`, per provare il motore da terminale |
| `packaging/` | come si impacchetta l'app per macOS |
| `patches/` | dipendenze corrette in locale (vedi il README lì dentro) |

## Roadmap

1. **v0.1 — Fondamenta:** motore, CI e release per macOS e Windows.
2. **v0.2 — Mini player:** musica locale, coda.
3. **v0.3 — Libreria e interfaccia:** app completa, scansione delle cartelle, album e artisti,
   impostazioni, playlist, ricerca, testi sincronizzati.
4. **v0.6 — Qualità ed EQ** *(in corso)*: uscita sempre aperta, gapless, ricampionatore,
   ReplayGain ed equalizzatore a 10 bande fatti; poi vista parametrica, AutoEq e modalità
   esclusiva.
5. **v1.0 — Servizi:** Spotify, telecomando di Music.app, Last.fm, aggiornamenti automatici.

## Licenza

LumiFi è software libero, distribuito con la [GNU General Public License v3](LICENSE) o
successiva.

LumiFi non è affiliato a Spotify, Apple o Google.

Dedicato a mio babbo, Giuliano.
