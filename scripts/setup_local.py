#!/usr/bin/env python3
"""
Local Model Setup Script — Cross-platform auto-install for local translation pipeline.

macOS Apple Silicon: installs MLX + mlx-whisper + mlx-lm (GPU-accelerated via Metal)
macOS Intel / Windows / Linux: installs faster-whisper + llama-cpp-python (CPU/CUDA)

Creates a Python venv, installs required packages, and pre-downloads models.
Reports progress via JSON lines to stdout for the Tauri frontend.

Usage:
    python3 setup_local.py [--check] [--env-dir DIR]
"""

import json
import os
import platform
import subprocess
import sys
import argparse
import shutil


def emit(data):
    """Print JSON line to stdout for Tauri to read."""
    print(json.dumps(data, ensure_ascii=False), flush=True)


def is_apple_silicon():
    return sys.platform == "darwin" and platform.machine() == "arm64"


def get_default_env_dir():
    if sys.platform == "win32":
        base = os.environ.get("LOCALAPPDATA", os.path.expanduser("~"))
        return os.path.join(base, "My Translator", "local-env")
    else:
        app_support = os.path.expanduser("~/Library/Application Support/My Translator")
        return os.path.join(app_support, "local-env")


def get_marker_path(env_dir):
    return os.path.join(env_dir, ".setup_complete")


def get_venv_python(env_dir):
    if sys.platform == "win32":
        return os.path.join(env_dir, "Scripts", "python.exe")
    return os.path.join(env_dir, "bin", "python3")


def get_venv_pip(env_dir):
    if sys.platform == "win32":
        return os.path.join(env_dir, "Scripts", "pip.exe")
    return os.path.join(env_dir, "bin", "pip3")


def is_setup_complete(env_dir):
    marker = get_marker_path(env_dir)
    if not os.path.exists(marker):
        return False
    venv_python = get_venv_python(env_dir)
    if not os.path.exists(venv_python):
        return False
    try:
        with open(marker) as f:
            data = json.load(f)
            return data.get("version") == 3
    except Exception:
        return False


def check_system_python():
    """Find a suitable Python 3.10+ for creating venv."""
    if sys.platform == "win32":
        candidates = [
            shutil.which("python"),
            shutil.which("python3"),
            "C:\\Python312\\python.exe",
            "C:\\Python311\\python.exe",
            "C:\\Python310\\python.exe",
        ]
    else:
        candidates = [
            "/opt/homebrew/bin/python3",
            "/usr/local/bin/python3",
            shutil.which("python3"),
        ]
    for path in candidates:
        if path and os.path.exists(path):
            try:
                result = subprocess.run(
                    [path, "--version"],
                    capture_output=True, text=True, timeout=5
                )
                version = result.stdout.strip().split()[-1]
                major, minor = map(int, version.split(".")[:2])
                if major >= 3 and minor >= 10:
                    return path, version
            except Exception:
                continue
    return None, None


def create_venv(python_path, env_dir):
    emit({"type": "progress", "step": "venv", "message": "Creating Python environment..."})

    os.makedirs(env_dir, exist_ok=True)

    result = subprocess.run(
        [python_path, "-m", "venv", env_dir, "--clear"],
        capture_output=True, text=True, timeout=120
    )
    if result.returncode != 0:
        raise RuntimeError(f"Failed to create venv: {result.stderr}")

    venv_pip = get_venv_pip(env_dir)
    subprocess.run(
        [venv_pip, "install", "--upgrade", "pip"],
        capture_output=True, text=True, timeout=120
    )

    emit({"type": "progress", "step": "venv", "message": "Python environment created [OK]", "done": True})


def install_packages(env_dir):
    venv_pip = get_venv_pip(env_dir)

    if is_apple_silicon():
        packages = [
            ("numpy", "Array processing"),
            ("mlx", "Apple Silicon ML framework"),
            ("mlx-lm", "LLM inference (MLX)"),
            ("mlx-whisper", "Whisper ASR (MLX)"),
        ]
    else:
        packages = [
            ("numpy", "Array processing"),
            ("torch", "PyTorch deep learning"),
            ("faster-whisper", "Whisper ASR (CTranslate2)"),
            ("llama-cpp-python", "LLM inference (GGUF)"),
            ("huggingface-hub", "Model downloads"),
        ]

    total = len(packages)
    for i, (pkg, desc) in enumerate(packages):
        emit({
            "type": "progress",
            "step": "packages",
            "message": f"Installing {pkg} ({i+1}/{total})... {desc}",
            "progress": (i / total) * 100,
        })

        cmd = [venv_pip, "install", pkg]
        if pkg == "torch" and sys.platform != "darwin":
            cmd = [venv_pip, "install", "torch", "--index-url", "https://download.pytorch.org/whl/cpu"]

        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=600
        )
        if result.returncode != 0:
            raise RuntimeError(f"Failed to install {pkg}: {result.stderr[:500]}")

    emit({
        "type": "progress",
        "step": "packages",
        "message": "All packages installed [OK]",
        "progress": 100,
        "done": True,
    })


def download_models(env_dir):
    venv_python = get_venv_python(env_dir)

    if is_apple_silicon():
        models = [
            ("mlx-community/whisper-large-v3-turbo", "Whisper ASR (~1.5GB)"),
            ("mlx-community/gemma-3-4b-it-qat-4bit", "Gemma Translation (~3GB)"),
        ]
    else:
        models = [
            ("Systran/faster-whisper-large-v3", "Whisper ASR (~3GB)"),
            ("bartowski/gemma-2-2b-it-GGUF:gemma-2-2b-it-Q4_K_M.gguf", "Gemma Translation (~1.5GB)"),
        ]

    total = len(models)
    for i, (model_id, desc) in enumerate(models):
        emit({
            "type": "progress",
            "step": "models",
            "message": f"Downloading {desc} ({i+1}/{total})...",
            "progress": (i / total) * 100,
        })

        if ":" in model_id:
            repo_id, filename = model_id.split(":", 1)
            script = f"""
import sys
try:
    from huggingface_hub import hf_hub_download
    hf_hub_download("{repo_id}", "{filename}")
    print("OK", flush=True)
except Exception as e:
    print(f"ERROR: {{e}}", file=sys.stderr, flush=True)
    sys.exit(1)
"""
        else:
            script = f"""
import sys
try:
    from huggingface_hub import snapshot_download
    snapshot_download("{model_id}")
    print("OK", flush=True)
except Exception as e:
    print(f"ERROR: {{e}}", file=sys.stderr, flush=True)
    sys.exit(1)
"""
        result = subprocess.run(
            [venv_python, "-c", script],
            capture_output=True, text=True, timeout=1800
        )
        if result.returncode != 0:
            raise RuntimeError(f"Failed to download {model_id}: {result.stderr[:500]}")

    emit({
        "type": "progress",
        "step": "models",
        "message": "All models downloaded [OK]",
        "progress": 100,
        "done": True,
    })


def write_marker(env_dir):
    marker = get_marker_path(env_dir)

    if is_apple_silicon():
        backend = "mlx"
        models = [
            "mlx-community/whisper-large-v3-turbo",
            "mlx-community/gemma-3-4b-it-qat-4bit",
        ]
    else:
        backend = "ctranslate2"
        models = [
            "Systran/faster-whisper-large-v3",
            "bartowski/gemma-2-2b-it-GGUF",
        ]

    with open(marker, "w") as f:
        json.dump({
            "version": 3,
            "backend": backend,
            "platform": sys.platform,
            "arch": platform.machine(),
            "python": get_venv_python(env_dir),
            "models": models,
        }, f, indent=2)


def main():
    parser = argparse.ArgumentParser(description="Local Model Setup")
    parser.add_argument("--check", action="store_true", help="Check if setup is complete")
    parser.add_argument("--env-dir", default=None, help="Custom venv directory")
    args = parser.parse_args()

    env_dir = args.env_dir or get_default_env_dir()

    if args.check:
        if is_setup_complete(env_dir):
            emit({"type": "check", "ready": True, "env_dir": env_dir,
                  "python": get_venv_python(env_dir)})
            sys.exit(0)
        else:
            emit({"type": "check", "ready": False, "env_dir": env_dir})
            sys.exit(1)

    backend = "MLX (Apple Silicon)" if is_apple_silicon() else "CTranslate2 + llama.cpp (CPU/CUDA)"
    emit({"type": "start", "message": f"Starting local setup ({backend})...", "env_dir": env_dir})

    try:
        python_path, python_version = check_system_python()
        if not python_path:
            hint = "brew install python" if sys.platform == "darwin" else "python.org/downloads"
            emit({"type": "error", "message": f"Python 3.10+ not found. Install from: {hint}"})
            sys.exit(1)

        emit({"type": "progress", "step": "check",
              "message": f"Found Python {python_version} at {python_path} [OK]"})

        create_venv(python_path, env_dir)
        install_packages(env_dir)
        download_models(env_dir)
        write_marker(env_dir)

        emit({
            "type": "complete",
            "message": "Local model setup complete! Ready to translate.",
            "python": get_venv_python(env_dir),
            "env_dir": env_dir,
        })

    except Exception as e:
        emit({"type": "error", "message": str(e)})
        sys.exit(1)


if __name__ == "__main__":
    main()
