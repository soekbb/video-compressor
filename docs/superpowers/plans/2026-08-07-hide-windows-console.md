# Hide Windows FFmpeg Console Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent FFmpeg and FFprobe from creating visible console windows when the Windows Tauri application runs a task.

**Architecture:** Centralize Windows process creation configuration in `compress.rs`, where the application already owns FFmpeg process helpers. The helper applies `CREATE_NO_WINDOW` only for Windows builds; all FFmpeg/FFprobe command construction in `compress.rs`, `encode.rs`, and `media.rs` calls it immediately after creating its `Command`.

**Tech Stack:** Rust 2021, `std::process::Command`, Windows `CommandExt`, Tauri 2.

## Global Constraints

- Use `CREATE_NO_WINDOW` with the exact value `0x08000000` on Windows.
- Preserve existing command arguments, process pipes, cancellation behavior, encoding fallback, and non-Windows behavior.
- Cover FFmpeg and FFprobe checks, metadata probes, encoder probes, compression, and merge subprocesses.
- Do not hide or alter the Unix-only `pkill` cleanup command.

---

### Task 1: Centralize hidden-window configuration and apply it everywhere

**Files:**
- Modify: `src-tauri/src/compress.rs:1-7, 86-102, 156-166, 249-263, 383-408`
- Modify: `src-tauri/src/encode.rs:1-4, 69-84`
- Modify: `src-tauri/src/media.rs:1-15, 81-102, 281-292, 451-465, 562-636`
- Modify: `docs/superpowers/specs/2026-08-07-hide-windows-console-design.md:23-25`

**Interfaces:**
- Produces: `pub fn configure_subprocess(command: &mut std::process::Command)`.
- Consumes: `std::os::windows::process::CommandExt::creation_flags` only under `#[cfg(windows)]`.
- Consumed by: every construction of a FFmpeg or FFprobe `Command` in `compress`, `encode`, and `media`.

- [ ] **Step 1: Write the failing Windows-only compilation test**

Add this test at the end of `src-tauri/src/compress.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use std::process::Command;

  #[cfg(windows)]
  #[test]
  fn configures_a_windows_subprocess() {
    let mut command = Command::new("cmd");
    configure_subprocess(&mut command);
  }
}
```

- [ ] **Step 2: Run the test to verify it fails before implementation**

Run on a Windows Rust toolchain:

```bash
cargo test --manifest-path src-tauri/Cargo.toml configures_a_windows_subprocess
```

Expected: compilation fails because `configure_subprocess` is not defined.

- [ ] **Step 3: Add the cross-platform process configuration helper**

Add after the process imports in `src-tauri/src/compress.rs`:

```rust
/// Suppress console windows for console subprocesses launched by the Windows GUI app.
pub fn configure_subprocess(command: &mut Command) {
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
  }

  #[cfg(not(windows))]
  let _ = command;
}
```

- [ ] **Step 4: Apply the helper to every FFmpeg/FFprobe command**

For each `Command::new(...)` targeting FFmpeg or FFprobe, initialize a mutable command, call `configure_subprocess(&mut command)`, then retain the existing builder chain. For example, change a probe from:

```rust
let output = Command::new(ffprobe)
  .args([...])
  .output()?;
```

to:

```rust
let mut command = Command::new(ffprobe);
configure_subprocess(&mut command);
let output = command
  .args([...])
  .output()?;
```

Import the helper in `encode.rs` and `media.rs`:

```rust
use crate::compress::configure_subprocess;
```

Apply this pattern to the FFmpeg/FFprobe executable checks, stream/duration/resolution/encoder probes, compression process, and merge process. Leave `Command::new("pkill")` untouched.

- [ ] **Step 5: Update the validation criteria in the design specification**

Replace the validation paragraph with:

```markdown
Add a Windows-only compilation test for the subprocess configuration helper. Run Rust formatting and tests; on a Windows build, run a compression, a merge, and a media scan to confirm no console window appears and all tasks finish normally.
```

- [ ] **Step 6: Run the test to verify it passes**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all Rust tests pass on the host platform. On Windows, `configures_a_windows_subprocess` compiles and passes.

- [ ] **Step 7: Format and validate all affected Rust code**

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: both commands exit successfully without warnings or errors.

- [ ] **Step 8: Manually validate a Windows packaged build**

Build and open the Windows application. Execute one compression task, one multi-file merge task, and a media scan. Confirm no FFmpeg/FFprobe console window appears, progress continues, cancellation still works, and each successful task produces the expected output.

- [ ] **Step 9: Commit the implementation**

```bash
git add src-tauri/src/compress.rs src-tauri/src/encode.rs src-tauri/src/media.rs docs/superpowers/specs/2026-08-07-hide-windows-console-design.md
git commit -m "fix: hide ffmpeg console windows on Windows"
```
