use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoVideoFailure {
  pub name: String,
  pub reason: String,
  pub at: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoRecord {
  pub path: String,
  pub name: String,
  pub completed_at: String,
  pub video_count: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub video_names: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub failures: Option<Vec<AutoVideoFailure>>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoPersistState {
  pub watch_dir: String,
  pub enabled: bool,
  pub done: Vec<AutoRecord>,
}

fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
  let dir = app
    .path()
    .app_data_dir()
    .map_err(|e| format!("无法获取应用数据目录：{e}"))?;
  if !dir.exists() {
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录：{e}"))?;
  }
  Ok(dir.join("auto-state.json"))
}

#[tauri::command]
pub fn load_auto_state(app: AppHandle) -> Result<AutoPersistState, String> {
  let path = state_path(&app)?;
  if !path.is_file() {
    return Ok(AutoPersistState::default());
  }
  let raw = fs::read_to_string(&path).map_err(|e| format!("读取自动压制记录失败：{e}"))?;
  serde_json::from_str(&raw).map_err(|e| format!("解析自动压制记录失败：{e}"))
}

#[tauri::command]
pub fn save_auto_state(app: AppHandle, state: AutoPersistState) -> Result<(), String> {
  let path = state_path(&app)?;
  let raw =
    serde_json::to_string_pretty(&state).map_err(|e| format!("序列化自动压制记录失败：{e}"))?;
  fs::write(&path, raw).map_err(|e| format!("写入自动压制记录失败：{e}"))?;
  Ok(())
}
