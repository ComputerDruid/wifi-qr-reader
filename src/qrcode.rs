use std::time::{Duration, Instant};

use bardecoder::{detect::Detect, extract::Extract, prepare::Prepare};
use image::DynamicImage;

#[allow(clippy::type_complexity, reason = "this is as clear as I can make it")]
pub(crate) fn qr_decode_thread(
    mut next_image: crate::mailslot::MailslotReceiver<(
        i32,
        image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    )>,
) -> String {
    {
        let mut decoding_time = Duration::ZERO;

        loop {
            let (frame_id, rgba_img) = next_image.recv();
            let bardecoder_start = Instant::now();
            eprintln!("searching for barcode in frame {frame_id}");
            let decoded = qr_decode(frame_id, rgba_img);
            decoding_time += bardecoder_start.elapsed();
            if let Some(decoded) = decoded {
                if decoded.starts_with("WIFI:") {
                    dbg!(decoding_time);
                    eprintln!("[{frame_id}] found code {decoded:?}");
                    return decoded;
                } else {
                    eprintln!("[{frame_id}] found non-wifi (or incorrect) QR code {decoded:?}");
                }
            }
        }
    }
}

fn qr_decode(frame_id: i32, image: image::ImageBuffer<image::Rgba<u8>, Vec<u8>>) -> Option<String> {
    let (width, height) = image.dimensions();
    let buf = image.into_vec();
    let image =
        image_0_24::ImageBuffer::<image_0_24::Rgba<u8>, _>::from_vec(width, height, buf).unwrap();

    let prepare = bardecoder::prepare::BlockedMean::new(5, 7);
    let prepared = prepare.prepare(&image);
    let detect = bardecoder::detect::LineScan::new();
    let detected = detect.detect(&prepared);
    let extract = bardecoder::extract::QRExtractor::new();
    for loc in detected {
        match loc {
            bardecoder::detect::Location::QR(qrloc) => {
                let extracted = match extract.extract(&prepared, qrloc) {
                    Ok(extracted) => extracted,
                    Err(err) => {
                        eprintln!("[{frame_id}] bardecoder extract error {err:?}");
                        continue;
                    }
                };
                let side = extracted.side;
                let grid = rqrr::SimpleGrid::from_func(side as usize, |x, y| {
                    extracted.data[y * (side as usize) + x] == 0
                });
                let grid = rqrr::Grid::new(grid);

                match grid.decode() {
                    Ok((_meta, content)) => {
                        eprintln!("[{frame_id}] rqrr found code {content:?}");
                        let qr_img = draw_qr_code(&extracted);
                        // qr_img.write_to(
                        //     &mut std::fs::OpenOptions::new()
                        //         .create(true)
                        //         .write(true)
                        //         .open(format!("/tmp/qr/{frame_id}.png"))
                        //         .unwrap(),
                        //     image::ImageFormat::Png,
                        // )
                        // .unwrap();
                        let qr_img = qr_img.into_rgb8();
                        dbg!(qr_img.width(), qr_img.height());
                        let sixel_data = icy_sixel::sixel_string(
                            qr_img.as_raw(),
                            qr_img.width() as i32,
                            qr_img.height() as i32,
                            icy_sixel::PixelFormat::RGB888,
                            icy_sixel::DiffusionMethod::Auto, // Auto, None, Atkinson, FS, JaJuNi, Stucki, Burkes, ADither, XDither
                            icy_sixel::MethodForLargest::Auto, // Auto, Norm, Lum
                            icy_sixel::MethodForRep::Auto, // Auto, CenterBox, AverageColors, Pixels
                            icy_sixel::Quality::HIGH,      // AUTO, HIGH, LOW, FULL, HIGHCOLOR
                        )
                        .expect("Failed to encode image to SIXEL format");
                        eprintln!("{sixel_data}");

                        return Some(content);
                    }
                    Err(err) => {
                        eprintln!("[{frame_id}] rqrr can't decode qr code: {err:?}");
                    }
                };

                continue;
            }
        }
    }
    None
}

fn draw_qr_code(qr: &bardecoder::util::qr::QRData) -> image::DynamicImage {
    let mut extracted = bardecoder::util::qr::QRData {
        side: qr.side,
        version: qr.version,
        data: qr.data.clone(),
    };
    let width = extracted.side;
    let height = extracted.side;
    let mut raw_image = vec![];
    for (row, col) in [(0, 0), (height - 7, 0), (0, width - 7)] {
        for r in row..row + 8 {
            if (0..height).contains(&r) {
                for c in col..col + 8 {
                    if (0..width).contains(&c) {
                        extracted.data[(r * width + c) as usize] = 255;
                    }
                }
            }
        }
        for r in row..row + 7 {
            for c in col..col + 7 {
                extracted.data[(r * width + c) as usize] = 0;
            }
        }
        for r in row + 1..row + 6 {
            for c in col + 1..col + 6 {
                extracted.data[(r * width + c) as usize] = 255;
            }
        }
        for r in row + 2..row + 5 {
            for c in col + 2..col + 5 {
                extracted.data[(r * width + c) as usize] = 0;
            }
        }
    }
    let mut extracted_iter = extracted.data.iter().copied();
    for row in 0..height + 2 {
        for col in 0..width + 2 {
            let val = if (1..height + 1).contains(&row) && (1..width + 1).contains(&col) {
                extracted_iter.next().unwrap()
            } else {
                255u8
            };
            raw_image.push(val);
            raw_image.push(val);
            raw_image.push(val);
        }
    }
    assert!(extracted_iter.next().is_none());

    let img = image::ImageBuffer::<image::Rgb<u8>, _>::from_vec(width + 2, height + 2, raw_image)
        .unwrap();
    DynamicImage::from(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_rqrr() {
        // Downsampling required; otherwise bardecoder gets confused by the light grey pixels
        // added to the background. It's like this QR code encoder is trying as hard as possible
        // to make codes that are hard to read :(
        let img_data = include_bytes!("testdata/Screenshot_20251018-135642.small.png");
        let img = image::load_from_memory(img_data).unwrap().into_rgba8();
        let result = qr_decode(0, img);
        assert_eq!(
            result,
            Some("WIFI:S:Not a real network;T:SAE;P:password;H:false;;".into())
        );
    }
}
