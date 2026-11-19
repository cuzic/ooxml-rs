use super::{Error, Format, Result};

const EMU_PER_INCH: f64 = 914_400.0;
pub(super) const MAX_EMU: u64 = 27_273_042_316_900;

pub(super) fn read(bytes: &[u8]) -> Result<(Format, (u64, u64))> {
    let (format, dimensions) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        (Format::Png, png(bytes))
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        (Format::Jpeg, jpeg(bytes))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        (
            Format::Gif,
            (|| pixels(le16(bytes, 6)?.into(), le16(bytes, 8)?.into(), 96.0, 96.0))(),
        )
    } else if bytes.starts_with(b"BM") {
        (Format::Bmp, bmp(bytes))
    } else if bytes.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a])
        || bytes.starts_with(&[1, 0, 9, 0])
        || bytes.starts_with(&[2, 0, 9, 0])
    {
        (Format::Wmf, wmf(bytes))
    } else if le32(bytes, 0) == Some(1) && bytes.get(40..44) == Some(b" EMF") {
        (Format::Emf, emf(bytes))
    } else {
        return Err(Error::ImageFormat);
    };
    Ok((format, dimensions.ok_or(Error::ImageSize)?))
}

pub(super) fn valid_extent(width: u64, height: u64) -> bool {
    (1..=MAX_EMU).contains(&width) && (1..=MAX_EMU).contains(&height)
}

fn extent(x: f64, y: f64) -> Option<(u64, u64)> {
    let (x, y) = (x.round() as u64, y.round() as u64);
    valid_extent(x, y).then_some((x, y))
}

fn pixels(width: u32, height: u32, x_dpi: f64, y_dpi: f64) -> Option<(u64, u64)> {
    let dpi = |value: f64| if value < 1.0 { 96.0 } else { value };
    extent(
        f64::from(width) * EMU_PER_INCH / dpi(x_dpi),
        f64::from(height) * EMU_PER_INCH / dpi(y_dpi),
    )
}

fn take<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(take(bytes, at)?))
}
fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(take(bytes, at)?))
}
fn i32le(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(take(bytes, at)?))
}
fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(take(bytes, at)?))
}
fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(take(bytes, at)?))
}

fn png(bytes: &[u8]) -> Option<(u64, u64)> {
    if bytes.get(8..16)? != b"\0\0\0\rIHDR" || bytes.len() < 33 {
        return None;
    }
    let mut chunks = &bytes[33..];
    let mut dpi = (96.0, 96.0);
    while let Some(len) = be32(chunks, 0) {
        match chunks.get(4..8)? {
            b"pHYs" if len == 9 && chunks.get(16) == Some(&1) => {
                dpi = (
                    f64::from(be32(chunks, 8)?) * 0.0254,
                    f64::from(be32(chunks, 12)?) * 0.0254,
                );
                break;
            }
            b"IDAT" | b"IEND" => break,
            _ => chunks = chunks.get((len as usize).checked_add(12)?..)?,
        }
    }
    pixels(be32(bytes, 16)?, be32(bytes, 20)?, dpi.0, dpi.1)
}

fn jpeg(bytes: &[u8]) -> Option<(u64, u64)> {
    let mut rest = &bytes[2..];
    let mut dpi = (96.0, 96.0);
    let mut size = None;
    while let Some(markers) = rest.strip_prefix(&[0xff]) {
        rest = &markers[markers.iter().take_while(|&&byte| byte == 0xff).count()..];
        let (&marker, next) = rest.split_first()?;
        rest = next;
        match marker {
            0xda | 0xd9 => break,
            0x01 | 0xd0..=0xd7 => continue,
            _ => {}
        }
        let len = usize::from(be16(rest, 0)?);
        let segment = rest.get(2..len)?;
        rest = &rest[len..];
        match marker {
            0xe0 if segment.starts_with(b"JFIF\0") && segment.len() >= 12 => {
                let factor = match segment[7] {
                    1 => 1.0,
                    2 => 2.54,
                    _ => 0.0,
                };
                dpi = (
                    f64::from(be16(segment, 8)?) * factor,
                    f64::from(be16(segment, 10)?) * factor,
                );
            }
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                size = Some((be16(segment, 3)?, be16(segment, 1)?));
            }
            _ => {}
        }
    }
    let (width, height) = size?;
    pixels(width.into(), height.into(), dpi.0, dpi.1)
}

fn bmp(bytes: &[u8]) -> Option<(u64, u64)> {
    match le32(bytes, 14)? {
        12 => pixels(le16(bytes, 18)?.into(), le16(bytes, 20)?.into(), 96.0, 96.0),
        40.. => pixels(
            i32le(bytes, 18)?.try_into().ok()?,
            i32le(bytes, 22)?.unsigned_abs(),
            f64::from(i32le(bytes, 38)?) * 0.0254,
            f64::from(i32le(bytes, 42)?) * 0.0254,
        ),
        _ => None,
    }
}

fn emf(bytes: &[u8]) -> Option<(u64, u64)> {
    let width = f64::from(i32le(bytes, 32)?) - f64::from(i32le(bytes, 24)?);
    let height = f64::from(i32le(bytes, 36)?) - f64::from(i32le(bytes, 28)?);
    extent(width * 360.0, height * 360.0)
}

fn wmf(bytes: &[u8]) -> Option<(u64, u64)> {
    if !bytes.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]) || bytes.len() < 22 {
        return None;
    }
    let coord = |at| Some(f64::from(i16::from_le_bytes(take(bytes, at)?)));
    let dpi = f64::from(le16(bytes, 14)?);
    if dpi == 0.0 {
        return None;
    }
    extent(
        (coord(10)? - coord(6)?) * EMU_PER_INCH / dpi,
        (coord(12)? - coord(8)?) * EMU_PER_INCH / dpi,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_sizes_and_every_truncated_prefix() {
        let gif = b"GIF89a\x02\x00\x03\x00";
        let mut bmp = [0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&2_i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-3_i32).to_le_bytes());

        let mut emf = [0u8; 88];
        emf[0] = 1;
        emf[32..36].copy_from_slice(&2540_i32.to_le_bytes());
        emf[36..40].copy_from_slice(&1270_i32.to_le_bytes());
        emf[40..44].copy_from_slice(b" EMF");

        let mut wmf = [0u8; 22];
        wmf[..4].copy_from_slice(&[0xd7, 0xcd, 0xc6, 0x9a]);
        wmf[6..8].copy_from_slice(&(-10_i16).to_le_bytes());
        wmf[10..12].copy_from_slice(&90_i16.to_le_bytes());
        wmf[12..14].copy_from_slice(&50_i16.to_le_bytes());
        wmf[14..16].copy_from_slice(&100_u16.to_le_bytes());

        for (format, bytes, expected) in [
            (Format::Gif, gif.as_slice(), (19_050, 28_575)),
            (Format::Bmp, bmp.as_slice(), (19_050, 28_575)),
            (Format::Emf, emf.as_slice(), (914_400, 457_200)),
            (Format::Wmf, wmf.as_slice(), (914_400, 457_200)),
        ] {
            assert_eq!(read(bytes).unwrap(), (format, expected));
            for end in 0..bytes.len() {
                let _ = read(&bytes[..end]);
            }
        }
        std::assert_matches!(read(&[1, 0, 9, 0]), Err(Error::ImageSize));
        std::assert_matches!(read(b"unknown"), Err(Error::ImageFormat));
    }

    #[test]
    fn jpeg_accepts_all_sof_markers_and_rejects_zero_height() {
        for marker in (0xc0..=0xcf).filter(|&marker| !matches!(marker, 0xc4 | 0xc8 | 0xcc)) {
            let mut jpeg = [
                0xff, 0xd8, 0xff, 0xff, marker, 0, 11, 8, 0, 3, 0, 2, 1, 1, 0x11, 0, 0xff, 0xd9,
            ];
            assert_eq!(read(&jpeg).unwrap(), (Format::Jpeg, (19_050, 28_575)));
            jpeg[9] = 0;
            std::assert_matches!(read(&jpeg), Err(Error::ImageSize));
            for end in 0..jpeg.len() {
                let _ = read(&jpeg[..end]);
            }
        }
    }

    #[test]
    fn png_density_and_extent_limits() {
        let fixture = *b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x10\0\0\0\x0c\x08\x02\0\0\0\0\0\0\0";
        for end in 0..fixture.len() {
            let _ = read(&fixture[..end]);
        }
        let mut png = fixture[..33].to_vec();
        png.extend_from_slice(b"\0\0\0\x09pHYs");
        png.extend_from_slice(&4000_u32.to_be_bytes());
        png.extend_from_slice(&2000_u32.to_be_bytes());
        png.extend_from_slice(&[1, 0, 0, 0, 0]);
        png.extend_from_slice(b"\0\0\0\0IEND\0\0\0\0");
        assert_eq!(read(&png).unwrap().1, (144_000, 216_000));
        png[49] = 0;
        assert_eq!(read(&png).unwrap().1, (152_400, 114_300));
        png[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        std::assert_matches!(read(&png), Err(Error::ImageSize));
    }
}
