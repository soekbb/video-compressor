use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionPreset {
  P720,
  P1080,
}

pub fn parse_resolution_preset(value: &str, fallback: ResolutionPreset) -> ResolutionPreset {
  match value.trim() {
    "720" => ResolutionPreset::P720,
    "1080" => ResolutionPreset::P1080,
    _ => fallback,
  }
}

pub fn target_canvas(preset: ResolutionPreset, width: u32, height: u32) -> (u32, u32) {
  let portrait = height > width;
  match (preset, portrait) {
    (ResolutionPreset::P720, true) => (720, 1280),
    (ResolutionPreset::P720, false) => (1280, 720),
    (ResolutionPreset::P1080, true) => (1080, 1920),
    (ResolutionPreset::P1080, false) => (1920, 1080),
  }
}

pub fn letterbox_vf(target_w: u32, target_h: u32) -> String {
  format!(
    "scale=w='trunc(iw*min({tw}/iw\\,{th}/ih)/2)*2':h='trunc(ih*min({tw}/iw\\,{th}/ih)/2)*2',pad={tw}:{th}:(ow-iw)/2:(oh-ih)/2:black,setsar=1",
    tw = target_w,
    th = target_h,
  )
}

pub fn majority_portrait(sizes: &[(u32, u32)]) -> bool {
  if sizes.is_empty() {
    return true;
  }
  let portrait = sizes.iter().filter(|(w, h)| *h > *w).count();
  let landscape = sizes.len() - portrait;
  if portrait > landscape {
    true
  } else if landscape > portrait {
    false
  } else {
    sizes[0].1 > sizes[0].0
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressAction {
  Skip,
  RemuxFaststart,
  Reencode,
}

pub fn decide_compress(
  h264: bool,
  src_w: u32,
  src_h: u32,
  target_w: u32,
  target_h: u32,
  moov_before_mdat: bool,
) -> CompressAction {
  if h264 && src_w == target_w && src_h == target_h {
    if moov_before_mdat {
      CompressAction::Skip
    } else {
      CompressAction::RemuxFaststart
    }
  } else {
    CompressAction::Reencode
  }
}

pub fn moov_is_before_mdat(path: &Path) -> bool {
  let Ok(mut f) = fs::File::open(path) else {
    return false;
  };
  let Ok(meta) = f.metadata() else {
    return false;
  };
  let file_len = meta.len();
  let mut off = 0u64;
  let mut seen_moov: Option<u64> = None;
  let mut seen_mdat: Option<u64> = None;
  let mut hdr = [0u8; 8];
  for _ in 0..16 {
    if off + 8 > file_len {
      break;
    }
    if f.read_exact(&mut hdr).is_err() {
      break;
    }
    let size = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
    let kind = &hdr[4..8];
    let box_size = if size == 1 {
      let mut wide = [0u8; 8];
      if f.read_exact(&mut wide).is_err() {
        break;
      }
      u64::from_be_bytes(wide)
    } else if size == 0 {
      file_len - off
    } else {
      size
    };
    if box_size < 8 {
      break;
    }
    if kind == b"moov" {
      seen_moov = Some(off);
    }
    if kind == b"mdat" {
      seen_mdat = Some(off);
    }
    if seen_moov.is_some() && seen_mdat.is_some() {
      break;
    }
    off = off.saturating_add(box_size);
    if std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(off)).is_err() {
      break;
    }
  }
  match (seen_moov, seen_mdat) {
    (Some(moov), Some(mdat)) => moov < mdat,
    _ => false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_resolution_preset_accepts_720_1080() {
    assert_eq!(
      parse_resolution_preset("720", ResolutionPreset::P1080),
      ResolutionPreset::P720
    );
    assert_eq!(
      parse_resolution_preset("1080", ResolutionPreset::P720),
      ResolutionPreset::P1080
    );
  }

  #[test]
  fn parse_resolution_preset_falls_back() {
    assert_eq!(
      parse_resolution_preset("4k", ResolutionPreset::P720),
      ResolutionPreset::P720
    );
    assert_eq!(
      parse_resolution_preset("", ResolutionPreset::P1080),
      ResolutionPreset::P1080
    );
    assert_eq!(
      parse_resolution_preset("720p", ResolutionPreset::P1080),
      ResolutionPreset::P1080
    );
  }

  #[test]
  fn target_canvas_portrait_and_landscape() {
    assert_eq!(
      target_canvas(ResolutionPreset::P720, 1080, 1920),
      (720, 1280)
    );
    assert_eq!(
      target_canvas(ResolutionPreset::P1080, 1080, 1920),
      (1080, 1920)
    );
    assert_eq!(
      target_canvas(ResolutionPreset::P720, 1920, 1080),
      (1280, 720)
    );
    assert_eq!(
      target_canvas(ResolutionPreset::P1080, 1920, 1080),
      (1920, 1080)
    );
    assert_eq!(
      target_canvas(ResolutionPreset::P1080, 100, 100),
      (1920, 1080)
    );
  }

  #[test]
  fn letterbox_vf_fits_inside_and_pads() {
    let vf = letterbox_vf(720, 1280);
    assert!(vf.contains("min(720/iw\\,1280/ih)"), "{vf}");
    assert!(vf.contains("pad=720:1280:(ow-iw)/2:(oh-ih)/2"), "{vf}");
    assert!(vf.contains("setsar=1"), "{vf}");
    assert!(!vf.contains("force_original_aspect_ratio"), "{vf}");
  }

  #[test]
  fn majority_portrait_counts_then_ties_to_first() {
    assert!(majority_portrait(&[(1080, 1920), (1080, 1920), (1920, 1080)]));
    assert!(!majority_portrait(&[(1920, 1080), (1280, 720), (720, 1280)]));
    assert!(majority_portrait(&[(720, 1280), (1920, 1080)]));
    assert!(!majority_portrait(&[(1920, 1080), (720, 1280)]));
    assert!(majority_portrait(&[]));
  }

  use std::fs;
  use std::io::Write;

  fn write_boxes(path: &std::path::Path, boxes: &[(&[u8; 4], u32)]) {
    let mut f = fs::File::create(path).unwrap();
    for (kind, payload) in boxes {
      let size = 8 + payload;
      f.write_all(&size.to_be_bytes()).unwrap();
      f.write_all(*kind).unwrap();
      f.write_all(&vec![0u8; *payload as usize]).unwrap();
    }
  }

  #[test]
  fn moov_is_before_mdat_detects_faststart_and_postponed() {
    let dir = std::env::temp_dir().join(format!(
      "kuaiya-moov-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let front = dir.join("front.mp4");
    write_boxes(&front, &[(b"ftyp", 4), (b"moov", 16), (b"mdat", 32)]);
    let back = dir.join("back.mp4");
    write_boxes(&back, &[(b"ftyp", 4), (b"mdat", 32), (b"moov", 16)]);
    let empty = dir.join("empty.bin");
    fs::write(&empty, [0u8; 3]).unwrap();
    assert!(moov_is_before_mdat(&front));
    assert!(!moov_is_before_mdat(&back));
    assert!(!moov_is_before_mdat(&empty));
    fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn decide_compress_skip_remux_reencode() {
    assert_eq!(
      decide_compress(true, 720, 1280, 720, 1280, true),
      CompressAction::Skip
    );
    assert_eq!(
      decide_compress(true, 720, 1280, 720, 1280, false),
      CompressAction::RemuxFaststart
    );
    assert_eq!(
      decide_compress(false, 720, 1280, 720, 1280, true),
      CompressAction::Reencode
    );
    assert_eq!(
      decide_compress(true, 1080, 1920, 720, 1280, true),
      CompressAction::Reencode
    );
  }
}
