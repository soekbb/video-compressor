use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
  pub id: String,
  #[serde(rename = "type")]
  pub task_type: String,
  pub title: String,
  pub status: String,
  pub progress: u32,
  pub error: Option<String>,
  pub meta: Option<String>,
  pub created_at: String,
  pub updated_at: String,
}

pub struct TaskDb {
  conn: Mutex<Connection>,
}

impl TaskDb {
  pub fn mark_stale_on_boot(&self) -> Result<(), String> {
    let conn = self.conn.lock().map_err(|_| "任务数据库忙碌".to_string())?;
    mark_stale(&conn)
  }
}

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
  let dir = app
    .path()
    .app_data_dir()
    .map_err(|e| format!("无法获取应用数据目录：{e}"))?;
  if !dir.exists() {
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录：{e}"))?;
  }
  Ok(dir.join("tasks.db"))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsRecord {
  pub concurrency: u32,
  pub scan_interval_minutes: u32,
  pub quality_preset: String,
  pub auto_scan_on_launch: bool,
}

fn settings_columns(conn: &Connection) -> Result<Vec<String>, String> {
  let mut stmt = conn
    .prepare("PRAGMA table_info(settings)")
    .map_err(|e| format!("读取设置表结构失败：{e}"))?;
  let cols = stmt
    .query_map([], |row| row.get::<_, String>(1))
    .map_err(|e| format!("读取设置表结构失败：{e}"))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| format!("读取设置表结构失败：{e}"))?;
  Ok(cols)
}

fn migrate_settings_columns(conn: &Connection) -> Result<(), String> {
  let cols = settings_columns(conn)?;
  if !cols.iter().any(|c| c == "quality_preset") {
    conn
      .execute(
        "ALTER TABLE settings ADD COLUMN quality_preset TEXT NOT NULL DEFAULT 'size'",
        [],
      )
      .map_err(|e| format!("升级设置表失败：{e}"))?;
  }
  let cols = settings_columns(conn)?;
  if !cols.iter().any(|c| c == "auto_scan_on_launch") {
    conn
      .execute(
        "ALTER TABLE settings ADD COLUMN auto_scan_on_launch INTEGER NOT NULL DEFAULT 0",
        [],
      )
      .map_err(|e| format!("升级设置表失败：{e}"))?;
  }
  Ok(())
}

fn init_schema(conn: &Connection) -> Result<(), String> {
  // CREATE IF NOT EXISTS 不会升级已有表结构；须先 migrate 再 INSERT
  conn
    .execute_batch(
      r#"
      CREATE TABLE IF NOT EXISTS tasks (
        id TEXT PRIMARY KEY,
        type TEXT NOT NULL,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        progress INTEGER NOT NULL DEFAULT 0,
        error TEXT,
        meta TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_tasks_updated ON tasks(updated_at DESC);

      CREATE TABLE IF NOT EXISTS settings (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        concurrency INTEGER NOT NULL,
        scan_interval_minutes INTEGER NOT NULL,
        quality_preset TEXT NOT NULL DEFAULT 'size',
        auto_scan_on_launch INTEGER NOT NULL DEFAULT 0
      );
      "#,
    )
    .map_err(|e| format!("初始化数据库表失败：{e}"))?;
  migrate_settings_columns(conn)?;
  conn
    .execute(
      "INSERT OR IGNORE INTO settings
         (id, concurrency, scan_interval_minutes, quality_preset, auto_scan_on_launch)
       VALUES (1, 2, 3, 'size', 0)",
      [],
    )
    .map_err(|e| format!("初始化设置失败：{e}"))?;
  Ok(())
}

pub fn open_task_db(app: &AppHandle) -> Result<TaskDb, String> {
  let path = db_path(app)?;
  let conn = Connection::open(&path).map_err(|e| format!("打开任务数据库失败：{e}"))?;
  init_schema(&conn)?;
  Ok(TaskDb {
    conn: Mutex::new(conn),
  })
}

fn mark_stale(conn: &Connection) -> Result<(), String> {
  // 应用异常退出 / 热重载后，进行中的任务无法继续；标为取消并写明原因，避免误以为是压制报错
  conn
    .execute(
      "UPDATE tasks
       SET status = 'cancelled',
           error = '应用重启，任务已中断',
           updated_at = datetime('now','localtime')
       WHERE status IN ('running', 'pending')",
      [],
    )
    .map_err(|e| format!("清理残留任务失败：{e}"))?;
  Ok(())
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
  Ok(TaskRecord {
    id: row.get(0)?,
    task_type: row.get(1)?,
    title: row.get(2)?,
    status: row.get(3)?,
    progress: row.get::<_, i64>(4)? as u32,
    error: row.get(5)?,
    meta: row.get(6)?,
    created_at: row.get(7)?,
    updated_at: row.get(8)?,
  })
}

#[tauri::command]
pub fn list_tasks(db: State<'_, TaskDb>) -> Result<Vec<TaskRecord>, String> {
  let conn = db.conn.lock().map_err(|_| "任务数据库忙碌".to_string())?;
  let mut stmt = conn
    .prepare(
      "SELECT id, type, title, status, progress, error, meta, created_at, updated_at
       FROM tasks ORDER BY updated_at DESC, created_at DESC",
    )
    .map_err(|e| format!("查询任务失败：{e}"))?;
  let rows = stmt
    .query_map([], map_row)
    .map_err(|e| format!("查询任务失败：{e}"))?;
  let mut out = Vec::new();
  for row in rows {
    out.push(row.map_err(|e| format!("读取任务失败：{e}"))?);
  }
  Ok(out)
}

#[tauri::command]
pub fn upsert_task(db: State<'_, TaskDb>, task: TaskRecord) -> Result<(), String> {
  let conn = db.conn.lock().map_err(|_| "任务数据库忙碌".to_string())?;
  conn
    .execute(
      "INSERT INTO tasks (id, type, title, status, progress, error, meta, created_at, updated_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
       ON CONFLICT(id) DO UPDATE SET
         type = excluded.type,
         title = excluded.title,
         status = excluded.status,
         progress = excluded.progress,
         error = excluded.error,
         meta = excluded.meta,
         updated_at = excluded.updated_at",
      params![
        task.id,
        task.task_type,
        task.title,
        task.status,
        task.progress as i64,
        task.error,
        task.meta,
        task.created_at,
        task.updated_at,
      ],
    )
    .map_err(|e| format!("保存任务失败：{e}"))?;
  Ok(())
}

#[tauri::command]
pub fn delete_task(db: State<'_, TaskDb>, id: String) -> Result<(), String> {
  let conn = db.conn.lock().map_err(|_| "任务数据库忙碌".to_string())?;
  conn
    .execute("DELETE FROM tasks WHERE id = ?1", params![id])
    .map_err(|e| format!("删除任务失败：{e}"))?;
  Ok(())
}

#[tauri::command]
pub fn delete_finished_tasks(db: State<'_, TaskDb>) -> Result<u32, String> {
  let conn = db.conn.lock().map_err(|_| "任务数据库忙碌".to_string())?;
  let n = conn
    .execute(
      "DELETE FROM tasks WHERE status IN ('done', 'error', 'cancelled')",
      [],
    )
    .map_err(|e| format!("清空已结束任务失败：{e}"))?;
  Ok(n as u32)
}

#[tauri::command]
pub fn mark_stale_tasks(db: State<'_, TaskDb>) -> Result<(), String> {
  let conn = db.conn.lock().map_err(|_| "任务数据库忙碌".to_string())?;
  mark_stale(&conn)
}

fn clamp_concurrency(n: u32) -> u32 {
  n.clamp(1, 5)
}

fn clamp_interval(n: u32) -> u32 {
  n.clamp(3, 60)
}

fn normalize_quality_preset(v: &str) -> String {
  if v == "quality" {
    "quality".into()
  } else {
    "size".into()
  }
}

#[tauri::command]
pub fn load_settings(db: State<'_, TaskDb>) -> Result<AppSettingsRecord, String> {
  let conn = db.conn.lock().map_err(|_| "任务数据库忙碌".to_string())?;
  // 兼容旧库：表不存在时补建并写入默认值
  init_schema(&conn)?;
  let row = conn
    .query_row(
      "SELECT concurrency, scan_interval_minutes, quality_preset, auto_scan_on_launch
       FROM settings WHERE id = 1",
      [],
      |r| {
        Ok(AppSettingsRecord {
          concurrency: clamp_concurrency(r.get::<_, i64>(0)? as u32),
          scan_interval_minutes: clamp_interval(r.get::<_, i64>(1)? as u32),
          quality_preset: normalize_quality_preset(&r.get::<_, String>(2)?),
          auto_scan_on_launch: r.get::<_, i64>(3)? != 0,
        })
      },
    )
    .map_err(|e| format!("读取设置失败：{e}"))?;
  Ok(row)
}

#[tauri::command]
pub fn save_settings(db: State<'_, TaskDb>, settings: AppSettingsRecord) -> Result<(), String> {
  let conn = db.conn.lock().map_err(|_| "任务数据库忙碌".to_string())?;
  init_schema(&conn)?;
  let preset = normalize_quality_preset(&settings.quality_preset);
  conn
    .execute(
      "INSERT INTO settings
         (id, concurrency, scan_interval_minutes, quality_preset, auto_scan_on_launch)
       VALUES (1, ?1, ?2, ?3, ?4)
       ON CONFLICT(id) DO UPDATE SET
         concurrency = excluded.concurrency,
         scan_interval_minutes = excluded.scan_interval_minutes,
         quality_preset = excluded.quality_preset,
         auto_scan_on_launch = excluded.auto_scan_on_launch",
      params![
        clamp_concurrency(settings.concurrency) as i64,
        clamp_interval(settings.scan_interval_minutes) as i64,
        preset,
        if settings.auto_scan_on_launch { 1_i64 } else { 0_i64 },
      ],
    )
    .map_err(|e| format!("保存设置失败：{e}"))?;
  Ok(())
}
