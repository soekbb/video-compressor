mod compress;
mod encode;
mod media;
mod persist;
mod replace;
mod reveal;
mod sysinfo;
mod tasks;

use compress::{
  cancel_compress, cleanup_orphan_ffmpeg, compress_video, is_compressed_output_valid,
  prepare_compress_batch, CompressState,
};
use media::{list_drama_folders, merge_videos, probe_video_dimensions};
use persist::{load_auto_state, save_auto_state};
use reveal::reveal_path;
use std::sync::Arc;
use sysinfo::get_system_info;
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
    .plugin(tauri_plugin_notification::init())
    .manage(Arc::new(CompressState::default()))
    .invoke_handler(tauri::generate_handler![
      compress_video,
      is_compressed_output_valid,
      prepare_compress_batch,
      cancel_compress,
      merge_videos,
      probe_video_dimensions,
      list_drama_folders,
      load_auto_state,
      save_auto_state,
      list_tasks,
      upsert_task,
      delete_task,
      delete_finished_tasks,
      mark_stale_tasks,
      load_settings,
      save_settings,
      reveal_path,
      get_system_info
    ])
    .setup(|app| {
      let db = open_task_db(app.handle())?;
      db.mark_stale_on_boot()?;
      // 热重载/崩溃后 Child 可能来不及 kill_on_drop，清理孤儿 FFmpeg
      cleanup_orphan_ffmpeg(app.handle());
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
