<p align="center">
  <img src="banner.png?v=2" alt="My Translator — Real-time Speech Translation">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/macOS-Apple%20Silicon-black?logo=apple" alt="macOS">
  <img src="https://img.shields.io/badge/Windows-10%2F11-blue?logo=windows" alt="Windows">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License">
  <img src="https://img.shields.io/github/stars/phuc-nt/my-translator?style=flat&color=yellow" alt="Stars">
</p>

**My Translator** is a real-time speech translation desktop app built with Tauri. It captures audio directly from your system or microphone, transcribes it, and displays translations in a minimal overlay — with no intermediary server involved.

> 📖 Installation & usage guides: [macOS (EN)](docs/installation_guide.md) · [macOS (VI)](docs/installation_guide_vi.md) · [Windows (EN)](docs/installation_guide_win.md) · [Windows (VI)](docs/installation_guide_win_vi.md)

---

## How It Works

The app operates in two modes, both following the same pipeline:

**☁️ Cloud Mode (Soniox)**
```
System Audio / Mic → 48kHz → 16kHz PCM → Soniox WebSocket (STT + Translation) → Overlay UI
                                                                                  ↓ (optional)
                                                                              TTS → 🔊
```

**🖥️ Local Mode (default) — Windows & macOS**
```
System Audio / Mic → 16kHz PCM → Whisper ASR → Translation → Overlay UI
                     (on-device)   (on-device)     ↓ (optional)
                                                 TTS → 🔊
```
- **Windows:** faster-whisper + Helsinki-NLP (OPUS-MT). One-time setup ~2–5GB.
- **macOS (Apple Silicon):** MLX + Whisper + Gemma. One-time setup ~5GB.

| | 🖥️ Local (default) | ☁️ Cloud (Soniox) |
|-|---------------------|-------------------|
| **Latency** | ~5–10s | ~2–3s |
| **Languages** | JA/EN/ZH/KO/… → VI/EN | 70+ |
| **Cost** | Free | ~$0.12/hr |
| **Internet** | Not needed | Required |
| **Platform** | Windows 10/11, macOS (Apple Silicon) | All |

---

## TTS Narration (Optional)

Read translations aloud as they appear. Three providers to choose from — no setup required for the default:

| | 🔵 Edge TTS ⭐ | 🌐 Web Speech | 🟣 ElevenLabs |
|-|---------------|--------------|---------------|
| **Cost** | Free | Free | Paid (API key) |
| **Quality** | ★★★★★ Neural | ★★★ Robotic | ★★★★★ Premium |
| **Internet** | Required | Not required | Required |
| **API Key** | Not needed | Not needed | Required |
| **Vietnamese** | ✅ Built-in (HoaiMy, NamMinh) | ⚠️ OS-dependent | ✅ Yes |
| **Platform** | All | All | All |

**Edge TTS** (default) uses Microsoft's neural speech engine — same as "Read Aloud" in Edge browser. Free, no account needed, works out of the box on all platforms. Speed adjustable from −50% to +100%.

> 📖 Full TTS guide: [English](docs/tts_guide.md) · [Tiếng Việt](docs/tts_guide_vi.md)

---

## Privacy

**Your audio never touches our servers — because there are none.**

- The app connects **directly** to the APIs you configure (Soniox, ElevenLabs) — no relay, no middleman
- **You own your API keys** — stored locally on your machine, never transmitted elsewhere
- **No account, no telemetry, no analytics** — zero tracking of any kind
- In Local mode: everything runs **100% on-device**, nothing leaves your machine
- Transcripts are saved as `.md` files locally, per session

---

## Tech Stack

- **[Tauri 2](https://tauri.app/)** — Rust backend + WebView frontend
- **[ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)** — macOS system audio capture
- **[cpal](https://github.com/RustAudio/cpal)** — Cross-platform microphone input
- **[Soniox](https://soniox.com)** — Real-time STT + translation (Cloud mode)
- **Local mode:** [faster-whisper](https://github.com/SYSTRAN/faster-whisper) + [Helsinki-NLP OPUS-MT](https://huggingface.co/Helsinki-NLP) (Windows); [MLX](https://github.com/ml-explore/mlx) + Whisper + Gemma (macOS Apple Silicon)
- **[Edge TTS](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/index-text-to-speech)** — Neural TTS, free, no key required (default)
- **Web Speech API** — OS-native TTS, offline
- **[ElevenLabs](https://elevenlabs.io)** — Premium TTS, API key required

---

## Build from Source

```bash
git clone https://github.com/phuc-nt/my-translator.git
cd my-translator
npm install
npm run tauri build
```

Requires: Rust (stable), Node.js 18+. For Local mode: Windows 10/11 or macOS 13+ (Apple Silicon). Python 3.9+ for Windows local pipeline.

---

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=phuc-nt/my-translator&type=Date)](https://star-history.com/#phuc-nt/my-translator&Date)

---

## License

MIT
