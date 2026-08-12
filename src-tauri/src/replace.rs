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

pub fn replace_file_force(temp: &Path, final_path: &Path) -> Result<(), String> {
  if try_replace(temp, final_path).is_ok() {
    return Ok(());
  }
  let pids = pids_holding(final_path);
  for pid in &pids {
    kill_pid(*pid);
  }
  std::thread::sleep(std::time::Duration::from_millis(200));
  match try_replace(temp, final_path) {
    Ok(()) => Ok(()),
    Err(e) => {
      let _ = fs::remove_file(temp);
      Err(format!("文件被占用，无法替换原文件：{e}"))
    }
  }
}

fn try_replace(temp: &Path, final_path: &Path) -> Result<(), String> {
  if final_path.exists() {
    fs::remove_file(final_path).map_err(|e| e.to_string())?;
  }
  fs::rename(temp, final_path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn temp_name_is_hidden_sibling() {
    let p = PathBuf::from("/Movies/剧/01.mp4");
    let t = temp_output_path(&p);
    let name = t.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with(".影工临时_01_"));
    assert!(name.ends_with(".mp4"));
    assert_eq!(t.parent(), p.parent());
  }
}
