// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `--screenshot <path>`: the app captures its own frame through egui.
//!
//! The GUI paints through eframe/wgpu, and the screen-check desktop has no
//! compositor to read the GPU surface from: `PrintWindow` and the `BitBlt`
//! fallback capture a black client area of a hidden window. The window's
//! own integration can read the pixels back, though:
//! `egui::ViewportCommand::Screenshot` renders the next frame to a capture
//! texture, copies it to a buffer and hands the `ColorImage` to a callback,
//! even when the window is hidden or occluded and the compositor gives it no
//! surface at all (see `egui-wgpu`'s `CaptureState`). This module is that
//! callback's owner: the switches, the readiness gate that waits for the
//! load (and the `--dialog` view) to settle, and the PNG writer.
//!
//! The switches are:
//!
//! - `--screenshot <path>`: write one PNG of the window to `path`.
//! - `--screenshot-frames <n>`: the number of frames to draw after the
//!   screen is ready before the capture (default 2, so a dialog that asks
//!   the command layer for its lists on its first pass is laid out).
//! - `--screenshot-delay <ms>`: a further wall-clock wait after those
//!   frames, for a screen whose content arrives on a worker thread.
//! - `--exit`: close the window once the PNG is written, so a screen-check
//!   run cannot hang. A run that never reaches its screen (a load that
//!   fails to finish) closes after [`READY_TIMEOUT`] without a PNG.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eframe::egui;

/// A run that has not reached the screen it was asked to capture by then
/// gives up, so `--exit` still ends the process and a test cannot hang.
pub const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// The frames drawn after the screen is ready, unless `--screenshot-frames`
/// names another count. Two frames cover the startup dialog's first pass
/// (its lists come back from the command layer on that pass) and the layout
/// pass that follows it.
pub const DEFAULT_FRAMES: u32 = 2;

/// The switches of a capture, as the command line parses them.
#[derive(Clone, Debug)]
pub struct ScreenshotPlan {
    pub path: std::path::PathBuf,
    pub frames: u32,
    pub delay: Duration,
    pub exit: bool,
}

/// Where the capture has got to.
pub enum ScreenshotOutcome {
    /// Waiting for the screen, the frame count or the delay.
    Pending,
    /// The PNG was written (or the write failed; the reason is logged).
    Finished,
    /// The screen never became ready within [`READY_TIMEOUT`].
    TimedOut,
}

/// The state machine between the first pass and the PNG on disk.
pub struct ScreenshotState {
    plan: ScreenshotPlan,
    started: Instant,
    ready_since: Option<Instant>,
    frames_since_ready: u32,
    requested: bool,
    /// Set once the capture callback has run, whichever way it went.
    captured: Arc<AtomicBool>,
    /// Whether the capture callback failed to write the PNG.
    failed: Arc<AtomicBool>,
    reported: bool,
    timed_out: bool,
}

impl ScreenshotState {
    pub fn new(plan: ScreenshotPlan) -> Self {
        Self {
            plan,
            started: Instant::now(),
            ready_since: None,
            frames_since_ready: 0,
            requested: false,
            captured: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
            reported: false,
            timed_out: false,
        }
    }

    /// Whether the process should close once the capture is finished.
    pub fn exit_when_done(&self) -> bool {
        self.plan.exit
    }

    /// Whether the PNG the run was asked for exists (for the exit code and
    /// the log).
    pub fn captured(&self) -> bool {
        self.captured.load(Ordering::SeqCst) && !self.failed.load(Ordering::SeqCst)
    }

    /// Advances the capture by one pass. `ready` is the caller's judgement
    /// that the screen the switch named is on the window (the load has
    /// finished, or a startup dialog is up); `frame_drawn` is true when the
    /// caller is the `ui` pass, so `--screenshot-frames` counts painted
    /// frames rather than logic ticks. Returns what the caller should do; a
    /// finished state is stable until the process closes.
    pub fn poll(&mut self, ctx: &egui::Context, ready: bool, frame_drawn: bool) -> ScreenshotOutcome {
        if self.captured.load(Ordering::SeqCst) {
            if !self.reported {
                self.reported = true;
                if self.failed.load(Ordering::SeqCst) {
                    log(&format!("screenshot: could not write {}", self.plan.path.display()));
                } else {
                    log(&format!("screenshot: wrote {}", self.plan.path.display()));
                }
            }
            return ScreenshotOutcome::Finished;
        }
        if self.timed_out {
            return ScreenshotOutcome::TimedOut;
        }
        if self.requested {
            // The callback has not run yet; the paint that carries the
            // capture is on its way.
            ctx.request_repaint();
            return ScreenshotOutcome::Pending;
        }
        if !ready {
            if self.started.elapsed() > READY_TIMEOUT {
                self.timed_out = true;
                log(&format!(
                    "screenshot: the screen was not ready within {} s",
                    READY_TIMEOUT.as_secs()
                ));
                return ScreenshotOutcome::TimedOut;
            }
            // The load runs on a worker thread: keep the logic ticking so
            // its result is polled even when the window is hidden and the
            // integration runs no ui pass.
            ctx.request_repaint_after(Duration::from_millis(100));
            return ScreenshotOutcome::Pending;
        }
        let since = *self.ready_since.get_or_insert_with(Instant::now);
        if self.frames_since_ready < self.plan.frames {
            if frame_drawn {
                self.frames_since_ready += 1;
            }
            ctx.request_repaint();
            // A hidden window runs no ui pass, and the frame count above is
            // a count of painted frames: ask for one.
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
            return ScreenshotOutcome::Pending;
        }
        let elapsed = since.elapsed();
        if elapsed < self.plan.delay {
            ctx.request_repaint_after(self.plan.delay - elapsed);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
            return ScreenshotOutcome::Pending;
        }

        self.requested = true;
        let path = self.plan.path.clone();
        let captured = Arc::clone(&self.captured);
        let failed = Arc::clone(&self.failed);
        let repaint_ctx = ctx.clone();
        ctx.request_screenshot(move |image| {
            let result = write_png(&path, &image);
            match &result {
                Ok(()) => log(&format!("screenshot: wrote {}", path.display())),
                Err(error) => log(&format!("screenshot: {error}")),
            }
            failed.store(result.is_err(), Ordering::SeqCst);
            captured.store(true, Ordering::SeqCst);
            // The callback does not ask for a pass of its own; the next one
            // reports the result and closes the window under `--exit`.
            repaint_ctx.request_repaint();
        });
        ScreenshotOutcome::Pending
    }
}

/// `XEDIT_GUI_LOG=<path>`: the capture lines, which a private desktop
/// otherwise hides. Also echoed on stderr.
fn log(message: &str) {
    eprintln!("xedit-gui: {message}");
    let Ok(path) = std::env::var("XEDIT_GUI_LOG") else {
        return;
    };
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

/// Writes the captured frame as 8-bit RGBA PNG. `ColorImage` pixels are
/// premultiplied; PNG stores straight alpha, so the colour goes through
/// `to_srgba_unmultiplied` (the surface is opaque, where the two agree).
fn write_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let file = std::fs::File::create(path).map_err(|error| format!("could not create {}: {error}", path.display()))?;
    let [width, height] = image.size;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        u32::try_from(width).map_err(|_| format!("the frame is {width} pixels wide"))?,
        u32::try_from(height).map_err(|_| format!("the frame is {height} pixels high"))?,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    writer
        .write_image_data(&rgba)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plan's defaults are the two frames and no delay; the switch
    /// parser of `main.rs` is covered by the binary's own help text.
    #[test]
    fn the_default_plan_waits_two_frames() {
        assert_eq!(DEFAULT_FRAMES, 2);
        let plan = ScreenshotPlan {
            path: std::path::PathBuf::from("shot.png"),
            frames: DEFAULT_FRAMES,
            delay: Duration::ZERO,
            exit: true,
        };
        let state = ScreenshotState::new(plan);
        assert!(state.exit_when_done());
        assert!(!state.captured());
    }

    /// A ready screen walks the frame count down and then requests the
    /// capture once; until the callback has run the state stays pending.
    #[test]
    fn the_frames_are_counted_before_the_capture() {
        let plan = ScreenshotPlan {
            // Never written: the capture callback only runs in a live
            // integration, and this test only walks the frame counter.
            path: std::path::PathBuf::from("screenshot-test.png"),
            frames: 2,
            delay: Duration::ZERO,
            exit: false,
        };
        let mut state = ScreenshotState::new(plan);
        // No egui pass is run: the context is only the command sink, and
        // the capture callback never runs in a test.
        let ctx = egui::Context::default();
        for _ in 0..2 {
            assert!(matches!(state.poll(&ctx, true, true), ScreenshotOutcome::Pending));
        }
        assert!(matches!(state.poll(&ctx, true, true), ScreenshotOutcome::Pending));
        assert!(state.requested_after_ready_for_test());
    }

    impl ScreenshotState {
        fn requested_after_ready_for_test(&self) -> bool {
            self.requested
        }
    }
}
