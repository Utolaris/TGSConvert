mod ffmpeg;
mod options;
mod render;
pub mod telegram;

use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, anyhow, bail};
use tempfile::Builder;

pub use options::{ConvertOptions, OutputFormat};
use render::{RenderSettings, load_animation, render_sequence};

#[derive(Clone, Copy, Debug)]
pub struct ConversionReport {
    pub width: usize,
    pub height: usize,
    pub frames: usize,
    pub duration_seconds: f64,
}

pub fn convert(options: &ConvertOptions) -> Result<ConversionReport> {
    options.validate()?;
    let output_parent = output_parent(&options.output)?;
    fs::create_dir_all(output_parent)
        .with_context(|| format!("failed to create {}", output_parent.display()))?;
    let absolute_output = absolute_output_path(&options.output)?;

    let cancel = Arc::new(AtomicBool::new(false));
    install_cancel_handler(Arc::clone(&cancel))?;
    let animation = load_animation(&options.input)?;
    let width = options.width.unwrap_or(animation.metadata.width);
    let height = options.height.unwrap_or(animation.metadata.height);
    let render_settings = RenderSettings {
        fps: options.fps,
        play_speed: options.play_speed,
        width,
        height,
        rotation_degrees: options.rotation_degrees,
        flip_horizontal: options.flip_horizontal,
        flip_vertical: options.flip_vertical,
        threads: options.threads,
    };
    let duration_seconds = animation.metadata.duration_seconds / options.play_speed;
    let temporary_directory = Builder::new()
        .prefix("tgs-frames-")
        .tempdir_in(output_parent)
        .context("failed to create temporary frame directory")?;

    eprintln!(
        "Rendering {} at {width}x{height}, {} fps, {} worker(s)",
        options.input.display(),
        options.fps,
        options.threads
    );
    let frames = render_sequence(
        &animation,
        render_settings,
        temporary_directory.path(),
        Arc::clone(&cancel),
    )?;
    if cancel.load(Ordering::Acquire) {
        bail!("conversion cancelled");
    }

    eprintln!(
        "Encoding {} as {}",
        options.output.display(),
        options.output_format.description()
    );
    ffmpeg::encode(
        temporary_directory.path(),
        options,
        frames,
        width,
        height,
        &absolute_output,
        Arc::clone(&cancel),
    )?;

    Ok(ConversionReport {
        width,
        height,
        frames,
        duration_seconds,
    })
}

fn install_cancel_handler(cancel: Arc<AtomicBool>) -> Result<()> {
    ctrlc::set_handler(move || {
        cancel.store(true, Ordering::Release);
        eprintln!("\nCancellation requested; stopping after the active frame.");
    })
    .map_err(|error| anyhow!("failed to install Ctrl-C handler: {error}"))
}

fn output_parent(output: &Path) -> Result<&Path> {
    Ok(output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new(".")))
}

fn absolute_output_path(output: &Path) -> Result<std::path::PathBuf> {
    if output.is_absolute() {
        Ok(output.to_owned())
    } else {
        std::env::current_dir()
            .context("failed to resolve current working directory")
            .map(|directory| directory.join(output))
    }
}
