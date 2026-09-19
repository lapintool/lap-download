use super::client::{apply_auth, build_client};
use super::filename::filename_from_headers;
use super::state::{
    DownloadState, Segment, build_segments, choose_available_path, load_state, part_path,
    save_state, state_path, url_marker_path, write_url_marker,
};
use crate::settings::Settings;
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub task_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub speed_bps: u64,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub task_id: String,
    pub url: String,
    pub save_dir: PathBuf,
    pub preferred_name: Option<String>,
    pub sha256: Option<String>,
    pub settings: Settings,
}

pub struct DownloadResult {
    pub path: PathBuf,
    pub skipped: bool,
    pub resumed: bool,
    pub verified: bool,
}

type ProgressCb = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

pub async fn download_file(
    opts: DownloadOptions,
    on_progress: ProgressCb,
    cancel: Arc<AtomicBool>,
) -> Result<DownloadResult, String> {
    let client = build_client(&opts.settings)?;
    fs::create_dir_all(&opts.save_dir)
        .map_err(|e| format!("create save dir {}: {e}", opts.save_dir.display()))?;

    emit(
        &on_progress,
        &opts.task_id,
        0,
        0,
        0,
        "resolving",
    );

    let remote = probe_remote(&client, &opts.settings, &opts.url).await?;
    let filename = opts
        .preferred_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(remote.filename.clone());
    let final_path = choose_available_path(&opts.save_dir, &filename, &opts.url);

    // Already completed?
    if final_path.exists() && !part_path(&final_path).exists() {
        let marker = url_marker_path(&final_path);
        let same = super::state::read_url_marker(&marker)
            .map(|u| u == opts.url)
            .unwrap_or(true);
        if same {
            let mut verified = false;
            if let Some(expect) = &opts.sha256 {
                emit(&on_progress, &opts.task_id, 0, 0, 0, "verifying");
                verify_sha256(&final_path, expect)?;
                verified = true;
            }
            return Ok(DownloadResult {
                path: final_path,
                skipped: true,
                resumed: false,
                verified,
            });
        }
    }

    let part = part_path(&final_path);
    let st_path = state_path(&final_path);
    let resumed = part.exists() && st_path.exists();

    let threads = opts.settings.threads.max(1);
    let use_parallel = remote.size > 0 && remote.supports_range && threads > 1;

    if use_parallel {
        download_parallel(
            &client,
            &opts,
            &remote,
            &final_path,
            &part,
            &st_path,
            threads,
            on_progress.clone(),
            cancel.clone(),
        )
        .await?;
    } else {
        download_sequential(
            &client,
            &opts,
            &remote,
            &final_path,
            &part,
            &st_path,
            on_progress.clone(),
            cancel.clone(),
        )
        .await?;
    }

    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }

    fs::rename(&part, &final_path).map_err(|e| format!("finalize rename: {e}"))?;
    let _ = fs::remove_file(&st_path);
    write_url_marker(&url_marker_path(&final_path), &opts.url)?;

    let mut verified = false;
    if let Some(expect) = &opts.sha256 {
        emit(&on_progress, &opts.task_id, remote.size, remote.size, 0, "verifying");
        verify_sha256(&final_path, expect)?;
        verified = true;
    }

    emit(
        &on_progress,
        &opts.task_id,
        remote.size.max(1),
        remote.size.max(1),
        0,
        "done",
    );

    Ok(DownloadResult {
        path: final_path,
        skipped: false,
        resumed,
        verified,
    })
}

struct RemoteInfo {
    size: u64,
    supports_range: bool,
    filename: String,
    resolved_url: String,
}

async fn probe_remote(
    client: &reqwest::Client,
    settings: &Settings,
    url: &str,
) -> Result<RemoteInfo, String> {
    // Prefer authenticated GET Range 0-0 (works for HF/Civitai).
    let req = apply_auth(client.get(url), settings).header("Range", "bytes=0-0");
    let resp = req.send().await.map_err(|e| format!("probe: {e}"))?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let final_url = resp.url().clone().to_string();

    // Drain a little then drop.
    let _ = resp.bytes().await;

    let mut supports_range = status == StatusCode::PARTIAL_CONTENT;
    let mut size = 0u64;

    if let Some(cr) = headers
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(total) = parse_content_range_total(cr) {
            size = total;
            supports_range = true;
        }
    }
    if size == 0 {
        if let Some(cl) = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
        {
            // For 206 with Content-Length=1, prefer Content-Range.
            if status == StatusCode::OK {
                size = cl;
            }
        }
    }
    if !supports_range {
        if let Some(ar) = headers
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
        {
            if ar.to_ascii_lowercase().contains("bytes") {
                supports_range = true;
            }
        }
    }

    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(format!(
            "probe failed: {status} (check Hugging Face token / cookie in settings)"
        ));
    }
    if !(status.is_success() || status == StatusCode::PARTIAL_CONTENT) {
        return Err(format!("probe failed: {status}"));
    }

    let filename = filename_from_headers(&headers, &final_url);
    Ok(RemoteInfo {
        size,
        supports_range,
        filename,
        resolved_url: final_url,
    })
}

fn parse_content_range_total(header: &str) -> Option<u64> {
    // bytes 0-0/12345
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() != 2 || !parts[0].eq_ignore_ascii_case("bytes") {
        return None;
    }
    let total = parts[1].split('/').nth(1)?;
    total.parse().ok()
}

async fn download_parallel(
    client: &reqwest::Client,
    opts: &DownloadOptions,
    remote: &RemoteInfo,
    _final_path: &Path,
    part: &Path,
    st_path: &Path,
    threads: u32,
    on_progress: ProgressCb,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let total = remote.size;
    let mut state = if let Some(existing) = load_state(st_path) {
        if existing.url == opts.url && existing.total == total {
            existing
        } else {
            let _ = fs::remove_file(part);
            let _ = fs::remove_file(st_path);
            DownloadState {
                url: opts.url.clone(),
                total,
                threads,
                segments: build_segments(total, threads),
            }
        }
    } else {
        let _ = fs::remove_file(part);
        DownloadState {
            url: opts.url.clone(),
            total,
            threads,
            segments: build_segments(total, threads),
        }
    };

    if state.segments.is_empty() {
        state.segments = build_segments(total, threads);
    }
    if state.all_done() {
        return Ok(());
    }

    {
        let f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(part)
            .map_err(|e| format!("open part: {e}"))?;
        f.set_len(total).map_err(|e| format!("allocate part: {e}"))?;
    }
    save_state(st_path, &state)?;

    let downloaded = Arc::new(AtomicU64::new(state.completed_bytes()));
    let state = Arc::new(Mutex::new(state));
    let speed_tracker = Arc::new(Mutex::new(SpeedTracker::new()));
    let download_url = remote.resolved_url.clone();

    let mut handles = Vec::new();
    {
        let guard = state.lock().await;
        for seg in guard.segments.clone() {
            if seg.done() {
                continue;
            }
            let client = client.clone();
            let settings = opts.settings.clone();
            let download_url = download_url.clone();
            let part = part.to_path_buf();
            let st_path = st_path.to_path_buf();
            let state = state.clone();
            let downloaded = downloaded.clone();
            let on_progress = on_progress.clone();
            let cancel = cancel.clone();
            let speed_tracker = speed_tracker.clone();
            let task_id = opts.task_id.clone();
            handles.push(tokio::spawn(async move {
                download_segment_with_retry(
                    client,
                    settings,
                    download_url,
                    part,
                    st_path,
                    seg,
                    state,
                    downloaded,
                    on_progress,
                    cancel,
                    speed_tracker,
                    task_id,
                    total,
                )
                .await
            }));
        }
    }

    let mut first_err = None;
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if first_err.is_none() && e != "cancelled" {
                    first_err = Some(e);
                }
                cancel.store(true, Ordering::SeqCst);
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(format!("worker join: {e}"));
                }
                cancel.store(true, Ordering::SeqCst);
            }
        }
    }

    if let Some(err) = first_err {
        return Err(err);
    }
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    Ok(())
}

async fn download_segment_with_retry(
    client: reqwest::Client,
    settings: Settings,
    download_url: String,
    part: PathBuf,
    st_path: PathBuf,
    mut seg: Segment,
    state: Arc<Mutex<DownloadState>>,
    downloaded: Arc<AtomicU64>,
    on_progress: ProgressCb,
    cancel: Arc<AtomicBool>,
    speed_tracker: Arc<Mutex<SpeedTracker>>,
    task_id: String,
    total: u64,
) -> Result<(), String> {
    const MAX_RETRIES: u32 = 8;
    let mut attempt = 0u32;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        match download_segment_once(
            &client,
            &settings,
            &download_url,
            &part,
            &st_path,
            &mut seg,
            &state,
            &downloaded,
            &on_progress,
            &cancel,
            &speed_tracker,
            &task_id,
            total,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) if e == "cancelled" => return Err(e),
            Err(e) if !is_retryable(&e) => return Err(e),
            Err(e) => {
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(format!(
                        "segment {} failed after {MAX_RETRIES} retries: {e}",
                        seg.index
                    ));
                }
                let delay = retry_delay(attempt);
                emit(
                    &on_progress,
                    &task_id,
                    downloaded.load(Ordering::SeqCst),
                    total,
                    0,
                    &format!("segment {} retry {attempt}/{MAX_RETRIES}: {e}", seg.index),
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn download_segment_once(
    client: &reqwest::Client,
    _settings: &Settings,
    download_url: &str,
    part: &Path,
    st_path: &Path,
    seg: &mut Segment,
    state: &Arc<Mutex<DownloadState>>,
    downloaded: &Arc<AtomicU64>,
    on_progress: &ProgressCb,
    cancel: &Arc<AtomicBool>,
    speed_tracker: &Arc<Mutex<SpeedTracker>>,
    task_id: &str,
    total: u64,
) -> Result<(), String> {
    if seg.done() {
        return Ok(());
    }
    let cursor = seg.cursor;
    let end = seg.end;
    let range = format!("bytes={cursor}-{end}");

    // Auth not needed on resolved CDN URL typically; still ok if same host.
    let resp = client
        .get(download_url)
        .header("Range", &range)
        .send()
        .await
        .map_err(|e| format!("segment {} request: {e}", seg.index))?;

    if resp.status() != StatusCode::PARTIAL_CONTENT {
        return Err(format!(
            "segment {} range failed: {}",
            seg.index,
            resp.status()
        ));
    }

    let mut stream = resp.bytes_stream();
    let mut file = OpenOptions::new()
        .write(true)
        .open(part)
        .map_err(|e| format!("open part: {e}"))?;
    file.seek(SeekFrom::Start(cursor))
        .map_err(|e| format!("seek: {e}"))?;

    let mut last_flush = Instant::now();
    let mut since_flush = 0u64;

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        let chunk = chunk.map_err(|e| format!("read: {e}"))?;
        if chunk.is_empty() {
            continue;
        }
        file.write_all(&chunk).map_err(|e| format!("write: {e}"))?;
        let n = chunk.len() as u64;
        seg.cursor += n;
        downloaded.fetch_add(n, Ordering::SeqCst);
        since_flush += n;

        {
            let mut st = state.lock().await;
            if let Some(s) = st.segments.iter_mut().find(|s| s.index == seg.index) {
                s.cursor = seg.cursor;
            }
            let force = seg.done() || since_flush >= 512 * 1024 || last_flush.elapsed() >= Duration::from_millis(300);
            if force {
                save_state(st_path, &st)?;
                last_flush = Instant::now();
                since_flush = 0;
            }
        }

        let speed = {
            let mut tracker = speed_tracker.lock().await;
            tracker.note(n)
        };
        emit(
            on_progress,
            task_id,
            downloaded.load(Ordering::SeqCst),
            total,
            speed,
            "downloading",
        );
    }

    if !seg.done() {
        return Err(format!(
            "segment {} connection closed early (remaining {})",
            seg.index,
            seg.end + 1 - seg.cursor
        ));
    }
    let mut st = state.lock().await;
    if let Some(s) = st.segments.iter_mut().find(|s| s.index == seg.index) {
        s.cursor = seg.cursor;
    }
    save_state(st_path, &st)?;
    Ok(())
}

async fn download_sequential(
    client: &reqwest::Client,
    opts: &DownloadOptions,
    remote: &RemoteInfo,
    _final_path: &Path,
    part: &Path,
    st_path: &Path,
    on_progress: ProgressCb,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut offset = 0u64;
    let mut total = remote.size;
    let mut state = DownloadState {
        url: opts.url.clone(),
        total,
        threads: 1,
        segments: vec![],
    };

    if let Some(existing) = load_state(st_path) {
        if existing.url == opts.url {
            state = existing;
            if state.total > 0 {
                total = state.total;
            }
            if let Some(seg) = state.segments.first() {
                offset = seg.cursor;
            } else if let Ok(meta) = fs::metadata(part) {
                offset = meta.len();
            }
        }
    } else if let Ok(meta) = fs::metadata(part) {
        offset = meta.len();
    }

    if total > 0 && offset >= total {
        return Ok(());
    }

    const MAX_RETRIES: u32 = 8;
    let mut attempt = 0u32;
    let speed_tracker = Arc::new(Mutex::new(SpeedTracker::new()));

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        match download_sequential_once(
            client,
            opts,
            remote,
            part,
            st_path,
            &mut state,
            &mut offset,
            &mut total,
            on_progress.clone(),
            cancel.clone(),
            speed_tracker.clone(),
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) if e == "cancelled" => return Err(e),
            Err(e) if !is_retryable(&e) => return Err(e),
            Err(e) => {
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(format!("download failed after {MAX_RETRIES} retries: {e}"));
                }
                let delay = retry_delay(attempt);
                emit(
                    &on_progress,
                    &opts.task_id,
                    offset,
                    total,
                    0,
                    &format!("retry {attempt}/{MAX_RETRIES}: {e}"),
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn download_sequential_once(
    client: &reqwest::Client,
    opts: &DownloadOptions,
    remote: &RemoteInfo,
    part: &Path,
    st_path: &Path,
    state: &mut DownloadState,
    offset: &mut u64,
    total: &mut u64,
    on_progress: ProgressCb,
    cancel: Arc<AtomicBool>,
    speed_tracker: Arc<Mutex<SpeedTracker>>,
) -> Result<(), String> {
    let mut req = client.get(&remote.resolved_url);
    // For host still needing auth (no redirect yet)
    if remote.resolved_url == opts.url {
        req = apply_auth(req, &opts.settings);
    }
    if *offset > 0 {
        req = req.header("Range", format!("bytes={}-", *offset));
    }
    let resp = req.send().await.map_err(|e| format!("GET: {e}"))?;
    let status = resp.status();
    if status == StatusCode::OK && *offset > 0 {
        return Err(format!("server does not support resume at offset {offset}"));
    }
    if !(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT) {
        return Err(format!("GET failed: {status}"));
    }

    if *total == 0 {
        if let Some(cl) = resp.content_length() {
            *total = cl + *offset;
            state.total = *total;
        }
    }
    if *total > 0 {
        state.segments = vec![Segment {
            index: 0,
            start: 0,
            end: total.saturating_sub(1),
            cursor: *offset,
        }];
        save_state(st_path, state)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(part)
        .map_err(|e| format!("open part: {e}"))?;
    file.seek(SeekFrom::Start(*offset))
        .map_err(|e| format!("seek: {e}"))?;

    let mut stream = resp.bytes_stream();
    let mut last_flush = Instant::now();
    let mut since_flush = 0u64;

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        let chunk = chunk.map_err(|e| format!("read: {e}"))?;
        if chunk.is_empty() {
            continue;
        }
        file.write_all(&chunk).map_err(|e| format!("write: {e}"))?;
        let n = chunk.len() as u64;
        *offset += n;
        since_flush += n;
        if !state.segments.is_empty() {
            state.segments[0].cursor = *offset;
        }
        if since_flush >= 512 * 1024 || last_flush.elapsed() >= Duration::from_millis(300) {
            save_state(st_path, state)?;
            last_flush = Instant::now();
            since_flush = 0;
        }
        let speed = {
            let mut tracker = speed_tracker.lock().await;
            tracker.note(n)
        };
        emit(
            &on_progress,
            &opts.task_id,
            *offset,
            *total,
            speed,
            "downloading",
        );
    }

    if *total > 0 && *offset < *total {
        return Err(format!(
            "connection closed early (remaining {})",
            *total - *offset
        ));
    }
    save_state(st_path, state)?;
    Ok(())
}

fn verify_sha256(path: &Path, expect: &str) -> Result<(), String> {
    let expect = expect.trim().to_ascii_lowercase();
    if expect.len() != 64 || !expect.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid sha256 value".into());
    }
    let mut file = File::open(path).map_err(|e| format!("open for sha256: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 64];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("read for sha256: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got = hex::encode(hasher.finalize());
    if got != expect {
        return Err(format!("sha256 mismatch: got {got}, want {expect}"));
    }
    Ok(())
}

fn emit(cb: &ProgressCb, task_id: &str, downloaded: u64, total: u64, speed: u64, message: &str) {
    cb(ProgressEvent {
        task_id: task_id.to_string(),
        downloaded,
        total,
        speed_bps: speed,
        message: message.to_string(),
    });
}

fn is_retryable(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    if lower.contains("401") || lower.contains("403") || lower.contains("unauthorized") {
        return false;
    }
    [
        "connection",
        "timed out",
        "timeout",
        "reset",
        "broken pipe",
        "closed early",
        "eof",
        "502",
        "503",
        "504",
        "429",
        "408",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

fn retry_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(4);
    let ms = (500u64) << shift;
    Duration::from_millis(ms.min(8000))
}

struct SpeedTracker {
    window_start: Instant,
    window_bytes: u64,
    last_bps: u64,
}

impl SpeedTracker {
    fn new() -> Self {
        Self {
            window_start: Instant::now(),
            window_bytes: 0,
            last_bps: 0,
        }
    }

    fn note(&mut self, n: u64) -> u64 {
        self.window_bytes += n;
        let elapsed = self.window_start.elapsed();
        if elapsed >= Duration::from_millis(400) {
            let secs = elapsed.as_secs_f64().max(0.001);
            self.last_bps = (self.window_bytes as f64 / secs) as u64;
            self.window_start = Instant::now();
            self.window_bytes = 0;
        }
        self.last_bps
    }
}
