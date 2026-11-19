<div align="center">

# LumiFi

**Un music player piccolo, leggero e in alta qualità per macOS e Windows.**<br />
La tua musica, come è stata registrata.

[![CI](https://github.com/hydese/lumifi/actions/workflows/ci.yml/badge.svg)](https://github.com/hydese/lumifi/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/license-GPL%20v3-3d6bf0)](LICENSE)

</div>

> [!NOTE]
> LumiFi è all'inizio. Oggi c'è il motore audio con un piccolo programma da riga di comando
> per provarlo; l'app con la sua interfaccia arriva nelle prossime versioni.

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

## Provare il motore

Scarica il programma per il tuo sistema dagli artifact dell'ultima
[esecuzione della CI](https://github.com/hydese/lumifi/actions/workflows/ci.yml), oppure
compilalo (serve [Rust](https://rustup.rs)):

```sh
cargo run --release -- info  "~/Music/album/01 - brano.flac"
cargo run --release -- play  "~/Music/album/01 - brano.flac"
cargo run --release -- play  brano.flac --volume 60
cargo run --release -- devices
```

`info` mostra codec, frequenza, bit e durata; `play` lo suona sul dispositivo predefinito alla
frequenza del file. Con il volume a 100 il segnale non viene toccato.

> [!TIP]
> **macOS:** i programmi non firmati da Apple vengono bloccati al primo avvio. Dopo averlo
> scaricato, esegui `xattr -d com.apple.quarantine ./lumifi`.

## Struttura

| Cartella | Contenuto |
| --- | --- |
| `crates/engine` | il motore audio: decodifica (Symphonia) e uscita (cpal) |
| `crates/cli` | il programma `lumifi` da riga di comando |

## Roadmap

1. **v0.1 — Fondamenta:** motore, CI e release per macOS e Windows.
2. **v0.3 — Libreria e interfaccia:** scansione delle cartelle, viste album e artisti, mini
   player, coda e playlist.
3. **v0.6 — Qualità ed EQ:** modalità esclusiva, gapless, ReplayGain, equalizzatore e AutoEq.
4. **v1.0 — Servizi:** Spotify, telecomando di Music.app, Last.fm, aggiornamenti automatici.

## Licenza

LumiFi è software libero, distribuito con la [GNU General Public License v3](LICENSE) o
successiva.

LumiFi non è affiliato a Spotify, Apple o Google.
