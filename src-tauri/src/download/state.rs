use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub index: u32,
    pub start: u64,
    pub end: u64,
    pub cursor: u64,
}

impl Segment {
    pub fn done(&self) -> bool {
        self.cursor > self.end
    }

    pub fn downloaded(&self) -> u64 {
        if self.cursor <= self.start {
            0
        } else {
            (self.cursor - self.start).min(self.end - self.start + 1)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadState {
    pub url: String,
    pub total: u64,
    pub threads: u32,
    pub segments: Vec<Segment>,
}

impl DownloadState {
    pub fn completed_bytes(&self) -> u64 {
        self.segments.iter().map(Segment::downloaded).sum()
    }

    pub fn all_done(&self) -> bool {
        !self.segments.is_empty() && self.segments.iter().all(Segment::done)
    }
}

pub fn part_path(final_path: &Path) -> PathBuf {
    let mut p = final_path.as_os_str().to_owned();
    p.push(".part");
    PathBuf::from(p)
}

pub fn state_path(final_path: &Path) -> PathBuf {
    let mut p = final_path.as_os_str().to_owned();
    p.push(".lapdwstate");
    PathBuf::from(p)
}

pub fn url_marker_path(final_path: &Path) -> PathBuf {
    let mut p = final_path.as_os_str().to_owned();
    p.push(".lapdwurl");
    PathBuf::from(p)
}

pub fn build_segments(total: u64, threads: u32) -> Vec<Segment> {
    let threads = threads.max(1) as u64;
    let chunk = total / threads;
    let mut segs = Vec::new();
    let mut start = 0u64;
    for i in 0..threads {
        let end = if i == threads - 1 {
            total.saturating_sub(1)
        } else {
            (start + chunk).saturating_sub(1).min(total.saturating_sub(1))
        };
        if start > end {
            break;
        }
        segs.push(Segment {
            index: i as u32,
            start,
            end,
            cursor: start,
        });
        start = end + 1;
        if start >= total {
            break;
        }
    }
    segs
}

pub fn load_state(path: &Path) -> Option<DownloadState> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_state(path: &Path, state: &DownloadState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("create state tmp: {e}"))?;
        f.write_all(json.as_bytes())
            .map_err(|e| format!("write state: {e}"))?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path).map_err(|e| format!("rename state: {e}"))
}

pub fn write_url_marker(path: &Path, url: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::write(path, url.trim()).map_err(|e| format!("write url marker: {e}"))
}

pub fn read_url_marker(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn choose_available_path(dir: &Path, filename: &str, url: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() && !part_path(&candidate).exists() {
        return candidate;
    }
    if let Some(stored) = read_url_marker(&url_marker_path(&candidate)) {
        if stored == url {
            return candidate;
        }
    } else if part_path(&candidate).exists() {
        if let Some(st) = load_state(&state_path(&candidate)) {
            if st.url == url {
                return candidate;
            }
        }
    } else if candidate.exists() {
        // Existing finished file without marker: treat as same if we can't tell — rename to be safe
        // unless marker missing and we assume conflict.
    }

    let (stem, ext) = split_name(filename);
    for i in 1..10_000 {
        let name = if ext.is_empty() {
            format!("{stem}({i})")
        } else {
            format!("{stem}({i}).{ext}")
        };
        let path = dir.join(&name);
        if !path.exists() && !part_path(&path).exists() {
            return path;
        }
        if let Some(stored) = read_url_marker(&url_marker_path(&path)) {
            if stored == url {
                return path;
            }
        }
    }
    dir.join(format!("{stem}.conflict"))
}

fn split_name(filename: &str) -> (String, String) {
    if let Some((stem, ext)) = filename.rsplit_once('.') {
        if !stem.is_empty() && !ext.is_empty() && !ext.contains('/') {
            return (stem.to_string(), ext.to_string());
        }
    }
    (filename.to_string(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn segments_cover_total() {
        let segs = build_segments(1000, 4);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[3].end, 999);
        let covered: u64 = segs.iter().map(|s| s.end - s.start + 1).sum();
        assert_eq!(covered, 1000);
    }

    #[test]
    fn choose_path_renames_on_conflict() {
        let dir = std::env::temp_dir().join(format!("lapdw-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("file.bin");
        fs::write(&a, b"x").unwrap();
        write_url_marker(&url_marker_path(&a), "https://a.example/file.bin").unwrap();
        let path = choose_available_path(&dir, "file.bin", "https://b.example/file.bin");
        assert!(path.file_name().unwrap().to_string_lossy().contains("(1)"));
        let _ = fs::remove_dir_all(&dir);
    }
}
