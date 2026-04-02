use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::ipc::Channel;

/// State for the local pipeline sidecar process
pub struct LocalPipelineState {
    pub process: Mutex<Option<Child>>,
}

fn log_to_file(msg: &str) {
    use std::fs::OpenOptions;
    let log_path = get_log_path();
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            writeln!(f, "[{}] {}", chrono_now(), msg)
        });
    eprintln!("[local-pipeline] {}", msg);
}

fn get_log_path() -> String {
    if cfg!(target_os = "windows") {
        let temp = std::env::var("TEMP")
            .unwrap_or_else(|_| std::env::var("TMP").unwrap_or_else(|_| ".".to_string()));
        format!("{}\\personal_translator_pipeline.log", temp)
    } else {
        "/tmp/personal_translator_pipeline.log".to_string()
    }
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", now)
}

fn get_app_data_dir() -> String {
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string()));
        format!("{}\\My Translator", appdata)
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/phucnt".to_string());
        format!("{}/Library/Application Support/My Translator", home)
    }
}

fn get_venv_python() -> String {
    let app_dir = get_app_data_dir();
    if cfg!(target_os = "windows") {
        format!("{}\\local-env\\Scripts\\python.exe", app_dir)
    } else {
        format!("{}/local-env/bin/python3", app_dir)
    }
}

fn get_setup_marker() -> String {
    let app_dir = get_app_data_dir();
    if cfg!(target_os = "windows") {
        format!("{}\\local-env\\.setup_complete", app_dir)
    } else {
        format!("{}/local-env/.setup_complete", app_dir)
    }
}

fn find_system_python() -> String {
    if cfg!(target_os = "windows") {
        // On Windows, try common Python paths
        let candidates = vec![
            "python".to_string(),
            "python3".to_string(),
            "C:\\Python312\\python.exe".to_string(),
            "C:\\Python311\\python.exe".to_string(),
            "C:\\Python310\\python.exe".to_string(),
        ];
        for candidate in &candidates {
            if Command::new(candidate)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
            {
                return candidate.clone();
            }
        }
        "python".to_string()
    } else {
        if std::path::Path::new("/opt/homebrew/bin/python3").exists() {
            "/opt/homebrew/bin/python3".to_string()
        } else {
            "python3".to_string()
        }
    }
}

fn find_pipeline_script() -> Result<std::path::PathBuf, String> {
    let mut candidates = vec![
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/local_pipeline.py"),
        std::path::PathBuf::from("scripts/local_pipeline.py"),
    ];

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if cfg!(target_os = "windows") {
                candidates.push(parent.join("scripts/local_pipeline.py"));
            } else {
                candidates.push(parent.join("../Resources/scripts/local_pipeline.py"));
            }
        }
    }

    log_to_file(&format!(
        "Checking candidates: {:?}",
        candidates.iter().map(|p| format!("{:?} exists={}", p, p.exists())).collect::<Vec<_>>()
    ));

    candidates
        .into_iter()
        .find(|p| p.exists())
        .ok_or_else(|| "Pipeline script not found. Ensure scripts/local_pipeline.py exists.".to_string())
}

fn find_setup_script() -> Result<std::path::PathBuf, String> {
    let mut candidates = vec![
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/setup_local.py"),
        std::path::PathBuf::from("scripts/setup_local.py"),
    ];

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if cfg!(target_os = "windows") {
                candidates.push(parent.join("scripts/setup_local.py"));
            } else {
                candidates.push(parent.join("../Resources/scripts/setup_local.py"));
            }
        }
    }

    candidates
        .into_iter()
        .find(|p| p.exists())
        .ok_or_else(|| "Setup script not found.".to_string())
}

fn kill_orphaned_pipelines() {
    if cfg!(target_os = "windows") {
        let _ = Command::new("taskkill")
            .args(["/F", "/FI", "IMAGENAME eq python.exe", "/FI", "WINDOWTITLE eq local_pipeline*"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output();
    } else {
        let _ = Command::new("pkill")
            .args(["-f", "local_pipeline.py"])
            .output();
    }
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
    kill_orphaned_pipelines();

    std::thread::sleep(std::time::Duration::from_millis(500));

    let _ = channel.send(r#"{"type":"status","message":"Finding pipeline script..."}"#.to_string());

    let script_path = find_pipeline_script()?;

    log_to_file(&format!("Using script: {:?}", script_path));
    let _ = channel.send(r#"{"type":"status","message":"Starting Python pipeline..."}"#.to_string());

    let venv_python = get_venv_python();
    let python = if std::path::Path::new(&venv_python).exists() {
        log_to_file(&format!("Using venv python: {}", venv_python));
        venv_python
    } else {
        let sys_python = find_system_python();
        log_to_file(&format!("Using system python: {}", sys_python));
        sys_python
    };

    let mut cmd = Command::new(&python);
    cmd.arg(&script_path)
        .arg("--asr-model").arg("whisper")
        .arg("--source-lang").arg(&source_lang)
        .arg("--target-lang").arg(&target_lang)
        .env("TOKENIZERS_PARALLELISM", "false")
        // Help diagnose native crashes (0xc0000005) in Python wheels.
        .env("PYTHONFAULTHANDLER", "1")
        .env("PYTHONUNBUFFERED", "1")
        // Force UTF-8 so Vietnamese text doesn't crash on cp932 consoles.
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/phucnt".to_string());
        cmd.env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin")
           .env("HOME", &home);
    }

    let mut child = cmd.spawn().map_err(|e| {
        let msg = format!("Failed to start pipeline: {}", e);
        log_to_file(&msg);
        msg
    })?;

    log_to_file(&format!("Python process spawned, PID={}", child.id()));
    let _ = channel.send(format!(
        r#"{{"type":"status","message":"Python started (PID={}), loading models..."}}"#,
        child.id()
    ));

    let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to get stderr")?;

    let channel_clone = channel.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout);
        loop {
            let mut buf = Vec::new();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    while buf.last() == Some(&b'\n') || buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                    if buf.is_empty() {
                        continue;
                    }
                    let line = String::from_utf8(buf.clone())
                        .unwrap_or_else(|_| String::from_utf8_lossy(&buf).to_string());
                    log_to_file(&format!("stdout: {}", &line));
                    let _ = channel_clone.send(line);
                }
                Err(e) => {
                    log_to_file(&format!("stdout error: {}", e));
                    break;
                }
            }
        }
        log_to_file("stdout reader ended");
    });

    let channel_clone2 = channel.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stderr);
        loop {
            let mut buf = Vec::new();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    while buf.last() == Some(&b'\n') || buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                    if buf.is_empty() {
                        continue;
                    }
                    let line = String::from_utf8(buf.clone())
                        .unwrap_or_else(|_| String::from_utf8_lossy(&buf).to_string());
                    log_to_file(&format!("stderr: {}", line));
                    let escaped = line.replace('"', r#"\""#);
                    let _ = channel_clone2.send(format!(
                        r#"{{"type":"status","message":"{}"}}"#,
                        escaped
                    ));
                }
                Err(e) => {
                    log_to_file(&format!("stderr error: {}", e));
                    break;
                }
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
    let Some(ref mut child) = *proc else {
        return Err("Local pipeline is not running".to_string());
    };

    // If the child already exited, stop and clear state so the UI can restart cleanly.
    match child.try_wait() {
        Ok(Some(status)) => {
            log_to_file(&format!("pipeline already exited: {}", status));
            stop_local_pipeline_inner(&state);
            return Err(format!("Local pipeline exited: {}", status));
        }
        Ok(None) => {}
        Err(e) => {
            log_to_file(&format!("try_wait error: {}", e));
        }
    }

    let Some(ref mut stdin) = child.stdin else {
        log_to_file("pipeline stdin is closed (None)");
        stop_local_pipeline_inner(&state);
        return Err("Local pipeline stdin is closed".to_string());
    };

    if let Err(e) = stdin.write_all(&data) {
        let raw = e.raw_os_error();
        let kind = e.kind();
        log_to_file(&format!("stdin write error: {} (kind={:?}, raw={:?})", e, kind, raw));

        // Windows: ERROR_NO_DATA (232) => "The pipe is being closed."
        if kind == std::io::ErrorKind::BrokenPipe || raw == Some(232) {
            stop_local_pipeline_inner(&state);
            return Err("Local pipeline disconnected (stdin pipe closed)".to_string());
        }

        return Err(e.to_string());
    }

    stdin.flush().map_err(|e| {
        log_to_file(&format!("stdin flush error: {}", e));
        e.to_string()
    })?;

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

/// Check if local model setup is complete
#[tauri::command]
pub fn check_mlx_setup() -> Result<String, String> {
    let marker = get_setup_marker();
    let venv_python = get_venv_python();

    if std::path::Path::new(&marker).exists() && std::path::Path::new(&venv_python).exists() {
        let content = std::fs::read_to_string(&marker).unwrap_or_default();
        Ok(format!(r#"{{"ready":true,"python":"{}","details":{}}}"#, venv_python, content))
    } else {
        Ok(r#"{"ready":false}"#.to_string())
    }
}

/// Run local model setup (install venv + packages + download models)
#[tauri::command]
pub fn run_mlx_setup(
    channel: Channel<String>,
) -> Result<(), String> {
    log_to_file("run_mlx_setup called");

    let script_path = find_setup_script()?;
    let python = find_system_python();

    log_to_file(&format!("Using python: {}, script: {:?}", python, script_path));

    let mut cmd = Command::new(&python);
    cmd.arg(&script_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
    }

    let mut child = cmd.spawn()
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
                    let _ = channel_clone2.send(
                        format!(r#"{{"type":"log","message":"{}"}}"#, escaped)
                    );
                }
                Err(_) => break,
            }
        }
    });

    Ok(())
}
