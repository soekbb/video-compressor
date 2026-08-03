use serde::Serialize;
use std::env::consts::{ARCH, OS};
use std::process::Command;
use std::thread;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
  pub os: String,
  pub arch: String,
  pub cpu_brand: Option<String>,
  pub cpu_cores: u32,
  pub total_memory_bytes: Option<u64>,
}

fn cpu_cores() -> u32 {
  thread::available_parallelism()
    .map(|n| n.get() as u32)
    .unwrap_or(1)
    .max(1)
}

#[cfg(target_os = "macos")]
fn sysctl_n(key: &str) -> Option<String> {
  let output = Command::new("sysctl")
    .args(["-n", key])
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
  if value.is_empty() {
    None
  } else {
    Some(value)
  }
}

#[cfg(target_os = "macos")]
fn cpu_brand() -> Option<String> {
  sysctl_n("machdep.cpu.brand_string")
}

#[cfg(target_os = "macos")]
fn total_memory_bytes() -> Option<u64> {
  sysctl_n("hw.memsize")?.parse().ok()
}

#[cfg(target_os = "linux")]
fn cpu_brand() -> Option<String> {
  let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
  for line in text.lines() {
    if let Some(value) = line.strip_prefix("model name") {
      let brand = value.trim().trim_start_matches(':').trim();
      if !brand.is_empty() {
        return Some(brand.to_string());
      }
    }
  }
  None
}

#[cfg(target_os = "linux")]
fn total_memory_bytes() -> Option<u64> {
  let text = std::fs::read_to_string("/proc/meminfo").ok()?;
  for line in text.lines() {
    if let Some(rest) = line.strip_prefix("MemTotal:") {
      let kb: u64 = rest
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
      return Some(kb.saturating_mul(1024));
    }
  }
  None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cpu_brand() -> Option<String> {
  None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn total_memory_bytes() -> Option<u64> {
  None
}

fn display_os(os: &str) -> String {
  match os {
    "macos" => "macOS".into(),
    "linux" => "Linux".into(),
    "windows" => "Windows".into(),
    other => other.into(),
  }
}

#[tauri::command]
pub fn get_system_info() -> SystemInfo {
  SystemInfo {
    os: display_os(OS),
    arch: ARCH.to_string(),
    cpu_brand: cpu_brand(),
    cpu_cores: cpu_cores(),
    total_memory_bytes: total_memory_bytes(),
  }
}
