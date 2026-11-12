//! Linux printing: one page of PDF with the picture on it, for the desktop's print portal
//! (`system::print_image`). The layout is the one macOS and Windows print with
//! (`menu_model::print_layout`): the picture at its on-screen size, shrunk to fit inside the
//! margins, centred, the page in landscape when the picture is wider than tall.

use crate::menu_model::print_layout;
use crate::model::Image;
use ashpd::zbus;
use std::io::Write;

/// Points per millimetre (a point is 1/72 inch).
const PT_PER_MM: f64 = 72.0 / 25.4;

/// A sheet of paper in points, portrait or landscape, and the margin kept free on every side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page {
    pub width: f64,
    pub height: f64,
    pub margin: f64,
}

/// A4 with quarter-inch margins: used when the print dialog leaves the paper size out.
pub const DEFAULT_PAGE: Page = Page { width: 595.28, height: 841.89, margin: 18.0 };

/// The page the print dialog chose, given in millimetres (`None`: not said). The widest margin
/// is kept on every side, as macOS does, so the picture stays centred.
pub fn page_from_mm(width: Option<f64>, height: Option<f64>, margins: [Option<f64>; 4]) -> Page {
    let (Some(w), Some(h)) = (width.filter(|w| *w > 0.0), height.filter(|h| *h > 0.0)) else {
        return DEFAULT_PAGE;
    };
    let margin = margins.iter().flatten().fold(None, |m: Option<f64>, x| Some(m.map_or(*x, |m| m.max(*x))));
    Page { width: w * PT_PER_MM, height: h * PT_PER_MM, margin: margin.map_or(DEFAULT_PAGE.margin, |m| m.max(0.0) * PT_PER_MM) }
}

/// What the print dialog answered (the portal's PreparePrint results): the token the print
/// request needs and the page chosen. Only these are read, and leniently: desktops differ in what
/// else they send and how (GNOME sends `pdf` for a format another reader wants as `PDF`, and leaves
/// the orientation out for some printers), and none of it changes the page.
pub fn prepared_page(results: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>) -> Option<(u32, Page)> {
    use zbus::zvariant::OwnedValue;
    let token = results.get("token")?.downcast_ref::<u32>().ok()?;
    let setup: std::collections::HashMap<String, OwnedValue> = results.get("page-setup").and_then(|v| v.try_clone().ok()).and_then(|v| v.try_into().ok()).unwrap_or_default();
    let mm = |key: &str| setup.get(key).and_then(|v| v.downcast_ref::<f64>().ok());
    Some((token, page_from_mm(mm("Width"), mm("Height"), [mm("MarginTop"), mm("MarginBottom"), mm("MarginLeft"), mm("MarginRight")])))
}

/// Where the picture lands, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub landscape: bool,
    /// The page as printed: `Page` turned for landscape.
    pub page_w: f64,
    pub page_h: f64,
    /// The picture's lower left corner (PDF counts from the bottom) and size.
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Lays a picture of `px` pixels at `scale` pixels per point out on `page`.
pub fn placement(px: (u32, u32), scale: f32, page: Page) -> Placement {
    let layout = print_layout(px, scale, (page.width, page.height), page.margin);
    let (short, long) = (page.width.min(page.height), page.width.max(page.height));
    let (page_w, page_h) = if layout.landscape { (long, short) } else { (short, long) };
    Placement { landscape: layout.landscape, page_w, page_h, x: (page_w - layout.width) / 2.0, y: (page_h - layout.height) / 2.0, w: layout.width, h: layout.height }
}

/// The picture's pixels as RGB on white: paper has no transparency.
fn rgb_on_white(img: &Image) -> Vec<u8> {
    img.px
        .chunks_exact(4)
        .flat_map(|p| {
            let a = u32::from(p[3]);
            [0, 1, 2].map(|i| ((u32::from(p[i]) * a + 255 * (255 - a) + 127) / 255) as u8)
        })
        .collect()
}

/// A one-page PDF with the picture laid out on `page` (see `placement`), losslessly compressed.
pub fn image_pdf(img: &Image, scale: f32, page: Page) -> Result<Vec<u8>, String> {
    if img.w == 0 || img.h == 0 || img.px.len() != img.w as usize * img.h as usize * 4 {
        return Err("the picture is empty".into());
    }
    let at = placement((img.w, img.h), scale, page);
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&rgb_on_white(img)).map_err(|e| e.to_string())?;
    let pixels = z.finish().map_err(|e| e.to_string())?;
    let content = format!("q\n{:.3} 0 0 {:.3} {:.3} {:.3} cm\n/Im0 Do\nQ\n", at.w, at.h, at.x, at.y);

    let mut pdf: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![];
    let mut object = |pdf: &mut Vec<u8>, body: &[u8]| {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    };
    object(&mut pdf, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut pdf, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let page_obj = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>", at.page_w, at.page_h);
    object(&mut pdf, page_obj.as_bytes());
    object(&mut pdf, format!("<< /Length {} >>\nstream\n{content}endstream", content.len()).as_bytes());
    let mut image = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", img.w, img.h, pixels.len()).into_bytes();
    image.extend_from_slice(&pixels);
    image.extend_from_slice(b"\nendstream");
    object(&mut pdf, &image);

    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    Ok(pdf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::Arc;

    fn image(w: u32, h: u32, px: Vec<u8>) -> Image {
        Image { w, h, px: Arc::new(px) }
    }

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn the_dialogs_page_is_turned_into_points_with_its_widest_margin() {
        let a4 = page_from_mm(Some(210.0), Some(297.0), [Some(6.35), Some(6.35), Some(10.0), None]);
        assert!((a4.width - 595.28).abs() < 0.01 && (a4.height - 841.89).abs() < 0.01, "{a4:?}");
        assert!((a4.margin - 10.0 * PT_PER_MM).abs() < 1e-9);
        assert_eq!(page_from_mm(None, Some(297.0), [None; 4]), DEFAULT_PAGE, "no paper size: A4");
        assert_eq!(page_from_mm(Some(210.0), Some(297.0), [None; 4]).margin, DEFAULT_PAGE.margin);
    }

    /// An answer like GNOME's: the settings carry `output-file-format` in lower case, and the page
    /// setup has no orientation (both made ashpd refuse the answer).
    fn gnome_answer(page_setup: Option<Vec<(&str, zbus::zvariant::Value<'static>)>>) -> std::collections::HashMap<String, zbus::zvariant::OwnedValue> {
        use zbus::zvariant::{OwnedValue, Value};
        let dict = |kv: Vec<(&str, Value<'static>)>| -> OwnedValue {
            let m: std::collections::HashMap<String, Value<'static>> = kv.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            OwnedValue::try_from(Value::from(m)).expect("dict")
        };
        let mut out = std::collections::HashMap::from([
            ("token".to_string(), OwnedValue::from(7u32)),
            ("settings".to_string(), dict(vec![("output-file-format", Value::from("pdf")), ("printer", Value::from("Office"))])),
        ]);
        if let Some(ps) = page_setup {
            out.insert("page-setup".to_string(), dict(ps));
        }
        out
    }

    #[test]
    fn the_dialogs_answer_is_read_leniently() {
        use zbus::zvariant::Value;
        let a4 = vec![("PPDName", Value::from("A4")), ("Width", Value::from(210.0)), ("Height", Value::from(297.0)), ("MarginTop", Value::from(6.35)), ("MarginBottom", Value::from(14.2)), ("MarginLeft", Value::from(6.35)), ("MarginRight", Value::from(6.35))];
        let (token, page) = prepared_page(&gnome_answer(Some(a4))).expect("read");
        assert_eq!(token, 7);
        assert!((page.width - 595.28).abs() < 0.01 && (page.margin - 14.2 * PT_PER_MM).abs() < 1e-9, "{page:?}");
        let (_, page) = prepared_page(&gnome_answer(None)).expect("no page setup is fine");
        assert_eq!(page, DEFAULT_PAGE);
        let mut no_token = gnome_answer(None);
        no_token.remove("token");
        assert_eq!(prepared_page(&no_token), None, "without the token there is no job to send");
    }

    #[test]
    fn a_wide_picture_prints_landscape_centred_inside_the_margins() {
        let at = placement((4000, 1000), 1.0, DEFAULT_PAGE);
        assert!(at.landscape);
        assert!(at.page_w > at.page_h, "the page is turned");
        assert!((at.x - (at.page_w - at.w) / 2.0).abs() < 1e-9 && (at.y - (at.page_h - at.h) / 2.0).abs() < 1e-9, "centred");
        assert!(at.x >= DEFAULT_PAGE.margin - 1e-9 && at.y >= DEFAULT_PAGE.margin - 1e-9, "inside the margins: {at:?}");
        assert!((at.w / at.h - 4.0).abs() < 1e-9, "not stretched");
    }

    #[test]
    fn a_small_picture_keeps_its_on_screen_size() {
        // 400x300 pixels at 2 pixels per point are 200x150 points on screen, and on paper.
        let at = placement((400, 300), 2.0, DEFAULT_PAGE);
        assert_eq!((at.w, at.h), (200.0, 150.0));
    }

    #[test]
    fn the_pdf_holds_the_picture_on_white_where_the_cross_reference_says() {
        // One opaque red pixel and one fully transparent one.
        let pdf = image_pdf(&image(2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0]), 1.0, DEFAULT_PAGE).expect("pdf");
        assert!(pdf.starts_with(b"%PDF-1.4\n") && pdf.ends_with(b"%%EOF\n"));
        assert!(find(&pdf, b"/Width 2 /Height 1 /ColorSpace /DeviceRGB").is_some());
        // Every entry of the cross-reference table points at its object.
        let xref = find(&pdf, b"\nxref\n").expect("xref") + 1;
        let table = String::from_utf8_lossy(&pdf[xref..]);
        let entries: Vec<usize> = table.lines().skip(3).take(5).map(|l| l[..10].parse().expect("offset")).collect();
        assert_eq!(entries.len(), 5, "five objects");
        let start_at: usize = table.lines().skip_while(|l| *l != "startxref").nth(1).expect("startxref").parse().expect("number");
        assert_eq!(start_at, xref, "startxref points at the table");
        for (i, at) in entries.iter().enumerate() {
            assert!(pdf[*at..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
        }
        // The image stream decompresses to red and white.
        let start = find(&pdf[entries[4]..], b"stream\n").expect("stream") + entries[4] + 7;
        let end = find(&pdf[start..], b"\nendstream").expect("end") + start;
        let mut rgb = vec![];
        flate2::read::ZlibDecoder::new(&pdf[start..end]).read_to_end(&mut rgb).expect("zlib");
        assert_eq!(rgb, vec![255, 0, 0, 255, 255, 255], "transparent prints as paper white");
    }

    #[test]
    fn an_empty_or_short_picture_is_refused_not_printed() {
        assert!(image_pdf(&image(0, 0, vec![]), 1.0, DEFAULT_PAGE).is_err());
        assert!(image_pdf(&image(2, 2, vec![0; 4]), 1.0, DEFAULT_PAGE).is_err());
    }
}
