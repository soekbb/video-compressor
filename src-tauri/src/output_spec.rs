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
}
