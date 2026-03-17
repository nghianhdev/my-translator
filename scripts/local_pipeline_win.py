#!/usr/bin/env python3
"""
Local translation pipeline for Windows (and Linux).
Uses faster-whisper for ASR and Helsinki-NLP / transformers for translation.
Same protocol as macOS pipeline: stdin PCM s16le 16kHz, stdout JSON lines.

Usage:
    python local_pipeline_win.py --source-lang ja --target-lang vi
"""

import sys
import os
import json
import time
import wave
import tempfile
import threading
import argparse
import numpy as np

os.environ["TOKENIZERS_PARALLELISM"] = "false"


def log(msg):
    print(f"[pipeline] {msg}", file=sys.stderr, flush=True)


def emit(data):
    print(json.dumps(data, ensure_ascii=False), flush=True)


# Map app language names to Whisper and translation model codes
WHISPER_LANG = {
    "auto": None,
    "Japanese": "ja", "ja": "ja",
    "English": "en", "en": "en",
    "Chinese": "zh", "zh": "zh",
    "Korean": "ko", "ko": "ko",
    "Vietnamese": "vi", "vi": "vi",
    "French": "fr", "German": "de", "Spanish": "es",
}
TARGET_LANG_NAME = {
    "vi": "Vietnamese", "en": "English", "ja": "Japanese",
    "ko": "Korean", "zh": "Chinese", "fr": "French",
    "de": "German", "es": "Spanish",
}


class LocalPipelineWin:
    def __init__(self, source_lang="ja", target_lang="vi", chunk_seconds=7, stride_seconds=5):
        self.source_lang = source_lang
        self.target_lang = target_lang
        self.target_lang_name = TARGET_LANG_NAME.get(target_lang, "Vietnamese")
        self.chunk_seconds = chunk_seconds
        self.stride_seconds = stride_seconds
        self.sample_rate = 16000
        self.bytes_per_sample = 2
        self.chunk_bytes = chunk_seconds * self.sample_rate * self.bytes_per_sample
        self.stride_bytes = stride_seconds * self.sample_rate * self.bytes_per_sample

        self.audio_buffer = bytearray()
        self.lock = threading.Lock()
        self.running = True
        self.prev_text = ""
        self.context_history = []
        self.max_context = 5

        self.whisper_model = None
        self.translate_pipes = {}  # (src, tgt) -> pipeline
        self._load_models()

    def _load_models(self):
        # ASR: faster-whisper (base for CPU, small if GPU available)
        log("Loading faster-whisper (base)...")
        emit({"type": "status", "message": "Loading Whisper..."})
        t0 = time.time()
        from faster_whisper import WhisperModel
        device = "cuda" if self._has_cuda() else "cpu"
        compute_type = "float16" if device == "cuda" else "int8"
        self.whisper_model = WhisperModel("base", device=device, compute_type=compute_type)
        log(f"Whisper loaded in {time.time()-t0:.1f}s")

        # Translation: load on first use
        emit({"type": "status", "message": "Translation models will load on first use."})
        log("Pipeline ready!")
        emit({"type": "ready"})

    def _has_cuda(self):
        try:
            import torch
            return torch.cuda.is_available()
        except Exception:
            return False

    def _get_translator(self, src_lang, tgt_lang):
        """Get or create a translation pipeline for (src_lang, tgt_lang)."""
        key = (src_lang, tgt_lang)
        if key in self.translate_pipes:
            return self.translate_pipes[key]
        from transformers import pipeline
        # Helsinki model naming: opus-mt-{src}-{tgt}
        model_map = {
            ("en", "vi"): "Helsinki-NLP/opus-mt-en-vi",
            ("vi", "en"): "Helsinki-NLP/opus-mt-vi-en",
            ("ja", "en"): "Helsinki-NLP/opus-mt-ja-en",
            ("en", "ja"): "Helsinki-NLP/opus-mt-en-ja",
            ("zh", "en"): "Helsinki-NLP/opus-mt-zh-en",
            ("en", "zh"): "Helsinki-NLP/opus-mt-en-zh",
            ("ko", "en"): "Helsinki-NLP/opus-mt-ko-en",
            ("en", "ko"): "Helsinki-NLP/opus-mt-en-ko",
            ("fr", "en"): "Helsinki-NLP/opus-mt-fr-en",
            ("de", "en"): "Helsinki-NLP/opus-mt-de-en",
            ("es", "en"): "Helsinki-NLP/opus-mt-es-en",
        }
        model_id = model_map.get(key)
        if model_id:
            pipe = pipeline("translation", model=model_id)
            self.translate_pipes[key] = pipe
            return pipe
        # Pivot via English if direct pair not available (e.g. ja -> vi)
        if tgt_lang != "en" and src_lang != "en":
            en_pipe = self._get_translator(src_lang, "en")
            to_tgt_pipe = self._get_translator("en", tgt_lang)
            if en_pipe and to_tgt_pipe and not isinstance(en_pipe, tuple) and not isinstance(to_tgt_pipe, tuple):
                self.translate_pipes[key] = ("pivot", en_pipe, to_tgt_pipe)
                return self.translate_pipes[key]
        return None

    def _translate(self, text, src_lang_hint="en"):
        """Translate text. src_lang_hint from ASR result."""
        if not text or not text.strip():
            return ""
        from transformers import pipeline
        src = src_lang_hint if src_lang_hint in ("en", "ja", "zh", "ko", "vi", "fr", "de", "es") else "en"
        tgt = self.target_lang
        if src == tgt:
            return text.strip()

        translator = self._get_translator(src, tgt)
        if translator is None:
            return text.strip()
        if isinstance(translator, tuple) and translator[0] == "pivot":
            _, pipe1, pipe2 = translator
            en = pipe1(text.strip())[0]["translation_text"]
            return pipe2(en)[0]["translation_text"]
        out = translator(text.strip())[0]["translation_text"]
        return out

    def _save_chunk_as_wav(self, pcm_bytes):
        tmp = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
        with wave.open(tmp.name, "w") as wf:
            wf.setnchannels(1)
            wf.setsampwidth(2)
            wf.setframerate(self.sample_rate)
            wf.writeframes(pcm_bytes)
        return tmp.name

    def _transcribe(self, wav_path):
        lang = WHISPER_LANG.get(self.source_lang, "ja")
        if lang == "auto":
            lang = None
        segments, info = self.whisper_model.transcribe(wav_path, language=lang, task="transcribe")
        text = " ".join(s.text for s in segments).strip()
        detected = info.language if info else (lang or "en")
        return text, detected

    def _dedup_transcript(self, text):
        if not self.prev_text or not text:
            return text
        prev, cur = self.prev_text, text
        best = 0
        for n in range(3, min(len(prev), len(cur), 100) + 1):
            if prev[-n:] == cur[:n]:
                best = n
        if best >= 3:
            return cur[best:].strip() or cur
        return cur

    def _process_chunk(self, pcm_bytes):
        samples = np.frombuffer(pcm_bytes, dtype=np.int16)
        rms = np.sqrt(np.mean(samples.astype(np.float32) ** 2))
        if rms < 100:
            return
        wav_path = self._save_chunk_as_wav(pcm_bytes)
        try:
            t1 = time.time()
            text, lang = self._transcribe(wav_path)
            t_asr = time.time() - t1
            if not text or text == self.prev_text:
                return
            new_text = self._dedup_transcript(text)
            if not new_text or len(new_text) < 2:
                self.prev_text = text
                return

            log(f"Transcript: {text}")
            t2 = time.time()
            translated = self._translate(new_text, lang)
            t_llm = time.time() - t2
            total = time.time() - t1
            log(f"ASR={t_asr:.2f}s translate={t_llm:.2f}s total={total:.2f}s")

            emit({
                "type": "result",
                "original": new_text,
                "translated": translated,
                "language": lang if isinstance(lang, str) else (lang[0] if lang else "en"),
                "timing": {"asr": round(t_asr, 2), "translate": round(t_llm, 2), "total": round(total, 2)},
            })
            self.prev_text = text
        finally:
            try:
                os.unlink(wav_path)
            except Exception:
                pass

    def stdin_reader(self):
        try:
            while self.running:
                data = sys.stdin.buffer.read(4096)
                if not data:
                    break
                with self.lock:
                    self.audio_buffer.extend(data)
        except Exception as e:
            log(f"stdin reader error: {e}")
        finally:
            self.running = False

    def run(self):
        reader = threading.Thread(target=self.stdin_reader, daemon=True)
        reader.start()
        processed_pos = 0
        while self.running:
            time.sleep(0.5)
            with self.lock:
                buf_len = len(self.audio_buffer)
            if buf_len - processed_pos >= self.chunk_bytes:
                with self.lock:
                    chunk = bytes(self.audio_buffer[processed_pos:processed_pos + self.chunk_bytes])
                self._process_chunk(chunk)
                processed_pos += self.stride_bytes
        with self.lock:
            remaining = len(self.audio_buffer) - processed_pos
            if remaining > self.sample_rate * self.bytes_per_sample:
                chunk = bytes(self.audio_buffer[processed_pos:])
                self._process_chunk(chunk)
        emit({"type": "done"})
        log("Pipeline stopped.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-lang", default="ja", help="Source language")
    parser.add_argument("--target-lang", default="vi", help="Target language code")
    parser.add_argument("--chunk-seconds", type=int, default=7)
    parser.add_argument("--stride-seconds", type=int, default=5)
    args = parser.parse_args()

    # Normalize source_lang to display name if needed (app may send "Japanese" or "ja")
    source = args.source_lang
    if source in ("auto", "Auto", "auto-detect"):
        source = "ja"

    pipeline = LocalPipelineWin(
        source_lang=source,
        target_lang=args.target_lang,
        chunk_seconds=args.chunk_seconds,
        stride_seconds=args.stride_seconds,
    )
    pipeline.run()


if __name__ == "__main__":
    main()
