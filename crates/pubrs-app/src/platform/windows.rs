//! Windows: the system print dialog (PrintDlgEx) and GDI printing. Pages
//! of the exported PDF are rasterised by the operating system's own PDF
//! renderer (Windows.Data.Pdf), so pubrs does not rasterise for printing.

use std::path::Path;

use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode,
};
use windows::Storage::StorageFile;
use windows::Storage::Streams::InMemoryRandomAccessStream;
use windows::Win32::Foundation::{GlobalFree, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps, HORZRES,
    LOGPIXELSX, LOGPIXELSY, SRCCOPY, StretchDIBits, VERTRES,
};
use windows::Win32::Storage::Xps::{DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::Win32::UI::Controls::Dialogs::{
    PD_NOCURRENTPAGE, PD_NOPAGENUMS, PD_NOSELECTION, PD_RESULT_PRINT, PD_RETURNDC,
    PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGEXW, PrintDlgExW, START_PAGE_GENERAL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::core::{HSTRING, PCWSTR};

use super::{PrintBackend, PrintOptions, PrintOutcome, PrinterInfo};

pub struct WindowsPrint;

/// A rasterised page: top-down BGRA pixels and its size in inches.
struct PageImage {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    inches: (f64, f64),
}

/// Rasterise every page of `pdf` at `dpi` with Windows.Data.Pdf on a
/// multithreaded-apartment worker thread.
fn rasterise(pdf: &Path, dpi: (f64, f64)) -> Result<Vec<PageImage>, String> {
    let path = pdf.to_path_buf();
    std::thread::spawn(move || -> Result<Vec<PageImage>, String> {
        // SAFETY: balanced with RoUninitialize at the end of this thread.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
        let result = (|| -> windows::core::Result<Vec<PageImage>> {
            let file =
                StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.join()?;
            let doc = PdfDocument::LoadFromFileAsync(&file)?.join()?;
            let mut pages = Vec::new();
            for i in 0..doc.PageCount()? {
                let page = doc.GetPage(i)?;
                let size = page.Size()?; // device-independent pixels (1/96 in)
                let inches = (f64::from(size.Width) / 96.0, f64::from(size.Height) / 96.0);
                let scale = (6000.0 / (inches.0 * dpi.0).max(inches.1 * dpi.1)).min(1.0);
                let w = ((inches.0 * dpi.0 * scale).round() as u32).max(1);
                let h = ((inches.1 * dpi.1 * scale).round() as u32).max(1);
                let options = PdfPageRenderOptions::new()?;
                options.SetDestinationWidth(w)?;
                options.SetDestinationHeight(h)?;
                options.SetBitmapEncoderId(BitmapEncoder::BmpEncoderId()?)?;
                let stream = InMemoryRandomAccessStream::new()?;
                page.RenderWithOptionsToStreamAsync(&stream, &options)?
                    .join()?;
                let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
                let pixels = decoder
                    .GetPixelDataTransformedAsync(
                        BitmapPixelFormat::Bgra8,
                        BitmapAlphaMode::Ignore,
                        &BitmapTransform::new()?,
                        ExifOrientationMode::IgnoreExifOrientation,
                        ColorManagementMode::DoNotColorManage,
                    )?
                    .join()?
                    .DetachPixelData()?;
                pages.push(PageImage {
                    width: decoder.PixelWidth()?,
                    height: decoder.PixelHeight()?,
                    bgra: pixels.to_vec(),
                    inches,
                });
            }
            Ok(pages)
        })();
        // SAFETY: paired with the RoInitialize above.
        unsafe { RoUninitialize() };
        result.map_err(|e| format!("Windows could not render the PDF: {e}"))
    })
    .join()
    .map_err(|_| "PDF rendering thread failed".to_string())?
}

impl PrintBackend for WindowsPrint {
    fn name(&self) -> &'static str {
        "Windows print dialog (GDI)"
    }

    fn has_system_dialog(&self) -> bool {
        true
    }

    fn printers(&self) -> Vec<PrinterInfo> {
        Vec::new()
    }

    fn print_pdf(
        &self,
        pdf: &Path,
        job_name: &str,
        _options: &PrintOptions,
    ) -> Result<PrintOutcome, String> {
        // SAFETY: plain Win32 calls with properly initialised structures; every
        // handle returned by the dialog is released below.
        unsafe {
            let owner: HWND = GetActiveWindow();
            let mut dlg = PRINTDLGEXW {
                lStructSize: std::mem::size_of::<PRINTDLGEXW>() as u32,
                hwndOwner: owner,
                Flags: PD_RETURNDC
                    | PD_NOPAGENUMS
                    | PD_NOCURRENTPAGE
                    | PD_NOSELECTION
                    | PD_USEDEVMODECOPIESANDCOLLATE,
                nCopies: 1,
                nStartPage: START_PAGE_GENERAL,
                ..Default::default()
            };
            PrintDlgExW(&mut dlg).map_err(|e| format!("print dialog failed: {e}"))?;
            let hdc = dlg.hDC;
            let free = |dlg: &PRINTDLGEXW| {
                if !dlg.hDevMode.is_invalid() {
                    let _ = GlobalFree(Some(dlg.hDevMode));
                }
                if !dlg.hDevNames.is_invalid() {
                    let _ = GlobalFree(Some(dlg.hDevNames));
                }
            };
            if dlg.dwResultAction != PD_RESULT_PRINT || hdc.is_invalid() {
                if !hdc.is_invalid() {
                    let _ = DeleteDC(hdc);
                }
                free(&dlg);
                return Ok(PrintOutcome::Cancelled);
            }
            let dpi = (
                f64::from(GetDeviceCaps(Some(hdc), LOGPIXELSX)),
                f64::from(GetDeviceCaps(Some(hdc), LOGPIXELSY)),
            );
            let area = (
                f64::from(GetDeviceCaps(Some(hdc), HORZRES)),
                f64::from(GetDeviceCaps(Some(hdc), VERTRES)),
            );
            let outcome = (|| -> Result<PrintOutcome, String> {
                let pages = rasterise(pdf, dpi)?;
                let name: Vec<u16> = job_name.encode_utf16().chain(std::iter::once(0)).collect();
                let info = DOCINFOW {
                    cbSize: std::mem::size_of::<DOCINFOW>() as i32,
                    lpszDocName: PCWSTR(name.as_ptr()),
                    ..Default::default()
                };
                if StartDocW(hdc, &info) <= 0 {
                    return Err("the printer refused the job".into());
                }
                for p in &pages {
                    // Actual size, centred, shrunk to fit the printable area if needed.
                    let mut w = p.inches.0 * dpi.0;
                    let mut h = p.inches.1 * dpi.1;
                    let fit = (area.0 / w).min(area.1 / h).min(1.0);
                    w *= fit;
                    h *= fit;
                    let x = ((area.0 - w) / 2.0).max(0.0);
                    let y = ((area.1 - h) / 2.0).max(0.0);
                    let bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: p.width as i32,
                            biHeight: -(p.height as i32),
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    StartPage(hdc);
                    StretchDIBits(
                        hdc,
                        x as i32,
                        y as i32,
                        w as i32,
                        h as i32,
                        0,
                        0,
                        p.width as i32,
                        p.height as i32,
                        Some(p.bgra.as_ptr().cast()),
                        &bmi,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                    EndPage(hdc);
                }
                EndDoc(hdc);
                Ok(PrintOutcome::Printed)
            })();
            let _ = DeleteDC(hdc);
            free(&dlg);
            outcome
        }
    }
}
