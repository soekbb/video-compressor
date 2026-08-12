use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn temp_output_path(final_path: &Path) -> PathBuf {
  let parent = final_path.parent().unwrap_or_else(|| Path::new("."));
  let stem = final_path
    .file_stem()
    .and_then(|s| s.to_str())
    .unwrap_or("video");
  let ext = final_path
    .extension()
    .and_then(|s| s.to_str())
    .map(|e| format!(".{e}"))
    .unwrap_or_default();
  let id = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis())
    .unwrap_or(0);
  parent.join(format!(".影工临时_{stem}_{id}{ext}"))
}

fn pids_holding(path: &Path) -> Vec<u32> {
  let path_str = path.to_string_lossy();
  let output = Command::new("lsof")
    .args(["-t", "--", path_str.as_ref()])
    .output();
  let Ok(out) = output else {
    return vec![];
  };
  if !out.status.success() && out.stdout.is_empty() {
    return vec![];
  }
  String::from_utf8_lossy(&out.stdout)
    .lines()
    .filter_map(|l| l.trim().parse::<u32>().ok())
    .filter(|pid| *pid != std::process::id())
    .collect()
}

fn kill_pid(pid: u32) {
  #[cfg(unix)]
  {
    let _ = Command::new("kill").args([pid.to_string()]).output();
  }
  #[cfg(windows)]
  {
    let _ = Command::new("taskkill")
      .args(["/PID", &pid.to_string(), "/F"])
      .output();
  }
}

fn looks_occupied(err: &str) -> bool {
  let lower = err.to_lowercase();
  lower.contains("busy")
    || lower.contains("sharing")
    || lower.contains("being used")
    || lower.contains("resource busy")
    || lower.contains("os error 16") // EBUSY
    || lower.contains("os error 26") // ETXTBSY
    || lower.contains("os error 32") // ERROR_SHARING_VIOLATION
    || lower.contains("os error 33") // ERROR_LOCK_VIOLATION
    || lower.contains("access is denied")
    || err.contains("文件被占用")
}

fn format_replace_error(err: &str, occupied: bool) -> String {
  if occupied || looks_occupied(err) {
    format!("文件被占用，无法替换原文件：{err}")
  } else {
    format!("无法替换原文件：{err}")
  }
}

/// On failure: drop temp only when the original still exists.
/// If final is gone and temp remains, keep temp as the playable survivor.
fn cleanup_temp_after_failure(temp: &Path, final_path: &Path) {
  if final_path.exists() {
    let _ = fs::remove_file(temp);
  }
}

pub fn replace_file_force(temp: &Path, final_path: &Path) -> Result<(), String> {
  match try_replace(temp, final_path) {
    Ok(()) => return Ok(()),
    Err(first_err) => {
      let pids = pids_holding(final_path);
      if pids.is_empty() {
        cleanup_temp_after_failure(temp, final_path);
        return Err(format_replace_error(&first_err, false));
      }
      for pid in &pids {
        kill_pid(*pid);
      }
      std::thread::sleep(std::time::Duration::from_millis(200));
      match try_replace(temp, final_path) {
        Ok(()) => Ok(()),
        Err(e) => {
          cleanup_temp_after_failure(temp, final_path);
          Err(format_replace_error(&e, true))
        }
      }
    }
  }
}

fn try_replace(temp: &Path, final_path: &Path) -> Result<(), String> {
  // Windows cannot rename over an existing file; Unix rename is atomic over existing.
  #[cfg(windows)]
  {
    if final_path.exists() {
      fs::remove_file(final_path).map_err(|e| e.to_string())?;
    }
  }
  fs::rename(temp, final_path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn unique_temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "video-compressor-replace-{}-{}-{}",
      label,
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[test]
  fn temp_name_is_hidden_sibling() {
    let p = PathBuf::from("/Movies/剧/01.mp4");
    let t = temp_output_path(&p);
    let name = t.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with(".影工临时_01_"));
    assert!(name.ends_with(".mp4"));
    assert_eq!(t.parent(), p.parent());
  }

  /// Missing temp must not destroy an existing final (Unix: no pre-delete before rename).
  #[cfg(unix)]
  #[test]
  fn failed_replace_preserves_existing_final_when_temp_missing() {
    let dir = unique_temp_dir("preserve-final");
    let final_path = dir.join("01.mp4");
    let temp = dir.join(".影工临时_01_1.mp4");
    fs::write(&final_path, b"original").unwrap();
    assert!(!temp.exists());

    let err = replace_file_force(&temp, &final_path).unwrap_err();
    assert!(
      final_path.is_file(),
      "final must survive failed replace; err={err}"
    );
    assert_eq!(fs::read(&final_path).unwrap(), b"original");
    fs::remove_dir_all(dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn keeps_temp_when_final_missing_after_failure() {
    let dir = unique_temp_dir("keep-temp");
    let final_path = dir.join("missing").join("01.mp4");
    let temp = dir.join(".影工临时_01_1.mp4");
    fs::write(&temp, b"compressed").unwrap();

    let err = replace_file_force(&temp, &final_path).unwrap_err();
    assert!(
      temp.is_file(),
      "temp must remain when final absent; err={err}"
    );
    assert!(!final_path.exists());
    assert_eq!(fs::read(&temp).unwrap(), b"compressed");
    fs::remove_dir_all(dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn unix_rename_over_existing_replaces_content() {
    let dir = unique_temp_dir("rename-over");
    let final_path = dir.join("01.mp4");
    let temp = dir.join(".影工临时_01_1.mp4");
    fs::write(&final_path, b"original").unwrap();
    fs::write(&temp, b"compressed").unwrap();

    replace_file_force(&temp, &final_path).unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), b"compressed");
    assert!(!temp.exists());
    fs::remove_dir_all(dir).unwrap();
  }
}
