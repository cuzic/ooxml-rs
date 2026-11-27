//! BIFF8 shared strings (SST and CONTINUE records).

struct Strings<'a> {
    remaining: &'a [u8],
    payload: &'a [u8],
}

impl<'a> Strings<'a> {
    fn record(&mut self, expected: u16) -> Option<()> {
        let [a, b, c, d] = *self.remaining.first_chunk()?;
        if u16::from_le_bytes([a, b]) != expected {
            return None;
        }
        let size = usize::from(u16::from_le_bytes([c, d]));
        (self.payload, self.remaining) = self.remaining.get(4..)?.split_at_checked(size)?;
        Some(())
    }

    fn byte(&mut self) -> Option<u8> {
        while self.payload.is_empty() {
            self.record(0x003c)?;
        }
        let (&byte, rest) = self.payload.split_first()?;
        self.payload = rest;
        Some(byte)
    }

    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        if let Some((head, tail)) = self.payload.split_first_chunk::<N>() {
            self.payload = tail;
            return Some(*head);
        }
        let mut result = [0; N];
        for byte in &mut result {
            *byte = self.byte()?;
        }
        Some(result)
    }

    fn skip(&mut self, mut count: usize) -> Option<()> {
        while count > self.payload.len() {
            count -= self.payload.len();
            self.record(0x003c)?;
        }
        self.payload = self.payload.get(count..)?;
        Some(())
    }

    fn characters(&mut self, out: &mut Vec<u16>, mut count: usize, mut wide: bool) -> Option<()> {
        while count > 0 {
            if self.payload.is_empty() {
                wide = self.byte()? & 1 != 0;
            }
            let width = if wide { 2 } else { 1 };
            let units = count.min(self.payload.len() / width);
            if units == 0 {
                // A CONTINUE record can change character width between characters.
                // A wide character itself cannot span records.
                if !self.payload.is_empty() {
                    return None;
                }
                continue;
            }
            let (text, rest) = self.payload.split_at(units * width);
            self.payload = rest;
            if wide {
                out.extend(
                    text.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|unit| u16::from_le_bytes(*unit)),
                );
            } else {
                out.extend(text.iter().copied().map(u16::from));
            }
            count -= units;
        }
        Some(())
    }
}

/// Projects BIFF8 shared strings as UTF-16, each preceded by its 16-bit length.
/// The input begins at the SST record header and includes its CONTINUE records.
pub fn biff_shared_strings(records: &[u8]) -> Option<Vec<u16>> {
    let mut strings = Strings {
        remaining: records,
        payload: &[],
    };
    strings.record(0x00fc)?;
    strings.take::<4>()?; // total occurrences
    let unique = u32::from_le_bytes(strings.take()?) as usize;
    if unique > records.len() / 3 {
        return None;
    }
    let mut output = Vec::new();
    for _ in 0..unique {
        let length = u16::from_le_bytes(strings.take()?);
        let flags = strings.byte()?;
        let runs = if flags & 8 != 0 {
            usize::from(u16::from_le_bytes(strings.take()?))
        } else {
            0
        };
        let extension = if flags & 4 != 0 {
            u32::from_le_bytes(strings.take()?) as usize
        } else {
            0
        };
        output.push(length);
        strings.characters(&mut output, usize::from(length), flags & 1 != 0)?;
        strings.skip(extension.checked_add(runs * 4)?)?;
    }
    Some(output)
}

const MAX_PAYLOAD: usize = 8224;

struct RecordWriter {
    bytes: Vec<u8>,
    header: usize,
}

impl RecordWriter {
    fn new(total: u32, unique: u32) -> Self {
        let mut bytes = vec![0xfc, 0, 0, 0];
        bytes.extend(total.to_le_bytes());
        bytes.extend(unique.to_le_bytes());
        Self { bytes, header: 0 }
    }

    fn available(&self) -> usize {
        MAX_PAYLOAD - (self.bytes.len() - self.header - 4)
    }

    fn finish_record(&mut self) {
        let size = (MAX_PAYLOAD - self.available()) as u16;
        self.bytes[self.header + 2..self.header + 4].copy_from_slice(&size.to_le_bytes());
    }

    fn continue_record(&mut self) {
        self.finish_record();
        self.header = self.bytes.len();
        self.bytes.extend([0x3c, 0, 0, 0]);
    }

    fn string(&mut self, text: &[u16]) {
        if self.available() < 3 {
            self.continue_record();
        }
        self.bytes.extend((text.len() as u16).to_le_bytes());
        self.bytes.push(1);
        let mut text = text;
        while !text.is_empty() {
            if self.available() < 2 {
                self.continue_record();
                self.bytes.push(1);
            }
            let (chunk, rest) = text.split_at(text.len().min(self.available() / 2));
            self.bytes
                .extend(chunk.iter().flat_map(|unit| unit.to_le_bytes()));
            text = rest;
        }
    }
}

/// Encodes BIFF8 SST/CONTINUE records from UTF-16 string lengths and code units.
pub fn encode_biff_shared_strings(
    total: u32,
    lengths: &[u32],
    mut text: &[u16],
) -> Option<Vec<u8>> {
    let unique = u32::try_from(lengths.len()).ok()?;
    if unique > total {
        return None;
    }
    let mut output = RecordWriter::new(total, unique);
    for &length in lengths {
        if length > 32767 {
            return None;
        }
        let (value, rest) = text.split_at_checked(length as usize)?;
        output.string(value);
        text = rest;
    }
    if !text.is_empty() {
        return None;
    }
    output.finish_record();
    Some(output.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_round_trips_across_record_boundaries() {
        // Fill the first record through the last available wide character.
        let mut text = vec![0x540d; 4107];
        text.extend([0xd83d, 0xde00, 0xd800]);
        let records = encode_biff_shared_strings(4, &[0, 4107, 3], &text).unwrap();
        let mut expected = vec![0, 4107];
        expected.extend(&text[..4107]);
        expected.push(3);
        expected.extend(&text[4107..]);
        assert_eq!(biff_shared_strings(&records), Some(expected));
        assert!(encode_biff_shared_strings(2, &[0, 4107, 3], &text).is_none());
        assert!(encode_biff_shared_strings(1, &[4], &[0]).is_none());
    }

    #[test]
    fn reads_continued_text_with_a_new_character_width() {
        let records = [
            0xfc, 0, 12, 0, 1, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, b'A', 0x3c, 0, 5, 0, 1, 0x0d, 0x54,
            b'B', 0,
        ];
        assert_eq!(biff_shared_strings(&records), Some(vec![3, 65, 0x540d, 66]));
        assert_eq!(biff_shared_strings(&records[..records.len() - 1]), None);
    }
}
