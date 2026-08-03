use std::path::Path;
use std::process::Command;

/// 在系统文件管理器中打开目录，或定位到文件。
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
  let target = Path::new(&path);
  if !target.exists() {
    return Err(format!("路径不存在：{path}"));
  }

  #[cfg(target_os = "macos")]
  {
    let mut cmd = Command::new("open");
    if target.is_file() {
      cmd.args(["-R", &path]);
    } else {
      cmd.arg(&path);
    }
    cmd.spawn().map_err(|e| format!("打开失败：{e}"))?;
    Ok(())
  }

  #[cfg(target_os = "windows")]
  {
    if target.is_file() {
      Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn()
        .map_err(|e| format!("打开失败：{e}"))?;
    } else {
      Command::new("explorer")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("打开失败：{e}"))?;
    }
    Ok(())
  }

  #[cfg(all(unix, not(target_os = "macos")))]
  {
    let open_target = if target.is_file() {
      target
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone())
    } else {
      path.clone()
    };
    Command::new("xdg-open")
      .arg(&open_target)
      .spawn()
      .map_err(|e| format!("打开失败：{e}"))?;
    Ok(())
  }
}
