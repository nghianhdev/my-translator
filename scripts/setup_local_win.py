#!/usr/bin/env python3
"""
Local pipeline setup for Windows (and optionally Linux).
Creates a venv, installs faster-whisper and translation models.
Reports progress via JSON lines to stdout for the Tauri frontend.

Usage:
    python setup_local_win.py [--check] [--env-dir DIR]
"""

import json
import os
import subprocess
import sys
import argparse
import shutil


def emit(data):
    """Print JSON line to stdout for Tauri to read."""
    print(json.dumps(data, ensure_ascii=False), flush=True)


def get_default_env_dir():
    """Get default venv directory (APPDATA on Windows, else ~/.local/share)."""
    if sys.platform == "win32":
        appdata = os.environ.get("APPDATA") or os.environ.get("LOCALAPPDATA") or os.path.expanduser("~")
        return os.path.join(appdata, "Personal Translator", "local-env")
    return os.path.join(os.path.expanduser("~"), ".local", "share", "Personal Translator", "local-env")


def get_marker_path(env_dir):
    return os.path.join(env_dir, ".setup_complete_win")


def is_setup_complete(env_dir):
    marker = get_marker_path(env_dir)
    if not os.path.exists(marker):
        return False
    if sys.platform == "win32":
        venv_python = os.path.join(env_dir, "Scripts", "python.exe")
    else:
        venv_python = os.path.join(env_dir, "bin", "python3")
    if not os.path.exists(venv_python):
        return False
    try:
        with open(marker) as f:
            data = json.load(f)
            return data.get("version") == 1
    except Exception:
        return False


def check_system_python():
    """Find Python 3.9+ for creating venv."""
    candidates = ["python", "python3", "py -3"]
    for name in candidates:
        try:
            if name.startswith("py "):
                result = subprocess.run(name.split() + ["--version"], capture_output=True, text=True, timeout=5)
            else:
                result = subprocess.run([name, "--version"], capture_output=True, text=True, timeout=5)
            out = (result.stdout or result.stderr or "").strip()
            # "Python 3.11.0" or "Python 3.9.0"
            if "Python" in out:
                parts = out.split()[-1].split(".")
                if len(parts) >= 2 and int(parts[0]) >= 3 and int(parts[1]) >= 9:
                    exe = shutil.which(name.split()[0] if name.startswith("py ") else name)
                    if exe:
                        return exe, out.split()[-1]
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
        raise RuntimeError(f"Failed to create venv: {result.stderr or result.stdout}")

    pip = os.path.join(env_dir, "Scripts", "pip.exe") if sys.platform == "win32" else os.path.join(env_dir, "bin", "pip3")
    subprocess.run(
        [pip, "install", "--upgrade", "pip"],
        capture_output=True, text=True, timeout=120
    )
    emit({"type": "progress", "step": "venv", "message": "Python environment created", "done": True})


def install_packages(env_dir):
    pip = os.path.join(env_dir, "Scripts", "pip.exe") if sys.platform == "win32" else os.path.join(env_dir, "bin", "pip3")
    packages = [
        ("numpy", "Array processing"),
        ("faster-whisper", "Whisper ASR (CPU/CUDA)"),
        ("transformers", "Translation models"),
        ("torch", "PyTorch"),
        ("sentencepiece", "Tokenizers"),
    ]
    total = len(packages)
    for i, (pkg, desc) in enumerate(packages):
        emit({
            "type": "progress",
            "step": "packages",
            "message": f"Installing {pkg} ({i+1}/{total})... {desc}",
            "progress": (i / total) * 100,
        })
        result = subprocess.run(
            [pip, "install", pkg],
            capture_output=True, text=True, timeout=600
        )
        if result.returncode != 0:
            raise RuntimeError(f"Failed to install {pkg}: {(result.stderr or result.stdout)[:500]}")
    emit({
        "type": "progress",
        "step": "packages",
        "message": "All packages installed",
        "progress": 100,
        "done": True,
    })


def download_models(env_dir):
    python_exe = os.path.join(env_dir, "Scripts", "python.exe") if sys.platform == "win32" else os.path.join(env_dir, "bin", "python3")
    # Pre-download faster-whisper base model and one translation model
    models = [
        ("faster-whisper base", "from faster_whisper import WhisperModel; WhisperModel('base', device='cpu', compute_type='int8')"),
        ("Helsinki en-vi", "from transformers import pipeline; pipeline('translation', model='Helsinki-NLP/opus-mt-en-vi')"),
    ]
    total = len(models)
    for i, (desc, code) in enumerate(models):
        emit({
            "type": "progress",
            "step": "models",
            "message": f"Downloading {desc} ({i+1}/{total})...",
            "progress": (i / total) * 100,
        })
        result = subprocess.run(
            [python_exe, "-c", code],
            capture_output=True, text=True, timeout=900
        )
        if result.returncode != 0:
            raise RuntimeError(f"Failed to download {desc}: {(result.stderr or result.stdout)[:500]}")
    emit({
        "type": "progress",
        "step": "models",
        "message": "All models downloaded",
        "progress": 100,
        "done": True,
    })


def write_marker(env_dir):
    marker = get_marker_path(env_dir)
    python_exe = os.path.join(env_dir, "Scripts", "python.exe") if sys.platform == "win32" else os.path.join(env_dir, "bin", "python3")
    with open(marker, "w") as f:
        json.dump({
            "version": 1,
            "python": python_exe,
            "platform": sys.platform,
        }, f, indent=2)


def main():
    parser = argparse.ArgumentParser(description="Local pipeline setup (Windows)")
    parser.add_argument("--check", action="store_true", help="Check if setup is complete")
    parser.add_argument("--env-dir", default=None, help="Custom venv directory")
    args = parser.parse_args()

    env_dir = args.env_dir or get_default_env_dir()

    if args.check:
        if is_setup_complete(env_dir):
            python_exe = os.path.join(env_dir, "Scripts", "python.exe") if sys.platform == "win32" else os.path.join(env_dir, "bin", "python3")
            emit({"type": "check", "ready": True, "env_dir": env_dir, "python": python_exe})
            sys.exit(0)
        else:
            emit({"type": "check", "ready": False, "env_dir": env_dir})
            sys.exit(1)

    emit({"type": "start", "message": "Starting local pipeline setup...", "env_dir": env_dir})

    try:
        python_path, python_version = check_system_python()
        if not python_path:
            emit({"type": "error", "message": "Python 3.9+ not found. Please install Python from python.org"})
            sys.exit(1)

        emit({"type": "progress", "step": "check", "message": f"Found Python {python_version} at {python_path}"})

        create_venv(python_path, env_dir)
        install_packages(env_dir)
        download_models(env_dir)
        write_marker(env_dir)

        python_exe = os.path.join(env_dir, "Scripts", "python.exe") if sys.platform == "win32" else os.path.join(env_dir, "bin", "python3")
        emit({
            "type": "complete",
            "message": "Local setup complete! Ready to translate.",
            "python": python_exe,
            "env_dir": env_dir,
        })

    except Exception as e:
        emit({"type": "error", "message": str(e)})
        sys.exit(1)


if __name__ == "__main__":
    main()
