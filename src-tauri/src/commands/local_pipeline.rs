use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::path::Path;
use tauri::ipc::Channel;

/// State for the local pipeline sidecar process
pub struct LocalPipelineState {
    pub process: Mutex<Option<Child>>,
}

/// Cross-platform log file path
fn pipeline_log_path() -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string()));
        Path::new(&appdata).join("Personal Translator").join("pipeline.log")
    } else {
        std::path::PathBuf::from("/tmp/personal_translator_pipeline.log")
    }
}

fn log_to_file(msg: &str) {
    use std::fs::OpenOptions;
    let path = pipeline_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| writeln!(f, "[{}] {}", chrono_now(), msg));
    eprintln!("[local-pipeline] {}", msg);
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", now)
}

/// Get platform-specific venv Python path and pipeline script name
fn local_env_config() -> (String, std::path::PathBuf, &'static str) {
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string()));
        let base = Path::new(&appdata).join("Personal Translator").join("local-env");
        let python = base.join("Scripts").join("python.exe");
        (python.to_string_lossy().into_owned(), base, "local_pipeline_win.py")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/phucnt".to_string());
        let base = Path::new(&home).join("Library/Application Support/Personal Translator/mlx-env");
        let python = base.join("bin").join("python3");
        (python.to_string_lossy().into_owned(), base.clone(), "local_pipeline.py")
    }
}

/// Kill orphaned pipeline processes (macOS/Linux: pkill; Windows: skip to avoid killing wrong processes)
fn kill_orphaned_pipeline() {
    if cfg!(target_os = "windows") {
        // On Windows we don't kill by name to avoid killing other Python apps
        return;
    }
    let _ = Command::new("pkill").args(["-f", "local_pipeline.py"]).output();
    let _ = Command::new("pkill").args(["-f", "local_pipeline_win.py"]).output();
}

/// Find pipeline script in dev or bundled locations
fn find_pipeline_script(script_name: &str) -> Result<std::path::PathBuf, String> {
    let candidates = vec![
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts").join(script_name),
        std::path::PathBuf::from("scripts").join(script_name),
        std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("../Resources/scripts")
            .join(script_name),
    ];
    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    Err(format!(
        "Pipeline script not found. Ensure scripts/{} exists.",
        script_name
    ))
}

/// Start the local translation pipeline (Python sidecar)
#[tauri::command]
pub fn start_local_pipeline(
    source_lang: String,
    target_lang: String,
    channel: Channel<String>,
    state: tauri::State<'_, LocalPipelineState>,
) -> Result<(), String> {
    log_to_file(&format!("start_local_pipeline called: src={}, tgt={}", source_lang, target_lang));

    let _ = channel.send(r#"{"type":"status","message":"Stopping old pipeline..."}"#.to_string());
    stop_local_pipeline_inner(&state);
    kill_orphaned_pipeline();
    std::thread::sleep(std::time::Duration::from_millis(500));

    let _ = channel.send(r#"{"type":"status","message":"Finding pipeline script..."}"#.to_string());

    let (venv_python, _env_base, script_name) = local_env_config();
    let script_path = find_pipeline_script(script_name)?;

    log_to_file(&format!("Using script: {:?}", script_path));
    let _ = channel.send(format!(r#"{{"type":"status","message":"Starting Python pipeline..."}}"#));

    // Prefer venv Python; fallback to system Python
    let python = if Path::new(&venv_python).exists() {
        log_to_file(&format!("Using venv python: {}", venv_python));
        venv_python
    } else if cfg!(target_os = "windows") {
        log_to_file("Using system python (Windows)");
        "python".to_string()
    } else if Path::new("/opt/homebrew/bin/python3").exists() {
        log_to_file("Using homebrew python");
        "/opt/homebrew/bin/python3".to_string()
    } else {
        "python3".to_string()
    };

    let mut cmd = Command::new(&python);
    cmd.arg(&script_path)
        .arg("--source-lang")
        .arg(&source_lang)
        .arg("--target-lang")
        .arg(&target_lang)
        .env("TOKENIZERS_PARALLELISM", "false")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // macOS MLX script uses --asr-model
    if !cfg!(target_os = "windows") {
        cmd.arg("--asr-model").arg("whisper");
    }
    if cfg!(target_os = "windows") {
        let path_env = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", path_env);
    } else {
        cmd.env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin");
        if let Ok(home) = std::env::var("HOME") {
            cmd.env("HOME", home);
        }
    }

    let mut child = cmd.spawn().map_err(|e| {
        let msg = format!("Failed to start pipeline: {}", e);
        log_to_file(&msg);
        msg
    })?;

    log_to_file(&format!("Python process spawned, PID={}", child.id()));
    let _ = channel.send(format!(r#"{{"type":"status","message":"Python started (PID={}), loading models..."}}"#, child.id()));

    let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to get stderr")?;

    let channel_clone = channel.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    log_to_file(&format!("stdout: {}", &line));
                    let _ = channel_clone.send(line);
                }
                Err(e) => {
                    log_to_file(&format!("stdout error: {}", e));
                    break;
                }
                _ => {}
            }
        }
        log_to_file("stdout reader ended");
    });

    let channel_clone2 = channel.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    log_to_file(&format!("stderr: {}", line));
                    let escaped = line.replace('"', r#"\""#);
                    let _ = channel_clone2.send(format!(r#"{{"type":"status","message":"{}"}}"#, escaped));
                }
                Err(_) => break,
            }
        }
        log_to_file("stderr reader ended");
    });

    let mut proc = state.process.lock().map_err(|e| e.to_string())?;
    *proc = Some(child);
    log_to_file("Pipeline state saved, returning OK");
    Ok(())
}

/// Send audio data to the local pipeline stdin
#[tauri::command]
pub fn send_audio_to_pipeline(
    data: Vec<u8>,
    state: tauri::State<'_, LocalPipelineState>,
) -> Result<(), String> {
    let mut proc = state.process.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut child) = *proc {
        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(&data).map_err(|e| {
                log_to_file(&format!("stdin write error: {}", e));
                e.to_string()
            })?;
            stdin.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Stop the local pipeline
#[tauri::command]
pub fn stop_local_pipeline(
    state: tauri::State<'_, LocalPipelineState>,
) -> Result<(), String> {
    log_to_file("stop_local_pipeline called");
    stop_local_pipeline_inner(&state);
    Ok(())
}

fn stop_local_pipeline_inner(state: &LocalPipelineState) {
    if let Ok(mut proc) = state.process.lock() {
        if let Some(mut child) = proc.take() {
            log_to_file(&format!("Killing pipeline PID={}", child.id()));
            drop(child.stdin.take());
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = child.kill();
            let _ = child.wait();
            log_to_file("Pipeline killed");
        }
    }
}

/// Check if local setup is complete (MLX on macOS, Windows venv on Windows)
#[tauri::command]
pub fn check_mlx_setup() -> Result<String, String> {
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string()));
        let marker = Path::new(&appdata).join("Personal Translator").join("local-env").join(".setup_complete_win");
        let venv_python = Path::new(&appdata).join("Personal Translator").join("local-env").join("Scripts").join("python.exe");
        if marker.exists() && venv_python.exists() {
            let content = std::fs::read_to_string(&marker).unwrap_or_default();
            let escaped = content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ");
            Ok(format!(r#"{{"ready":true,"python":"{}","details":"{}"}}"#, venv_python.display(), escaped))
        } else {
            Ok(r#"{"ready":false}"#.to_string())
        }
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/phucnt".to_string());
        let marker = format!("{}/Library/Application Support/Personal Translator/mlx-env/.setup_complete", home);
        let venv_python = format!("{}/Library/Application Support/Personal Translator/mlx-env/bin/python3", home);
        if Path::new(&marker).exists() && Path::new(&venv_python).exists() {
            let content = std::fs::read_to_string(&marker).unwrap_or_default();
            Ok(format!(r#"{{"ready":true,"python":"{}","details":{}}}"#, venv_python, content))
        } else {
            Ok(r#"{"ready":false}"#.to_string())
        }
    }
}

/// Run local setup (setup_mlx.py on macOS, setup_local_win.py on Windows)
#[tauri::command]
pub fn run_mlx_setup(
    channel: Channel<String>,
) -> Result<(), String> {
    log_to_file("run_mlx_setup called");

    let (script_name, system_python) = if cfg!(target_os = "windows") {
        ("setup_local_win.py", "python")
    } else {
        ("setup_mlx.py", "/opt/homebrew/bin/python3")
    };

    let script_path = {
        let candidates = vec![
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts").join(script_name),
            std::path::PathBuf::from("scripts").join(script_name),
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("../Resources/scripts")
                .join(script_name),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .ok_or_else(|| format!("Setup script {} not found.", script_name))?
    };

    let python = if cfg!(target_os = "windows") {
        "python".to_string()
    } else if Path::new(system_python).exists() {
        system_python.to_string()
    } else {
        "python3".to_string()
    };

    let mut child = Command::new(&python)
        .arg(&script_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start setup: {}", e))?;

    log_to_file(&format!("Setup process spawned, PID={}", child.id()));

    let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
    let channel_clone = channel.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    log_to_file(&format!("setup stdout: {}", &line));
                    let _ = channel_clone.send(line);
                }
                Err(e) => {
                    log_to_file(&format!("setup stdout error: {}", e));
                    break;
                }
                _ => {}
            }
        }
    });

    let stderr = child.stderr.take().ok_or("Failed to get stderr")?;
    let channel_clone2 = channel.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    log_to_file(&format!("setup stderr: {}", line));
                    let escaped = line.replace('"', r#"\""#);
                    let _ = channel_clone2.send(format!(r#"{{"type":"log","message":"{}"}}"#, escaped));
                }
                Err(_) => break,
            }
        }
    });

    Ok(())
}
