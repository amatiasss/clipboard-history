use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
};

const MAX_ENTRIES: usize = 200;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct History {
    pub entries: Vec<String>,
}

pub fn history_path() -> PathBuf {
    let mut path = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("clipboard-history");
    fs::create_dir_all(&path).ok();
    path.push("history.json");
    path
}

pub fn private_mode_path() -> PathBuf {
    let mut path = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("clipboard-history");
    path.push(".private");
    path
}

pub fn is_private_mode() -> bool {
    private_mode_path().exists()
}

pub fn load_history(path: &PathBuf) -> History {
    let file = fs::OpenOptions::new().read(true).write(true).create(true).open(path);
    match file {
        Ok(f) => {
            f.lock_exclusive().ok();
            let result = fs::read_to_string(path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(History { entries: vec![] });
            f.unlock().ok();
            result
        }
        Err(_) => History { entries: vec![] },
    }
}

pub fn save_history(path: &PathBuf, history: &mut History) {
    if history.entries.len() > MAX_ENTRIES {
        let drain_count = history.entries.len() - MAX_ENTRIES;
        history.entries.drain(0..drain_count);
    }
    let file = fs::OpenOptions::new().write(true).create(true).open(path);
    if let Ok(f) = file {
        f.lock_exclusive().ok();
        if let Ok(json) = serde_json::to_string_pretty(&*history) {
            fs::write(path, json).ok();
        }
        f.unlock().ok();
    }
}

fn main() {
    let path = history_path();

    println!("Monitoring clipboard... Press Ctrl+C to stop.");

    let tmp = std::env::temp_dir().join("clipboard-daemon-entry.txt");
    let script = format!("wl-paste > {}; echo x", tmp.display());

    let mut cmd = Command::new("wl-paste");
    cmd.args(["--watch", "sh", "-c", &script])
        .stdout(Stdio::piped());

    for var in ["WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DISPLAY"] {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }

    let mut child = cmd.spawn()
        .expect("Failed to start wl-paste. Is wl-clipboard installed?");

    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        match line {
            Ok(_) => {
                if is_private_mode() {
                    continue;
                }
                let text = fs::read_to_string(&tmp)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    // relê do disco para respeitar deleções feitas pelo applet
                    let mut history = load_history(&path);
                    if history.entries.last().map(|e| e.as_str()) != Some(&text) {
                        println!("Captured: {}", &text[..text.len().min(80)]);
                        history.entries.push(text);
                        save_history(&path, &mut history);
                    }
                }
            }
            Err(e) => eprintln!("read error: {e}"),
        }
    }

    let status = child.wait().unwrap();
    eprintln!("wl-paste exited with: {status}");
    eprintln!("WAYLAND_DISPLAY={:?}", std::env::var("WAYLAND_DISPLAY"));
    eprintln!("XDG_RUNTIME_DIR={:?}", std::env::var("XDG_RUNTIME_DIR"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("clipboard-history-test-{}-{}.json", std::process::id(), id))
    }

    #[test]
    fn load_returns_empty_when_file_missing() {
        let path = tmp_path();
        let history = load_history(&path);
        assert!(history.entries.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let path = tmp_path();
        let mut original = History {
            entries: vec!["hello".into(), "world".into()],
        };
        save_history(&path, &mut original);
        let loaded = load_history(&path);
        assert_eq!(original, loaded);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn load_ignores_invalid_json() {
        let path = tmp_path();
        fs::write(&path, "not valid json").unwrap();
        let history = load_history(&path);
        assert!(history.entries.is_empty());
        fs::remove_file(&path).ok();
    }

    #[test]
    fn save_persists_multiple_entries() {
        let path = tmp_path();
        let mut history = History { entries: vec![] };
        for text in ["first", "second", "third"] {
            history.entries.push(text.into());
            save_history(&path, &mut history);
        }
        let loaded = load_history(&path);
        assert_eq!(loaded.entries, vec!["first", "second", "third"]);
        fs::remove_file(&path).ok();
    }
}
