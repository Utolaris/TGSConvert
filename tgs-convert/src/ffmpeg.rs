use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};

use crate::options::{ConvertOptions, OutputFormat};

pub fn encode(
    frames_directory: &Path,
    options: &ConvertOptions,
    frame_count: usize,
    width: usize,
    height: usize,
    output: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    if options.output_format == OutputFormat::Gif {
        return encode_gif(
            frames_directory,
            options,
            frame_count,
            width,
            height,
            output,
            cancel,
        );
    }
    if options.output_format == OutputFormat::Webp {
        return encode_webp(frames_directory, options, frame_count, output, cancel);
    }

    let duration_seconds = frame_count as f64 / f64::from(options.fps);
    let fps = options.fps.to_string();
    let frame_count = frame_count.to_string();
    let threads = options.threads.to_string();
    let mut arguments = vec![
        "-hide_banner".to_owned(),
        "-y".to_owned(),
        "-framerate".to_owned(),
        fps,
        "-i".to_owned(),
        "%05d.png".to_owned(),
        "-frames:v".to_owned(),
        frame_count,
    ];

    let progress_name = match options.output_format {
        OutputFormat::WebmVp9 => {
            let crf = options.vp9_crf().to_string();
            let cpu_used = options.vp9_cpu_used().to_string();
            arguments.extend([
                "-c:v".to_owned(),
                "libvpx-vp9".to_owned(),
                "-crf".to_owned(),
                crf,
                "-b:v".to_owned(),
                "0".to_owned(),
                "-cpu-used".to_owned(),
                cpu_used,
                "-threads".to_owned(),
                threads.clone(),
                "-row-mt".to_owned(),
                "1".to_owned(),
                "-tile-columns".to_owned(),
                "2".to_owned(),
                "-tile-rows".to_owned(),
                "1".to_owned(),
                "-frame-parallel".to_owned(),
                "1".to_owned(),
                "-auto-alt-ref".to_owned(),
                "1".to_owned(),
                "-lag-in-frames".to_owned(),
                "25".to_owned(),
                "-pix_fmt".to_owned(),
                "yuva420p".to_owned(),
            ]);
            "VP9 alpha WebM"
        }
        OutputFormat::MovProres4444 => {
            let bits_per_mb = options.prores_bits_per_mb().to_string();
            arguments.extend([
                "-c:v".to_owned(),
                "prores_ks".to_owned(),
                "-profile:v".to_owned(),
                "4".to_owned(),
                "-bits_per_mb".to_owned(),
                bits_per_mb,
                "-alpha_bits".to_owned(),
                "16".to_owned(),
                "-threads".to_owned(),
                threads.clone(),
                "-pix_fmt".to_owned(),
                "yuva444p10le".to_owned(),
                "-movflags".to_owned(),
                "+faststart".to_owned(),
            ]);
            "ProRes 4444 alpha MOV"
        }
        OutputFormat::Webp => {
            unreachable!("WebP is encoded by img2webp before FFmpeg arguments are built")
        }
        OutputFormat::Gif => {
            unreachable!("GIF is encoded by gifski before FFmpeg arguments are built")
        }
    };
    arguments.extend([
        "-progress".to_owned(),
        "pipe:1".to_owned(),
        "-nostats".to_owned(),
    ]);
    let mut child = Command::new(&options.ffmpeg)
        .current_dir(frames_directory)
        .args(&arguments)
        .arg(output)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to start {}", options.ffmpeg.display()))?;

    let stdout = child
        .stdout
        .take()
        .expect("ffmpeg stdout was configured as piped");
    let progress_cancel = Arc::clone(&cancel);
    let progress_reader = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut last_percent = 0_u8;
        for line in reader.lines().map_while(|line| line.ok()) {
            if progress_cancel.load(Ordering::Acquire) {
                break;
            }
            if let Some(value) = line.strip_prefix("out_time_us=") {
                if let Ok(microseconds) = value.parse::<f64>() {
                    let percent = (microseconds / 1_000_000.0 / duration_seconds * 100.0)
                        .clamp(0.0, 100.0) as u8;
                    if percent > last_percent || percent == 100 {
                        last_percent = percent;
                        eprint!("\rEncoding {progress_name}: {percent}%");
                    }
                }
            }
        }
    });

    let status = loop {
        if cancel.load(Ordering::Acquire) {
            child
                .kill()
                .context("failed to stop ffmpeg after cancellation")?;
            let _ = child.wait();
            let _ = progress_reader.join();
            bail!("conversion cancelled");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let _ = progress_reader.join();
    eprintln!();

    if !status.success() {
        bail!("ffmpeg exited with {status}");
    }
    Ok(())
}

fn encode_webp(
    frames_directory: &Path,
    options: &ConvertOptions,
    frame_count: usize,
    output: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let quality = options.quality.max(1).to_string();
    let mut command = Command::new("img2webp");
    command
        .current_dir(frames_directory)
        .args(["-loop", "0", "-min_size"]);
    for frame in 0..frame_count {
        let duration = webp_frame_duration_ms(frame, options.fps).to_string();
        command.args(["-d", &duration]);
        if options.quality == 100 {
            command.args(["-lossless", "-exact", "-m", "6"]);
        } else {
            command.args(["-lossy", "-exact", "-q", &quality, "-m", "6"]);
        }
        command.arg(format!("{frame:05}.png"));
    }
    command.args(["-o"]).arg(output);

    let mut child = command
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to start img2webp; install Homebrew webp or make it available on PATH")?;
    let status = wait_for_child(&mut child, cancel, "img2webp")?;
    if !status.success() {
        bail!("img2webp exited with {status}");
    }
    Ok(())
}

fn webp_frame_duration_ms(frame: usize, fps: u32) -> u32 {
    let frame = frame as u64;
    let fps = u64::from(fps);
    (((frame + 1) * 1_000 / fps) - (frame * 1_000 / fps)) as u32
}

fn encode_gif(
    frames_directory: &Path,
    options: &ConvertOptions,
    frame_count: usize,
    width: usize,
    height: usize,
    output: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let fps = options.fps.to_string();
    let quality = options.quality.max(1).to_string();
    let width = width.to_string();
    let height = height.to_string();
    let mut command = Command::new("gifski");
    command
        .current_dir(frames_directory)
        .args([
            "--fps",
            &fps,
            "--quality",
            &quality,
            "--repeat",
            "0",
            "--width",
            &width,
            "--height",
            &height,
            "--no-sort",
            "--output",
        ])
        .arg(output);
    for frame in 0..frame_count {
        command.arg(format!("{frame:05}.png"));
    }

    let mut child = command
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to start gifski; install it or make it available on PATH")?;
    let status = wait_for_child(&mut child, cancel, "gifski")?;
    if !status.success() {
        bail!("gifski exited with {status}");
    }
    Ok(())
}

fn wait_for_child(
    child: &mut std::process::Child,
    cancel: Arc<AtomicBool>,
    name: &str,
) -> Result<std::process::ExitStatus> {
    loop {
        if cancel.load(Ordering::Acquire) {
            child
                .kill()
                .with_context(|| format!("failed to stop {name} after cancellation"))?;
            let _ = child.wait();
            bail!("conversion cancelled");
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::webp_frame_duration_ms;

    #[test]
    fn webp_sixty_fps_has_exact_total_duration() {
        let durations = (0..180)
            .map(|frame| webp_frame_duration_ms(frame, 60))
            .collect::<Vec<_>>();
        assert!(durations.iter().all(|duration| matches!(duration, 16 | 17)));
        assert_eq!(durations.iter().sum::<u32>(), 3_000);
    }
}
