//! Text and barcode recognition on Windows and Linux, which the macOS build gets from Vision.
//!
//! Windows uses the system's own recognizer (`Windows.Media.Ocr`, every language with its OCR
//! feature installed). Linux uses ocrs, a pure-Rust recognizer, with its models built in (Latin
//! script). QR codes and other barcodes are read with rxing on both.
use crate::model::{Image, R};
use crate::system::OcrResult;

/// A recognized line of text and its box in image pixels.
pub struct Line {
    pub text: String,
    pub rect: R,
}

/// Pictures smaller than this in either direction hold no readable text.
const MIN_TEXT: u32 = 6;
/// No barcode fits in fewer pixels than this, and rxing loops on narrow tall pictures under it.
const MIN_CODE: u32 = 16;

pub fn recognize(img: &Image) -> Result<OcrResult, String> {
    if img.w < MIN_TEXT || img.h < MIN_TEXT || img.px.len() < (img.w as usize) * (img.h as usize) * 4 {
        return Ok(OcrResult { lines: vec![], codes: vec![] });
    }
    let lines = engine::lines(img)?;
    Ok(OcrResult { lines: lines.into_iter().map(|l| l.text).collect(), codes: barcodes(img) })
}

/// Text areas for Blur Text and Erase Text, in pixels from the top left.
pub fn text_boxes(img: &Image) -> Vec<R> {
    if img.w < MIN_TEXT || img.h < MIN_TEXT || img.px.len() < (img.w as usize) * (img.h as usize) * 4 {
        return vec![];
    }
    engine::lines(img).map(|lines| lines.into_iter().map(|l| l.rect.expand(2.0)).collect()).unwrap_or_default()
}

/// Every QR code and barcode in the picture, as text.
pub fn barcodes(img: &Image) -> Vec<String> {
    if img.w < MIN_CODE || img.h < MIN_CODE {
        return vec![];
    }
    let luma: Vec<u8> = img
        .px
        .chunks_exact(4)
        .map(|p| {
            // On a transparent pixel, what shows is the white of the editor's export.
            let a = p[3] as u32;
            let y = (p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000;
            ((y * a + 255 * (255 - a)) / 255) as u8
        })
        .collect();
    let mut out: Vec<String> = vec![];
    if let Ok(found) = rxing::helpers::detect_multiple_in_luma(luma, img.w, img.h) {
        for r in found {
            let text = r.getText().to_string();
            if !text.is_empty() && !out.contains(&text) {
                out.push(text);
            }
        }
    }
    out
}

#[cfg(target_os = "windows")]
mod engine {
    use super::Line;
    use crate::model::{Image, R};
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;

    const NO_LANGUAGE: &str =
        "Windows has no text recognition language installed. Add one in Settings → Time & language → Language & region: choose a language → Language options → Optical character recognition.";

    pub fn lines(img: &Image) -> Result<Vec<Line>, String> {
        recognize(img).map_err(|e| {
            if OcrEngine::AvailableRecognizerLanguages().map(|l| l.Size().unwrap_or(0)).unwrap_or(0) == 0 {
                NO_LANGUAGE.to_string()
            } else {
                format!("Unable to perform the OCR requests: {}", e.message())
            }
        })
    }

    fn recognize(img: &Image) -> windows::core::Result<Vec<Line>> {
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let max = OcrEngine::MaxImageDimension().unwrap_or(10_000).max(1) as f32;
        // Small screen text reads better enlarged; big pictures have to fit the engine's limit.
        let longest = img.w.max(img.h) as f32;
        let factor = if longest * 2.0 <= max && (img.w as u64 * img.h as u64) <= 4_000_000 { 2.0 } else { (max / longest).min(1.0) };
        let scaled = if (factor - 1.0).abs() < f32::EPSILON { img.clone() } else { crate::render::resize(img, factor) };
        if scaled.w == 0 || scaled.h == 0 {
            return Ok(vec![]);
        }
        let mut bgra = scaled.px.to_vec();
        for p in bgra.chunks_exact_mut(4) {
            // Flatten onto white, as the export would show it, then swap to BGRA.
            let a = p[3] as u32;
            let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
            let (r, g, b) = (mix(p[0]), mix(p[1]), mix(p[2]));
            p.copy_from_slice(&[b, g, r, 255]);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let buffer = writer.DetachBuffer()?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, scaled.w as i32, scaled.h as i32)?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let found = result.Lines()?;
        let mut out = vec![];
        for i in 0..found.Size()? {
            let line = found.GetAt(i)?;
            let text = line.Text()?.to_string();
            let words = line.Words()?;
            let mut rect: Option<R> = None;
            for j in 0..words.Size()? {
                let b = words.GetAt(j)?.BoundingRect()?;
                let r = R { x0: b.X / factor, y0: b.Y / factor, x1: (b.X + b.Width) / factor, y1: (b.Y + b.Height) / factor };
                rect = Some(match rect {
                    Some(u) => R { x0: u.x0.min(r.x0), y0: u.y0.min(r.y0), x1: u.x1.max(r.x1), y1: u.y1.max(r.y1) },
                    None => r,
                });
            }
            if let Some(rect) = rect {
                out.push(Line { text, rect });
            }
        }
        Ok(out)
    }
}

#[cfg(target_os = "linux")]
mod engine {
    pub use super::ocrs_engine::lines;
}

/// ocrs, used on Linux, and compiled into Windows test builds so its tests run there too.
#[cfg(any(target_os = "linux", all(test, target_os = "windows")))]
pub mod ocrs_engine {
    use super::Line;
    use crate::model::{Image, R};
    use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
    use std::sync::{Mutex, OnceLock, PoisonError};

    static DETECTION: &[u8] = include_bytes!("../assets/ocr/text-detection.onnx");
    static RECOGNITION: &[u8] = include_bytes!("../assets/ocr/text-recognition.onnx");

    fn engine() -> Result<&'static Mutex<OcrEngine>, String> {
        static ENGINE: OnceLock<Result<Mutex<OcrEngine>, String>> = OnceLock::new();
        ENGINE
            .get_or_init(|| {
                let load = |data: &'static [u8]| rten::Model::load_static_slice(data).map_err(|e| e.to_string());
                OcrEngine::new(OcrEngineParams { detection_model: Some(load(DETECTION)?), recognition_model: Some(load(RECOGNITION)?), ..Default::default() })
                    .map(Mutex::new)
                    .map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(|e| format!("Unable to load the text recognition models: {e}"))
    }

    pub fn lines(img: &Image) -> Result<Vec<Line>, String> {
        let engine = engine()?.lock().unwrap_or_else(PoisonError::into_inner);
        let fail = |e: String| format!("Unable to perform the OCR requests: {e}");
        // ocrs reads RGB; transparent pixels show as the white of the export.
        let rgb: Vec<u8> = img
            .px
            .chunks_exact(4)
            .flat_map(|p| {
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                [mix(p[0]), mix(p[1]), mix(p[2])]
            })
            .collect();
        let source = ImageSource::from_bytes(&rgb, (img.w, img.h)).map_err(|e| fail(e.to_string()))?;
        let input = engine.prepare_input(source).map_err(|e| fail(e.to_string()))?;
        let words = engine.detect_words(&input).map_err(|e| fail(e.to_string()))?;
        let rows = engine.find_text_lines(&input, &words);
        let recognized = engine.recognize_text(&input, &rows).map_err(|e| fail(e.to_string()))?;
        let mut out = vec![];
        for line in recognized.into_iter().flatten() {
            let text = line.to_string().trim().to_string();
            if text.is_empty() {
                continue;
            }
            let b = line.bounding_rect();
            out.push(Line { text, rect: R { x0: b.left() as f32, y0: b.top() as f32, x1: b.right() as f32, y1: b.bottom() as f32 } });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{p, Doc, Fill, Kind, Obj, Style};
    use std::sync::Arc;

    /// "Hello ShotCraft" drawn by the editor's own text tool on a light background.
    fn text_picture() -> Image {
        let (w, h) = (900u32, 400u32);
        let base = Image { w, h, px: Arc::new([245u8, 246, 248, 255].repeat((w * h) as usize)) };
        let mut doc = Doc::new(base, 2.0);
        let mut st = Style::default();
        st.size = 6;
        st.fill = Fill::HandDrawn;
        st.color = [0, 0, 0, 255];
        doc.objs = vec![Obj { kind: Kind::Text { at: p(40.0, 280.0), text: "Hello ShotCraft".into() }, style: st }];
        crate::render::export(&doc, &crate::render::Fonts::load())
    }

    #[test]
    fn ocrs_reads_the_editors_text() {
        let lines = ocrs_engine::lines(&text_picture()).expect("ocrs runs");
        let text: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert!(text.join(" ").contains("ShotCraft"), "read: {text:?}");
        let r = lines.iter().find(|l| l.text.contains("ShotCraft")).map(|l| l.rect).expect("a box");
        assert!(r.y0 > 250.0 && r.y1 < 380.0 && r.x0 < 60.0 && r.x1 > 300.0 && r.x1 <= 900.0, "box around the text: {r:?}");
    }

    #[test]
    fn reads_a_qr_code() {
        use rxing::Writer;
        let m = rxing::MultiFormatWriter.encode("https://example.com/shotcraft", &rxing::BarcodeFormat::QR_CODE, 240, 240).expect("encode");
        let (w, h) = (m.width(), m.height());
        let px: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| if m.get(x, y) { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
        let img = Image { w, h, px: Arc::new(px) };
        assert_eq!(barcodes(&img), vec!["https://example.com/shotcraft".to_string()]);
    }

    #[test]
    fn odd_sizes_give_no_text_and_no_error() {
        assert!(recognize(&Image { w: 0, h: 0, px: Arc::new(vec![]) }).expect("empty").lines.is_empty());
        for (w, h) in [(1u32, 1u32), (3, 3), (2000, 2), (2, 2000)] {
            let img = Image { w, h, px: Arc::new(vec![255; (w * h * 4) as usize]) };
            assert!(barcodes(&img).is_empty());
            let _ = ocrs_engine::lines(&img);
        }
    }
}
