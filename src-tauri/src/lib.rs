mod compress;
mod media;
mod persist;
mod tasks;

use compress::{cancel_compress, compress_video, prepare_compress_batch, CompressState};
use media::{list_drama_folders, merge_videos};
use persist::{load_auto_state, save_auto_state};
use std::sync::Arc;
use tauri::Manager;
use tasks::{
  delete_finished_tasks, delete_task, list_tasks, load_settings, mark_stale_tasks, open_task_db,
  save_settings, upsert_task,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_fs::init())
    .manage(Arc::new(CompressState::default()))
    .invoke_handler(tauri::generate_handler![
      compress_video,
      prepare_compress_batch,
      cancel_compress,
      merge_videos,
      list_drama_folders,
      load_auto_state,
      save_auto_state,
      list_tasks,
      upsert_task,
      delete_task,
      delete_finished_tasks,
      mark_stale_tasks,
      load_settings,
      save_settings
    ])
    .setup(|app| {
      let db = open_task_db(app.handle())?;
      db.mark_stale_on_boot()?;
      app.manage(db);

      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
