use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, log_enabled};
use nokhwa::{
    Camera,
    pixel_format::RgbAFormat,
    utils::{CameraIndex, RequestedFormat, RequestedFormatType},
};
use percent_encoding::percent_decode_str;

mod mailslot;
mod qrcode;

fn main() {
    let logger =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).build();
    let level = logger.filter();
    let multi_progress = indicatif::MultiProgress::new();

    indicatif_log_bridge::LogWrapper::new(multi_progress.clone(), logger)
        .try_init()
        .unwrap();
    log::set_max_level(level);

    // first camera in system
    let index = CameraIndex::Index(0);
    // request the absolute highest resolution CameraFormat that can be decoded to RGB.
    let requested =
        RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestResolution);
    // make the camera
    let mut camera = Camera::new(index, requested).unwrap();
    camera.open_stream().unwrap();
    let mut capture_time = Duration::ZERO;
    let mut decode_time = Duration::ZERO;
    let mut sixel_time = Duration::ZERO;

    let progress = multi_progress.add(
        indicatif::ProgressBar::new_spinner()
            .with_message("frames captured")
            .with_style(ProgressStyle::with_template("{spinner} {pos} {msg}").unwrap()),
    );

    for warmup_iter in 0..3 {
        let frame = camera.frame().unwrap();
        eprintln!(
            "Captured Warmup frame {warmup_iter} {}",
            frame.buffer().len()
        );
    }

    let (next_image_for_qrcode_thread, image_receiver) = crate::mailslot::mailslot();
    let qrcode_thread = {
        let qr_progress = multi_progress.add(
            ProgressBar::new_spinner()
                .with_message("last scanned")
                .with_style(ProgressStyle::with_template("{spinner} {pos} {msg}").unwrap()),
        );
        std::thread::spawn(|| qrcode::qr_decode_thread(image_receiver, qr_progress))
    };

    for frame_id in 1.. {
        // sleep(Duration::from_secs(1));
        if qrcode_thread.is_finished() {
            break;
        }

        // get a frame
        let capture_start = Instant::now();
        let frame = camera.frame().unwrap();
        // eprintln!("Captured Frame {frame_id} len {}", frame.buffer().len());
        capture_time += capture_start.elapsed();

        // decode into an ImageBuffer
        let decode_start = Instant::now();
        let decoded = frame.decode_image::<RgbAFormat>().unwrap();
        // eprintln!("Decoded Frame {frame_id}");
        decode_time += decode_start.elapsed();

        if frame_id % 10 == 5 && log_enabled!(log::Level::Debug) {
            let sixel_start = Instant::now();
            let (width, height) = decoded.dimensions();
            let img_rgba8888 = decoded.as_raw();
            // Encode as SIXEL data

            let sixel_data = icy_sixel::sixel_string(
                img_rgba8888,
                width as i32,
                height as i32,
                icy_sixel::PixelFormat::RGBA8888,
                icy_sixel::DiffusionMethod::Auto, // Auto, None, Atkinson, FS, JaJuNi, Stucki, Burkes, ADither, XDither
                icy_sixel::MethodForLargest::Auto, // Auto, Norm, Lum
                icy_sixel::MethodForRep::Auto,    // Auto, CenterBox, AverageColors, Pixels
                icy_sixel::Quality::HIGH,         // AUTO, HIGH, LOW, FULL, HIGHCOLOR
            )
            .expect("Failed to encode image to SIXEL format");
            debug!("Capture preview:\n{sixel_data}");
            sixel_time += sixel_start.elapsed();
        }
        progress.set_position(frame_id.try_into().unwrap());

        next_image_for_qrcode_thread.send_replace((frame_id, decoded));
    }
    let wifi_uri = qrcode_thread.join().unwrap();
    progress.finish_and_clear();
    let connection = parse_wifi_uri(wifi_uri);
    let nmcli = connection.render_to_nmcli();
    println!("To connect, run:\n  {nmcli}");
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum WifiUriParamKey {
    SecurityType,
    TransitionDisable,
    Ssid,
    Hidden,
    SaePasswordIdentifier,
    Password,
    PublicKey,
}

impl WifiUriParamKey {
    fn from_str(key: &str) -> Self {
        match key {
            "T" => Self::SecurityType,
            "R" => Self::TransitionDisable,
            "S" => Self::Ssid,
            "H" => Self::Hidden,
            "I" => Self::SaePasswordIdentifier,
            "P" => Self::Password,
            "K" => Self::PublicKey,

            _ => {
                panic!("unknown WIFI URI param '{key}:'")
            }
        }
    }
}

fn parse_wifi_uri(wifi_uri: String) -> WifiConnection {
    let mut remaining = wifi_uri
        .strip_prefix("WIFI:")
        .expect("WIFI URI should start with 'WIFI:'");
    let mut params = HashMap::new();
    loop {
        if remaining == ";" {
            break;
        } else if remaining.is_empty() {
            panic!("unterminated WIFI URI");
        }
        let tag: &str;
        (tag, remaining) = remaining
            .split_once(':')
            .unwrap_or_else(|| panic!("no ':' left in WIFI URI"));
        let value: &str;
        (value, remaining) = remaining
            .split_once(';')
            .unwrap_or_else(|| panic!("unterminated {tag}: in WIFI URI"));
        let value = percent_decode_str(value)
            .decode_utf8()
            .expect("only utf8 values are supported for now")
            .into_owned();
        let key = WifiUriParamKey::from_str(tag);
        if params.insert(key, value.to_owned()).is_some() {
            panic!("duplicate key '{tag}:' in WIFI URI");
        }
    }
    debug!("WIFI URI params: {params:?}");
    if let Some(transition_disable) = params.remove(&WifiUriParamKey::TransitionDisable) {
        if let Ok(transition_disable) = transition_disable.parse::<i32>()
            && transition_disable == 0
        {
            // 0 is normal, so nothing to do here.
        } else {
            panic!("unsupported transition_disable flag: {transition_disable:?}");
        }
    }
    if let Some(security) = params.remove(&WifiUriParamKey::SecurityType)
        && security != "WPA"
    {
        panic!("unsupported security type {security:?}")
    }
    let c = WifiConnection {
        ssid: params
            .remove(&WifiUriParamKey::Ssid)
            .unwrap_or_else(|| panic!("WIFI URI missing SSID")),
        password: params.remove(&WifiUriParamKey::Password),
        hidden: params
            .remove(&WifiUriParamKey::Hidden)
            .map(|s| {
                s.parse::<bool>()
                    .expect("hidden should be \"true\" or \"false\"")
            })
            .unwrap_or(false),
    };

    if !params.is_empty() {
        panic!("unsupported flags: {params:?}");
    }
    c
}

struct WifiConnection {
    ssid: String,
    password: Option<String>,
    hidden: bool,
}

impl WifiConnection {
    fn render_to_nmcli(&self) -> String {
        let password_args = match &self.password {
            Some(password) => format!(" password {}", shlex::try_quote(password).unwrap()),
            None => "".to_owned(),
        };
        let hidden_args = match self.hidden {
            true => " hidden yes",
            false => "",
        };
        format!(
            "nmcli device wifi connect {ssid}{password_args}{hidden_args}",
            ssid = shlex::try_quote(&self.ssid).unwrap()
        )
    }
}
