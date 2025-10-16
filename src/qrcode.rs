use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bardecoder::{decode::Decode, detect::Detect, extract::Extract, prepare::Prepare};
use image::DynamicImage;

pub(crate) fn qr_decode_thread(
    next_image: Arc<Mutex<Option<(i32, image::ImageBuffer<image::Rgba<u8>, Vec<u8>>)>>>,
) -> String {
    {
        let mut bardecoder_time = Duration::ZERO;

        loop {
            let Some((frame_id, rgba_img)) = next_image.lock().unwrap().take() else {
                std::thread::park();
                continue;
            };
            let bardecoder_start = Instant::now();
            eprintln!("searching for barcode in frame {frame_id}");
            let decoded = qr_decode(frame_id, rgba_img);
            bardecoder_time += bardecoder_start.elapsed();
            if let Some(decoded) = decoded {
                if decoded.starts_with("WIFI:") {
                    dbg!(bardecoder_time);
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
    let decode = bardecoder::decode::QRDecoder::new();
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

                match decode.decode(Ok(bardecoder::util::qr::QRData {
                    data: extracted.data.clone(),
                    version: extracted.version,
                    side: extracted.side,
                })) {
                    Ok(decoded) => return Some(decoded),
                    Err(err) => {
                        eprintln!("[{frame_id}] bardecoder decode error {err:?}");

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
                                    icy_sixel::Quality::HIGH, // AUTO, HIGH, LOW, FULL, HIGHCOLOR
                                )
                                .expect("Failed to encode image to SIXEL format");
                                eprintln!("{sixel_data}");

                                return Some(content);
                            }
                            Err(err) => {
                                eprintln!("[{frame_id}] rqrr can't decode qr code either: {err:?}");
                            }
                        };

                        continue;
                    }
                };
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
