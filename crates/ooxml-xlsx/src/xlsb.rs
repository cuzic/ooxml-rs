//! BIFF12 worksheet records used by the XLSB adapter.

use std::io::{Error, ErrorKind, Result};

const BEGIN_FILTER: u32 = 0xa1;
const END_FILTER: u32 = 0xa2;
const MERGE: u32 = 0xb0;
const BEGIN_MERGES: u32 = 0xb1;
const END_MERGES: u32 = 0xb2;
const MARGINS: u32 = 0x1dc;
const HYPERLINK: u32 = 0x1ee;

fn invalid() -> Error {
    Error::new(ErrorKind::InvalidData, "Invalid XLSB worksheet record")
}

/// A zero-based inclusive rectangle in BIFF12 row-first order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range(pub [u32; 4]);

impl Range {
    fn read(payload: &[u8]) -> Result<Self> {
        Ok(Self([
            u32_at(payload, 0)?,
            u32_at(payload, 4)?,
            u32_at(payload, 8)?,
            u32_at(payload, 12)?,
        ]))
    }

    fn write(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(self.0) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    read_at(bytes, offset).map(u32::from_le_bytes)
}

fn read_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    bytes
        .get(offset..)
        .and_then(|value| value.first_chunk::<N>())
        .copied()
        .ok_or_else(invalid)
}

fn read_integer(bytes: &mut &[u8], max_bytes: usize) -> Result<u32> {
    let mut value = 0;
    for shift in 0..max_bytes {
        let (&byte, rest) = bytes.split_first().ok_or_else(invalid)?;
        *bytes = rest;
        value |= u32::from(byte & 0x7f) << (7 * shift);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(invalid())
}

fn read_record<'a>(records: &mut &'a [u8]) -> Result<(u32, &'a [u8])> {
    let id = read_integer(records, 2)?;
    let length = read_integer(records, 4)? as usize;
    let (payload, rest) = records.split_at_checked(length).ok_or_else(invalid)?;
    *records = rest;
    Ok((id, payload))
}

/// Sheet catalogue and internal formula reference table from workbook.bin.
#[derive(Debug, Default, PartialEq)]
pub struct WorkbookInfo {
    pub date1904: bool,
    /// Visibility, relationship length and sheet name length for each sheet.
    pub sheets: Vec<u32>,
    pub text: Vec<u16>,
    /// First and last sheet for internal references, or two negative sentinels.
    pub references: Vec<i32>,
    /// Flags, sheet index, name length, formula length and comment length for each name.
    pub names: Vec<u32>,
    pub name_text: Vec<u16>,
    pub name_formulas: Vec<u8>,
}

impl WorkbookInfo {
    pub fn read(records: &[u8]) -> Result<Self> {
        Self::read_records(records, true)
    }

    /// Reads visibility, sheet names, relationships and workbook date settings.
    pub fn read_catalogue(records: &[u8]) -> Result<Self> {
        Self::read_records(records, false)
    }

    fn read_records(mut records: &[u8], formulas: bool) -> Result<Self> {
        let mut result = Self::default();
        let mut sources = Vec::new();
        let mut references: Vec<&[u8; 12]> = Vec::new();
        while !records.is_empty() {
            let (id, payload) = read_record(&mut records)?;
            match id {
                0x27 if formulas => result.read_name(payload)?,
                0x99 => result.date1904 = u32_at(payload, 0)? & 1 != 0,
                0x163 | 0x165 | 0x166 | 0x29b if formulas => sources.push(id),
                0x16a if formulas => {
                    let count = u32_at(payload, 0)?;
                    let (entries, trailing) = split_units(&payload[4..], count, 12)?;
                    if !trailing.is_empty() {
                        return Err(invalid());
                    }
                    references.extend(entries.as_chunks::<12>().0);
                }
                0x9c => {
                    let hidden = u32_at(payload, 0)?;
                    let (relationship, rest) = wide_units(payload.get(8..).ok_or_else(invalid)?)?;
                    let (name, _) = wide_units(rest)?;
                    result.sheets.extend_from_slice(&[
                        hidden,
                        (relationship.len() / 2) as u32,
                        (name.len() / 2) as u32,
                    ]);
                    result.text.extend(
                        relationship
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .chain(name.as_chunks::<2>().0)
                            .copied()
                            .map(u16::from_le_bytes),
                    );
                }
                _ => {}
            }
        }
        for entry in references {
            let source = u32_at(entry, 0)? as usize;
            let bounds = if sources.get(source) == Some(&0x165) {
                [
                    i32::from_le_bytes(read_at(entry, 4)?),
                    i32::from_le_bytes(read_at(entry, 8)?),
                ]
            } else {
                [-1, -1]
            };
            result.references.extend_from_slice(&bounds);
        }
        Ok(result)
    }
    fn read_name(&mut self, payload: &[u8]) -> Result<()> {
        let flags = u32_at(payload, 0)?;
        let sheet = u32_at(payload, 5)?;
        let (name, rest) = wide_units(payload.get(9..).ok_or_else(invalid)?)?;
        let length = u32_at(rest, 0)?;
        let (formula, rest) = split_units(&rest[4..], length, 1)?;
        let extra_length = u32_at(rest, 0)?;
        let (_, rest) = split_units(&rest[4..], extra_length, 1)?;
        let (comment, _) = wide_units(rest)?;
        self.names.extend_from_slice(&[
            flags,
            sheet,
            (name.len() / 2) as u32,
            length,
            (comment.len() / 2) as u32,
        ]);
        self.name_text.extend(
            name.as_chunks::<2>()
                .0
                .iter()
                .chain(comment.as_chunks::<2>().0)
                .copied()
                .map(u16::from_le_bytes),
        );
        self.name_formulas.extend_from_slice(formula);
        Ok(())
    }
}

/// Writes the workbook catalogue and internal formula reference table.
pub fn write_workbook(
    date1904: bool,
    visibility: &[u32],
    lengths: &[u32],
    names: &[u16],
    references: &[u32],
) -> Result<Vec<u8>> {
    write_workbook_with_names(
        date1904,
        visibility,
        lengths,
        names,
        references,
        DefinedNames::default(),
    )
}

/// Packed workbook name fields, UTF-16 labels and comments, and formula tokens.
#[derive(Default)]
pub struct DefinedNames<'a> {
    /// Flags, sheet index, label length, formula length and comment length.
    pub fields: &'a [u32],
    pub text: &'a [u16],
    pub formulas: &'a [u8],
}

pub fn write_workbook_with_names(
    date1904: bool,
    visibility: &[u32],
    lengths: &[u32],
    mut names: &[u16],
    references: &[u32],
    defined_names: DefinedNames<'_>,
) -> Result<Vec<u8>> {
    let sheet_count = u32::try_from(lengths.len()).map_err(|_| invalid())?;
    if sheet_count == 0
        || lengths.len() != visibility.len()
        || references.len() > 65536
        || references.iter().any(|&index| index >= sheet_count)
    {
        return Err(invalid());
    }
    let mut output = Vec::new();
    write_record(&mut output, 0x83, &[]);
    let mut properties = [0; 12];
    properties[0] = u8::from(date1904);
    write_record(&mut output, 0x99, &properties);
    write_record(&mut output, 0x8f, &[]);
    let mut payload = Vec::new();
    for (index, (&hidden, &length)) in visibility.iter().zip(lengths).enumerate() {
        if length == 0 || length > 31 || hidden > 2 {
            return Err(invalid());
        }
        let (name, rest) = names
            .split_at_checked(length as usize)
            .ok_or_else(invalid)?;
        names = rest;
        let id = index as u32 + 1;
        let relationship = format!("rId{id}");
        payload.clear();
        payload.extend_from_slice(&hidden.to_le_bytes());
        payload.extend_from_slice(&id.to_le_bytes());
        payload.extend_from_slice(&(relationship.len() as u32).to_le_bytes());
        payload.extend(relationship.encode_utf16().flat_map(u16::to_le_bytes));
        payload.extend_from_slice(&length.to_le_bytes());
        payload.extend(name.iter().copied().flat_map(u16::to_le_bytes));
        write_record(&mut output, 0x9c, &payload);
    }
    if !names.is_empty() {
        return Err(invalid());
    }
    write_record(&mut output, 0x90, &[]);
    if !references.is_empty() {
        write_record(&mut output, 0x161, &[]);
        write_record(&mut output, 0x165, &[]);
        payload.clear();
        payload.extend_from_slice(&(references.len() as u32).to_le_bytes());
        for &sheet in references {
            payload.extend_from_slice(&0_u32.to_le_bytes());
            payload.extend_from_slice(&sheet.to_le_bytes());
            payload.extend_from_slice(&sheet.to_le_bytes());
        }
        write_record(&mut output, 0x16a, &payload);
        write_record(&mut output, 0x162, &[]);
    }
    defined_names.write(&mut output, &mut payload, sheet_count)?;
    write_record(&mut output, 0x84, &[]);
    Ok(output)
}

impl DefinedNames<'_> {
    fn write(
        mut self,
        output: &mut Vec<u8>,
        payload: &mut Vec<u8>,
        sheet_count: u32,
    ) -> Result<()> {
        let (fields, trailing) = self.fields.as_chunks::<5>();
        if !trailing.is_empty() {
            return Err(invalid());
        }
        for &[flags, sheet, name_length, formula_length, comment_length] in fields {
            if !matches!(flags, 0 | 0x20)
                || (sheet != u32::MAX && sheet >= sheet_count)
                || !(1..=255).contains(&name_length)
                || !(1..=16384).contains(&formula_length)
                || comment_length > 255
            {
                return Err(invalid());
            }
            let (name, rest) = self
                .text
                .split_at_checked(name_length as usize)
                .ok_or_else(invalid)?;
            let (comment, rest) = rest
                .split_at_checked(comment_length as usize)
                .ok_or_else(invalid)?;
            self.text = rest;
            let (formula, rest) = self
                .formulas
                .split_at_checked(formula_length as usize)
                .ok_or_else(invalid)?;
            self.formulas = rest;
            payload.clear();
            payload.extend_from_slice(&flags.to_le_bytes());
            payload.push(0);
            payload.extend_from_slice(&sheet.to_le_bytes());
            payload.extend_from_slice(&name_length.to_le_bytes());
            payload.extend(name.iter().copied().flat_map(u16::to_le_bytes));
            payload.extend_from_slice(&formula_length.to_le_bytes());
            payload.extend_from_slice(formula);
            payload.extend_from_slice(&0_u32.to_le_bytes());
            let comment_size = if comment.is_empty() {
                u32::MAX
            } else {
                comment_length
            };
            payload.extend_from_slice(&comment_size.to_le_bytes());
            payload.extend(comment.iter().copied().flat_map(u16::to_le_bytes));
            write_record(output, 0x27, payload);
        }
        if !self.text.is_empty() || !self.formulas.is_empty() {
            return Err(invalid());
        }
        Ok(())
    }
}

fn wide_units(payload: &[u8]) -> Result<(&[u8], &[u8])> {
    let length = u32_at(payload, 0)?;
    split_units(
        &payload[4..],
        if length == u32::MAX { 0 } else { length },
        2,
    )
}

/// Reads calculation-chain entries as sheet ID, row, column and flags.
pub fn read_calculation_chain(mut records: &[u8]) -> Result<Vec<[u32; 4]>> {
    let mut entries = Vec::new();
    while !records.is_empty() {
        let (id, payload) = read_record(&mut records)?;
        if id == 0x3f {
            let sheet = u32_at(payload, 0)?;
            let row = u32_at(payload, 4)?;
            let column = u32_at(payload, 8)?;
            let flags = *payload.get(12).ok_or_else(invalid)?;
            if row >= 1_048_576 || column >= 16_384 {
                return Err(invalid());
            }
            entries.push([sheet, row, column, u32::from(flags)]);
        }
    }
    Ok(entries)
}

/// Shared string table in workbook order.
#[derive(Debug, Default, PartialEq)]
pub struct SharedStrings {
    /// The UTF-16 length and run count for each entry.
    pub lengths: Vec<u32>,
    /// UTF-16 text followed by pairs of character and font indices.
    pub units: Vec<u16>,
}

impl SharedStrings {
    pub fn read(mut records: &[u8]) -> Result<Self> {
        let mut result = Self::default();
        while !records.is_empty() {
            let (id, payload) = read_record(&mut records)?;
            if id != 0x13 {
                continue;
            }
            let flags = *payload.first().ok_or_else(invalid)?;
            let length = u32_at(payload, 1)?;
            let length = if length == u32::MAX { 0 } else { length };
            let (text, rest) = split_units(&payload[5..], length, 2)?;
            let (count, runs) = if flags & 1 == 0 {
                (1, &[0_u8; 4][..])
            } else {
                let count = u32_at(rest, 0)?;
                (count, split_units(&rest[4..], count, 4)?.0)
            };
            result.lengths.extend_from_slice(&[length, count]);
            result.units.extend(
                text.as_chunks::<2>()
                    .0
                    .iter()
                    .chain(runs.as_chunks::<2>().0)
                    .copied()
                    .map(u16::from_le_bytes),
            );
        }
        Ok(result)
    }
}

/// Encodes a shared-string table with plain UTF-16 strings in index order.
pub fn write_shared_strings(total: u32, lengths: &[u32], mut units: &[u16]) -> Result<Vec<u8>> {
    let unique = u32::try_from(lengths.len()).map_err(|_| invalid())?;
    if total < unique {
        return Err(invalid());
    }
    let mut output = Vec::new();
    let mut header = [0; 8];
    header[..4].copy_from_slice(&total.to_le_bytes());
    header[4..].copy_from_slice(&unique.to_le_bytes());
    write_record(&mut output, 0x9f, &header);
    let mut payload = Vec::new();
    for &length in lengths {
        if length > 32767 {
            return Err(invalid());
        }
        let (value, remaining) = units
            .split_at_checked(length as usize)
            .ok_or_else(invalid)?;
        units = remaining;
        payload.clear();
        payload.push(0);
        payload.extend_from_slice(&length.to_le_bytes());
        payload.extend(value.iter().copied().flat_map(u16::to_le_bytes));
        write_record(&mut output, 0x13, &payload);
    }
    if !units.is_empty() {
        return Err(invalid());
    }
    write_record(&mut output, 0xa0, &[]);
    Ok(output)
}

fn split_units(payload: &[u8], count: u32, width: usize) -> Result<(&[u8], &[u8])> {
    let length = (count as usize).checked_mul(width).ok_or_else(invalid)?;
    payload.split_at_checked(length).ok_or_else(invalid)
}

/// Number formats and the format ID used by each cell XF.
#[derive(Debug, Default, PartialEq)]
pub struct Styles {
    /// Pairs of custom format ID and UTF-16 length.
    pub formats: Vec<u32>,
    pub text: Vec<u16>,
    pub cell_formats: Vec<u16>,
}

impl Styles {
    pub fn read(mut records: &[u8]) -> Result<Self> {
        let mut result = Self::default();
        let mut in_cell_formats = false;
        while !records.is_empty() {
            let (id, payload) = read_record(&mut records)?;
            match id {
                0x2c => {
                    let format = u16::from_le_bytes(read_at(payload, 0)?);
                    let length = u32_at(payload, 2)?;
                    let length = if length == u32::MAX { 0 } else { length };
                    let (text, _) = split_units(&payload[6..], length, 2)?;
                    result
                        .formats
                        .extend_from_slice(&[u32::from(format), length]);
                    result.text.extend(
                        text.as_chunks::<2>()
                            .0
                            .iter()
                            .copied()
                            .map(u16::from_le_bytes),
                    );
                }
                0x269 => in_cell_formats = true,
                0x26a => in_cell_formats = false,
                0x2f if in_cell_formats => {
                    result
                        .cell_formats
                        .push(u16::from_le_bytes(read_at(payload, 2)?));
                }
                _ => {}
            }
        }
        Ok(result)
    }
}

/// Writes number-format records and cell XFs. Negative IDs designate custom formats.
pub fn write_styles(ids: &[i32], custom_lengths: &[u32], mut text: &[u16]) -> Result<Vec<u8>> {
    let custom_count = ids.iter().filter(|&&id| id == -1).count();
    if custom_count != custom_lengths.len()
        || custom_count > usize::from(u16::MAX - 164) + 1
        || ids.iter().any(|&id| !(-1..164).contains(&id))
    {
        return Err(invalid());
    }
    let cell_count = u32::try_from(ids.len()).map_err(|_| invalid())?;
    let mut output = Vec::new();
    write_record(&mut output, 0x116, &[]);
    if custom_count != 0 {
        write_record(&mut output, 0x267, &(custom_count as u32).to_le_bytes());
        let mut record = Vec::new();
        for (index, &length) in custom_lengths.iter().enumerate() {
            let (value, rest) = text.split_at_checked(length as usize).ok_or_else(invalid)?;
            text = rest;
            record.clear();
            record.extend_from_slice(&(164 + index as u16).to_le_bytes());
            record.extend_from_slice(&length.to_le_bytes());
            record.extend(value.iter().copied().flat_map(u16::to_le_bytes));
            write_record(&mut output, 0x2c, &record);
        }
        write_record(&mut output, 0x268, &[]);
    }
    if !text.is_empty() {
        return Err(invalid());
    }
    write_record(&mut output, 0x269, &cell_count.to_le_bytes());
    let mut next = 164_u32;
    for &id in ids {
        let format = if id >= 0 {
            id as u16
        } else {
            let value = next as u16;
            next += 1;
            value
        };
        let mut xf = [0; 16];
        xf[2..4].copy_from_slice(&format.to_le_bytes());
        write_record(&mut output, 0x2f, &xf);
    }
    write_record(&mut output, 0x26a, &[]);
    write_record(&mut output, 0x117, &[]);
    Ok(output)
}

/// An XLSB worksheet decoded in record order.
#[derive(Debug, Default, PartialEq)]
pub struct Cells {
    /// Each 16-byte entry stores row/style, column/style/kind, then the value.
    pub values: Vec<u8>,
    pub text: Vec<u8>,
    pub formulas: Vec<u8>,
}

impl Cells {
    fn push(
        &mut self,
        id: u32,
        row: u32,
        column: u32,
        style: u32,
        value: &[u8],
        formula: bool,
    ) -> Result<()> {
        let (kind, bits, end) = match id {
            1 => (0, 0, 0),
            2 => {
                let raw = u32_at(value, 0)?;
                let mut number = if raw & 2 != 0 {
                    ((raw as i32) >> 2) as f64
                } else {
                    f64::from_bits(u64::from(raw & !3) << 32)
                };
                if raw & 1 != 0 {
                    number /= 100.0;
                }
                (1, number.to_bits(), 4)
            }
            3 | 11 | 4 | 10 => (
                if id == 3 || id == 11 { 2 } else { 3 },
                u64::from(*value.first().ok_or_else(invalid)?),
                1,
            ),
            5 | 9 => (1, u64::from_le_bytes(read_at(value, 0)?), 8),
            6 | 8 | 0x3e => {
                let start = usize::from(id == 0x3e);
                let count = u32_at(value, start)?;
                let length = if count == u32::MAX {
                    0
                } else {
                    usize::try_from(u64::from(count) * 2).map_err(|_| invalid())?
                };
                let end = (start + 4).checked_add(length).ok_or_else(invalid)?;
                let text = value.get(start + 4..end).ok_or_else(invalid)?;
                let offset = u32::try_from(self.text.len()).map_err(|_| invalid())?;
                self.text.extend_from_slice(text);
                (4, u64::from(offset) | ((length as u64) << 32), end)
            }
            7 => (5, u64::from(u32_at(value, 0)?), 4),
            _ => return Err(invalid()),
        };
        if formula && (8..=11).contains(&id) {
            self.push_formula(0, row, column, value.get(end + 2..).ok_or_else(invalid)?)?;
        }
        self.values
            .extend_from_slice(&(row | ((style & 0xfff) << 20)).to_le_bytes());
        self.values
            .extend_from_slice(&(column | ((style >> 12) << 14) | (kind << 26)).to_le_bytes());
        self.values.extend_from_slice(&bits.to_le_bytes());
        Ok(())
    }

    fn push_formula(&mut self, kind: u32, row: u32, column: u32, value: &[u8]) -> Result<()> {
        let offset: usize = if kind == 1 {
            17
        } else if kind == 2 {
            16
        } else {
            0
        };
        let tokens = u32_at(value, offset)? as usize;
        let extra_offset = (offset + 4).checked_add(tokens).ok_or_else(invalid)?;
        let extra = u32_at(value, extra_offset)? as usize;
        let end = (extra_offset + 4).checked_add(extra).ok_or_else(invalid)?;
        let value = value.get(..end).ok_or_else(invalid)?;
        for field in [
            kind,
            row,
            column,
            u32::try_from(end).map_err(|_| invalid())?,
        ] {
            self.formulas.extend_from_slice(&field.to_le_bytes());
        }
        self.formulas.extend_from_slice(value);
        Ok(())
    }
}

/// Layout, cell values and formula payloads for a single worksheet.
#[derive(Debug, Default, PartialEq)]
pub struct SheetProjection {
    pub cells: Cells,
    pub dimensions: Option<Range>,
    /// Row index, height in twentieths of a point and flags.
    pub rows: Vec<[u32; 3]>,
    /// First column, last column, width in 1/256 units and flags.
    pub columns: Vec<[u32; 4]>,
    pub tail: SheetTail,
}

impl SheetProjection {
    pub fn read(
        bytes: &[u8],
        row_limit: Option<u32>,
        column_styles: bool,
        stubs: bool,
        formulas: bool,
    ) -> Result<Self> {
        let mut projection = Self::default();
        let mut records = bytes;
        let (mut row, mut column) = (None, None);
        while !records.is_empty() {
            let (id, payload) = read_record(&mut records)?;
            match id {
                0 => {
                    let index = u32_at(payload, 0)?;
                    if index >= 1_048_576 {
                        return Err(invalid());
                    }
                    row = Some(index);
                    column = None;
                    let flags = u32::from(*payload.get(11).ok_or_else(invalid)?);
                    if flags & 0x37 != 0 {
                        let height = u32::from(u16::from_le_bytes(read_at(payload, 8)?));
                        projection.rows.push([index, height, flags]);
                    }
                    if row_limit.is_some_and(|limit| index >= limit) {
                        return Ok(projection);
                    }
                }
                0x3c if column_styles => {
                    let first = u32_at(payload, 0)?;
                    let last = u32_at(payload, 4)?;
                    if first > last || last >= 16_384 {
                        return Err(invalid());
                    }
                    let width = u32_at(payload, 8)?;
                    let flags = u32::from(u16::from_le_bytes(read_at(payload, 16)?));
                    projection.columns.push([first, last, width, flags]);
                }
                0x94 => projection.dimensions = Some(Range::read(payload)?),
                1..=0x12 | 0x3e if row.is_some() => {
                    let short = (0x0c..=0x12).contains(&id);
                    let (index, header, kind) = if short {
                        (column.map_or(0, |previous| previous + 1), 0, id - 0x0b)
                    } else {
                        (u32_at(payload, 0)?, 4, id)
                    };
                    if index >= 16_384 {
                        return Err(invalid());
                    }
                    column = Some(index);
                    let style = u32_at(payload, header)? & 0xff_ffff;
                    if kind != 1 || stubs {
                        // Grow large fixed-width projections once before the geometric
                        // allocator can leave multiple large buffers in Wasm memory.
                        let values = &mut projection.cells.values;
                        if row_limit.is_none() && values.len() == 64 * 1024 {
                            let target = bytes.len().min(2 * 1024 * 1024);
                            values
                                .try_reserve_exact(target.saturating_sub(values.len()))
                                .map_err(|_| invalid())?;
                        }
                        projection.cells.push(
                            kind,
                            row.unwrap(),
                            index,
                            style,
                            &payload[header + 4..],
                            formulas,
                        )?;
                    }
                }
                0x1aa | 0x1ab if formulas && row.is_some() => {
                    let [top, _, left, _] = Range::read(payload)?.0;
                    projection.cells.push_formula(
                        if id == 0x1aa { 1 } else { 2 },
                        top,
                        left,
                        payload,
                    )?;
                }
                0x92 => {
                    projection.tail = SheetTail::read(records)?;
                    return Ok(projection);
                }
                _ => {}
            }
        }
        Ok(projection)
    }
}

fn write_integer(output: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn write_record(output: &mut Vec<u8>, id: u32, payload: &[u8]) {
    write_integer(output, id);
    write_integer(output, payload.len() as u32);
    output.extend_from_slice(payload);
}

/// Writes the sheet dimensions, column layout and opening cell-table record.
pub fn write_sheet_header(dimensions: Range, columns: &[[u32; 3]]) -> Result<Vec<u8>> {
    let [top, bottom, left, right] = dimensions.0;
    if top > bottom || bottom >= 1_048_576 || left > right || right >= 16_384 {
        return Err(invalid());
    }
    let mut output = Vec::new();
    write_record(&mut output, 0x81, &[]);
    write_record(&mut output, 0x94, &dimensions.write());
    if !columns.is_empty() {
        write_record(&mut output, 0x186, &[]);
        for &[index, width, flags] in columns {
            if index >= 16_384 || width > 255 * 256 {
                return Err(invalid());
            }
            let flags = u16::try_from(flags).map_err(|_| invalid())?;
            let mut payload = [0; 18];
            payload[..4].copy_from_slice(&index.to_le_bytes());
            payload[4..8].copy_from_slice(&index.to_le_bytes());
            payload[8..12].copy_from_slice(&width.to_le_bytes());
            payload[16..].copy_from_slice(&flags.to_le_bytes());
            write_record(&mut output, 0x3c, &payload);
        }
        write_record(&mut output, 0x187, &[]);
    }
    write_record(&mut output, 0x91, &[]);
    Ok(output)
}

fn write_row_header(output: &mut Vec<u8>, rows: &mut &[u32], previous: Option<u32>) -> Result<u32> {
    let ([row, height, flags, spans], rest) = rows.split_first_chunk::<4>().ok_or_else(invalid)?;
    if *row >= 1_048_576 || previous.is_some_and(|previous| previous >= *row) || *spans > 16 {
        return Err(invalid());
    }
    let height = u16::try_from(*height).map_err(|_| invalid())?;
    let flags = u8::try_from(*flags).map_err(|_| invalid())?;
    let (columns, rest) = rest
        .split_at_checked(*spans as usize * 2)
        .ok_or_else(invalid)?;
    *rows = rest;
    let mut payload = [0; 17 + 16 * 8];
    payload[..4].copy_from_slice(&row.to_le_bytes());
    payload[8..10].copy_from_slice(&height.to_le_bytes());
    payload[11] = flags;
    payload[13..17].copy_from_slice(&spans.to_le_bytes());
    let mut last_group = None;
    for (index, &[first, last]) in columns.as_chunks::<2>().0.iter().enumerate() {
        let group = first / 1024;
        if first > last
            || last >= 16384
            || group != last / 1024
            || last_group.is_some_and(|previous| previous >= group)
        {
            return Err(invalid());
        }
        last_group = Some(group);
        let offset = 17 + index * 8;
        payload[offset..offset + 4].copy_from_slice(&first.to_le_bytes());
        payload[offset + 4..offset + 8].copy_from_slice(&last.to_le_bytes());
    }
    write_record(output, 0, &payload[..17 + columns.len() * 4]);
    Ok(*row)
}

/// Writes rows and their cells in worksheet record order.
pub fn write_cells(
    mut rows: &[u32],
    counts: &[u32],
    mut cells: &[[u32; 9]],
    numbers: &[f64],
    mut text: &[u16],
    mut tokens: &[u8],
) -> Result<Vec<u8>> {
    let mut previous = None;
    let mut output = Vec::new();
    let mut payload = Vec::new();
    for &count in counts {
        let row = write_row_header(&mut output, &mut rows, previous)?;
        previous = Some(row);
        let (entries, rest) = cells.split_at_checked(count as usize).ok_or_else(invalid)?;
        cells = rest;
        for &[
            column,
            style,
            kind,
            value,
            length,
            anchor,
            array_length,
            bottom,
            right,
        ] in entries
        {
            if column >= 16384 || style > 0xff_ffff {
                return Err(invalid());
            }
            payload.clear();
            payload.extend_from_slice(&column.to_le_bytes());
            payload.extend_from_slice(&style.to_le_bytes());
            match kind {
                1 => {}
                3 | 4 | 10 | 11 => payload.push(value as u8),
                5 | 9 => payload.extend_from_slice(
                    &numbers
                        .get(value as usize)
                        .ok_or_else(invalid)?
                        .to_le_bytes(),
                ),
                6 | 8 => {
                    let (units, rest) =
                        text.split_at_checked(value as usize).ok_or_else(invalid)?;
                    text = rest;
                    payload.extend_from_slice(&value.to_le_bytes());
                    payload.extend(units.iter().copied().flat_map(u16::to_le_bytes));
                }
                7 => payload.extend_from_slice(&value.to_le_bytes()),
                _ => return Err(invalid()),
            }
            if (8..=11).contains(&kind) {
                let (formula, rest) = tokens
                    .split_at_checked(length as usize)
                    .ok_or_else(invalid)?;
                tokens = rest;
                payload.extend_from_slice(&[0; 2]);
                payload.extend_from_slice(&length.to_le_bytes());
                payload.extend_from_slice(formula);
                let has_anchor = anchor != u32::MAX;
                payload.extend_from_slice(&(if has_anchor { 4_u32 } else { 0 }).to_le_bytes());
                if has_anchor {
                    payload.extend_from_slice(&anchor.to_le_bytes());
                }
            }
            write_record(&mut output, kind, &payload);
            if array_length != 0 {
                let (formula, rest) = tokens
                    .split_at_checked(array_length as usize)
                    .ok_or_else(invalid)?;
                tokens = rest;
                payload.clear();
                payload.extend_from_slice(&Range([row, bottom, column, right]).write());
                payload.push(0);
                payload.extend_from_slice(&array_length.to_le_bytes());
                payload.extend_from_slice(formula);
                payload.extend_from_slice(&0_u32.to_le_bytes());
                write_record(&mut output, 0x1aa, &payload);
            }
        }
    }
    if !rows.is_empty() || !cells.is_empty() || !text.is_empty() || !tokens.is_empty() {
        return Err(invalid());
    }
    Ok(output)
}

/// A link attached to an inclusive worksheet range.
#[derive(Debug, PartialEq)]
pub struct Hyperlink {
    pub range: Range,
    pub relationship: String,
    pub location: String,
    pub tooltip: String,
    pub display: String,
}

/// Number of cells assigned when the adapter projects a hyperlink range.
pub fn hyperlink_cell_count(Range([top, bottom, left, right]): Range) -> Result<u64> {
    if top > bottom || bottom >= 1_048_576 || left > right || right >= 16_384 {
        return Err(invalid());
    }
    Ok(u64::from(bottom - top + 1) * u64::from(right - left + 1))
}

/// Maximum number of hyperlink cell assignments per projected worksheet.
pub const MAX_HYPERLINK_CELLS: u64 = 1_048_576;

impl Hyperlink {
    fn read(payload: &[u8]) -> Result<Self> {
        let range = Range::read(payload)?;
        let (relationship, rest) = wide_units(&payload[16..])?;
        let (location, rest) = wide_units(rest)?;
        let (tooltip, rest) = wide_units(rest)?;
        let (display, _) = wide_units(rest)?;
        let decode = |bytes: &[u8]| {
            let units: Vec<_> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect();
            String::from_utf16_lossy(&units)
        };
        Ok(Self {
            range,
            relationship: decode(relationship),
            location: decode(location),
            tooltip: decode(tooltip),
            display: decode(display),
        })
    }
}

/// Legacy XLSB sheet protection, with one restriction bit per BIFF12 field.
#[derive(Debug, PartialEq)]
pub struct SheetProtection {
    verifier: u16,
    restricted: u16,
}

impl SheetProtection {
    /// Uses the SheetJS permission defaults for omitted options.
    pub fn new(password: &str, supplied: u16, allowed: u16) -> Self {
        const DEFAULT_RESTRICTIONS: u16 = (1 << 0) | (1 << 1) | (1 << 10) | (1 << 14);
        let mut verifier = 0;
        if !password.is_empty() {
            let rotate = |value: u16| ((value << 1) & 0x7fff) | (value >> 14);
            let bytes: Vec<_> = password.encode_utf16().map(|unit| unit as u8).collect();
            for &byte in bytes.iter().rev() {
                verifier = rotate(verifier) ^ u16::from(byte);
            }
            verifier = rotate(verifier) ^ u16::from(bytes.len() as u8) ^ 0xce4b;
        }
        Self {
            verifier,
            restricted: (DEFAULT_RESTRICTIONS & !supplied) | (supplied & !allowed),
        }
    }

    fn write(&self, output: &mut Vec<u8>) {
        let mut payload = [0; 66];
        payload[..2].copy_from_slice(&self.verifier.to_le_bytes());
        payload[2] = 1;
        for (index, field) in payload[6..].as_chunks_mut::<4>().0.iter_mut().enumerate() {
            field[0] = ((self.restricted >> index) & 1) as u8;
        }
        write_record(output, 0x217, &payload);
    }
}

/// Worksheet records following the cell table.
#[derive(Debug, Default, PartialEq)]
pub struct SheetTail {
    pub filter: Option<Range>,
    pub merges: Vec<Range>,
    /// Left, right, top, bottom, header, footer in inches.
    pub margins: Option<[f64; 6]>,
    pub hyperlinks: Vec<Hyperlink>,
    pub protection: Option<SheetProtection>,
}

impl SheetTail {
    /// Decode the records after the worksheet cell table.
    pub fn read(mut records: &[u8]) -> Result<Self> {
        let mut tail = Self::default();
        let mut linked_cells = 0_u64;
        while !records.is_empty() {
            let (id, payload) = read_record(&mut records)?;
            match id {
                BEGIN_FILTER => tail.filter = Some(Range::read(payload)?),
                MERGE => tail.merges.push(Range::read(payload)?),
                HYPERLINK => {
                    let link = Hyperlink::read(payload)?;
                    linked_cells += hyperlink_cell_count(link.range)?;
                    if linked_cells > MAX_HYPERLINK_CELLS {
                        return Err(invalid());
                    }
                    tail.hyperlinks.push(link);
                }

                MARGINS => {
                    let mut margins = [0.0; 6];
                    for (index, margin) in margins.iter_mut().enumerate() {
                        *margin = f64::from_le_bytes(read_at(payload, index * 8)?);
                    }
                    tail.margins = Some(margins);
                }
                _ => {}
            }
        }
        Ok(tail)
    }

    /// Closes the cell table, writes the worksheet's trailing metadata, and closes the sheet.
    pub fn encode_sheet_end(&self) -> Vec<u8> {
        let mut output = Vec::new();
        write_record(&mut output, 0x92, &[]);
        self.encode_records(&mut output);
        write_record(&mut output, 0x82, &[]);
        output
    }

    fn encode_records(&self, output: &mut Vec<u8>) {
        if let Some(protection) = &self.protection {
            protection.write(output);
        }
        if let Some(filter) = self.filter {
            write_record(output, BEGIN_FILTER, &filter.write());
            write_record(output, END_FILTER, &[]);
        }
        if !self.merges.is_empty() {
            write_record(
                output,
                BEGIN_MERGES,
                &(self.merges.len() as u32).to_le_bytes(),
            );
            for range in &self.merges {
                write_record(output, MERGE, &range.write());
            }
            write_record(output, END_MERGES, &[]);
        }
        for link in &self.hyperlinks {
            let mut bytes = link.range.write().to_vec();
            for (index, field) in [
                &link.relationship,
                &link.location,
                &link.tooltip,
                &link.display,
            ]
            .into_iter()
            .enumerate()
            {
                let length = if index == 0 && field.is_empty() {
                    u32::MAX
                } else {
                    field.encode_utf16().count() as u32
                };
                bytes.extend_from_slice(&length.to_le_bytes());
                bytes.extend(field.encode_utf16().flat_map(u16::to_le_bytes));
            }
            write_record(output, HYPERLINK, &bytes);
        }
        if let Some(margins) = self.margins {
            let mut bytes = [0; 48];
            for (chunk, margin) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(margins) {
                chunk.copy_from_slice(&margin.to_le_bytes());
            }
            write_record(output, MARGINS, &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_does_not_decode_formula_metadata() {
        let mut bytes = write_workbook(true, &[2], &[1], &[65], &[0]).unwrap();
        assert_eq!(WorkbookInfo::read(&bytes).unwrap().references, [0, 0]);
        write_record(&mut bytes, 0x27, &[]);
        let catalogue = WorkbookInfo::read_catalogue(&bytes).unwrap();
        assert_eq!(catalogue.sheets, [2, 4, 1]);
        assert!(catalogue.date1904);
        assert!(catalogue.references.is_empty());
        assert!(catalogue.names.is_empty());
        assert!(WorkbookInfo::read(&bytes).is_err());
    }

    #[test]
    fn closes_worksheet_in_record_order() {
        let tail = SheetTail {
            filter: Some(Range([0, 1, 0, 1])),
            ..Default::default()
        };
        let bytes = tail.encode_sheet_end();
        let mut records = bytes.as_slice();
        assert_eq!(read_record(&mut records).unwrap().0, 0x92);
        assert_eq!(read_record(&mut records).unwrap().0, BEGIN_FILTER);
        assert_eq!(read_record(&mut records).unwrap().0, END_FILTER);
        assert_eq!(read_record(&mut records).unwrap().0, 0x82);
        assert!(records.is_empty());
    }

    #[test]
    fn sheet_tail_round_trip_with_unknown_record() {
        let tail = SheetTail {
            filter: Some(Range([0, 10, 1, 4])),
            merges: vec![Range([2, 2, 1, 4])],
            margins: Some([0.5, 0.7, 0.0, 0.75, 0.3, 0.25]),
            hyperlinks: vec![Hyperlink {
                range: Range([1, 2, 0, 1]),
                relationship: String::new(),
                location: "Data!A1".into(),
                tooltip: "日本語".into(),
                display: "go".into(),
            }],
            ..Default::default()
        };
        let mut records = Vec::new();
        write_record(&mut records, 0x01ff, &[1, 2, 3]);
        tail.encode_records(&mut records);
        assert_eq!(SheetTail::read(&records).unwrap(), tail);
    }

    #[test]
    fn projection_stops_at_excluded_row_or_projects_tail() {
        let mut records = Vec::new();
        let mut row = [0; 17];
        row[8..10].copy_from_slice(&620u16.to_le_bytes());
        row[11] = 0x32;
        write_record(&mut records, 0, &row);
        row[0] = 1;
        write_record(&mut records, 0, &row);
        let cell = [0; 16];
        write_record(&mut records, 5, &cell);
        let tail = SheetTail {
            merges: vec![Range([0, 1, 0, 1])],
            ..Default::default()
        };
        records.extend(tail.encode_sheet_end());
        let limited = SheetProjection::read(&records, Some(1), false, false, true).unwrap();
        assert_eq!(limited.rows, [[0, 620, 0x32], [1, 620, 0x32]]);
        assert!(limited.cells.values.is_empty());
        assert!(limited.tail.merges.is_empty());
        let complete = SheetProjection::read(&records, None, true, false, true).unwrap();
        assert_eq!(complete.rows.len(), 2);
        assert_eq!(complete.tail, tail);
    }

    #[test]
    fn projection_packs_full_and_short_cells_with_their_style_and_values() {
        let mut records = Vec::new();
        let mut row = [0; 17];
        row[..4].copy_from_slice(&70_000u32.to_le_bytes());
        write_record(&mut records, 0, &row);
        let mut full = Vec::new();
        full.extend_from_slice(&255u32.to_le_bytes());
        full.extend_from_slice(&0x12_3456u32.to_le_bytes());
        full.extend_from_slice(&42.5f64.to_le_bytes());
        write_record(&mut records, 5, &full);
        let mut short = Vec::new();
        short.extend_from_slice(&0u32.to_le_bytes());
        short.extend_from_slice(&(-498i32).to_le_bytes());
        write_record(&mut records, 0x0d, &short);
        let result = SheetProjection::read(&records, None, false, false, false).unwrap();
        let cells = result.cells.values;
        assert_eq!(cells.len(), 32);
        assert_eq!(u32_at(&cells, 0).unwrap(), 70_000 | (0x456 << 20));
        assert_eq!(u32_at(&cells, 4).unwrap(), 255 | (0x123 << 14) | (1 << 26));
        assert_eq!(f64::from_le_bytes(read_at(&cells, 8).unwrap()), 42.5);
        assert_eq!(u32_at(&cells, 20).unwrap(), 256 | (1 << 26));
        assert_eq!(f64::from_le_bytes(read_at(&cells, 24).unwrap()), -125.0);
    }

    #[test]
    fn shared_strings_decode_text_runs_and_reject_truncation() {
        let mut records = Vec::new();
        let plain = [0, 1, 0, 0, 0, b'A', 0];
        let rich = [
            1, 2, 0, 0, 0, 0xe5, 0x65, 0x2c, 0x67, 2, 0, 0, 0, 0, 0, 5, 0, 1, 0, 7, 0,
        ];
        write_record(&mut records, 0x13, &plain);
        write_record(&mut records, 0x13, &rich);
        let strings = SharedStrings::read(&records).unwrap();
        assert_eq!(strings.lengths, [1, 1, 2, 2]);
        assert_eq!(strings.units, [65, 0, 0, 0x65e5, 0x672c, 0, 5, 1, 7]);
        records.pop();
        assert!(SharedStrings::read(&records).is_err());
    }

    #[test]
    fn styles_only_index_cell_xfs_and_decode_custom_formats() {
        let mut records = Vec::new();
        write_record(&mut records, 0x2c, &[164, 0, 2, 0, 0, 0, b'0', 0, b'%', 0]);
        write_record(&mut records, 0x2f, &[0, 0, 1, 0]);
        write_record(&mut records, 0x269, &[]);
        write_record(&mut records, 0x2f, &[0, 0, 164, 0]);
        write_record(&mut records, 0x26a, &[]);
        write_record(&mut records, 0x2f, &[0, 0, 2, 0]);
        let styles = Styles::read(&records).unwrap();
        assert_eq!(styles.formats, [164, 2]);
        assert_eq!(styles.text, [48, 37]);
        assert_eq!(styles.cell_formats, [164]);
        records.clear();
        write_record(&mut records, 0x2c, &[164, 0, 2, 0, 0, 0, b'0', 0]);
        assert!(Styles::read(&records).is_err());
    }

    #[test]
    fn workbook_decodes_catalogue_and_distinguishes_external_references() {
        let mut records = Vec::new();
        write_record(&mut records, 0x99, &1u32.to_le_bytes());
        write_record(
            &mut records,
            0x9c,
            &[
                2, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, b'R', 0, 1, 0, 0, 0, b'S', 0,
            ],
        );
        write_record(&mut records, 0x165, &[]);
        write_record(&mut records, 0x163, &[]);
        let xti: Vec<_> = [2_u32, 0, 0, 0, 1, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        write_record(&mut records, 0x16a, &xti);
        let info = WorkbookInfo::read(&records).unwrap();
        assert!(info.date1904);
        assert_eq!(info.sheets, [2, 1, 1]);
        assert_eq!(info.text, [82, 83]);
        assert_eq!(info.references, [0, 0, -1, -1]);
    }

    #[test]
    fn writes_plain_shared_strings_in_index_order() {
        let bytes = write_shared_strings(3, &[2, 0], &[0xd83d, 0xde03]).unwrap();
        let mut records = bytes.as_slice();
        assert_eq!(
            read_record(&mut records).unwrap(),
            (0x9f, &[3, 0, 0, 0, 2, 0, 0, 0][..])
        );
        let strings = SharedStrings::read(&bytes).unwrap();
        assert_eq!(strings.lengths, [2, 1, 0, 1]);
        assert_eq!(strings.units, [0xd83d, 0xde03, 0, 0, 0, 0]);
        assert!(write_shared_strings(1, &[1, 0], &[1]).is_err());
        assert!(write_shared_strings(1, &[2], &[1]).is_err());
        assert!(write_shared_strings(1, &[0], &[1]).is_err());
    }

    #[test]
    fn writes_styles_in_cell_xf_order() {
        let bytes = write_styles(&[0, -1, 14, -1], &[1, 2], &[65, 66, 67]).unwrap();
        let styles = Styles::read(&bytes).unwrap();
        assert_eq!(styles.formats, [164, 1, 165, 2]);
        assert_eq!(styles.text, [65, 66, 67]);
        assert_eq!(styles.cell_formats, [0, 164, 14, 165]);
        assert!(write_styles(&[0, -1], &[], &[]).is_err());
        assert!(write_styles(&[-1], &[2], &[65]).is_err());
    }

    #[test]
    fn writes_workbook_catalogue_and_reference_order() {
        let bytes = write_workbook(true, &[0, 2], &[1, 1], &[65, 66], &[1, 0]).unwrap();
        let book = WorkbookInfo::read(&bytes).unwrap();
        assert!(book.date1904);
        assert_eq!(book.sheets, [0, 4, 1, 2, 4, 1]);
        assert_eq!(book.text, "rId1ArId2B".encode_utf16().collect::<Vec<_>>());
        assert_eq!(book.references, [1, 1, 0, 0]);
        assert!(write_workbook(false, &[0], &[1], &[65], &[1]).is_err());
        assert!(write_workbook(false, &[0], &[2], &[65], &[]).is_err());
    }

    #[test]
    fn writes_sheet_dimensions_and_columns() {
        let extent = Range([0, 4, 1, 7]);
        let bytes = write_sheet_header(extent, &[[3, 22 * 256, 0x303]]).unwrap();
        let projection = SheetProjection::read(&bytes, None, true, false, false).unwrap();
        assert_eq!(projection.dimensions, Some(extent));
        assert_eq!(projection.columns, [[3, 3, 22 * 256, 0x303]]);
        assert!(write_sheet_header(extent, &[[16_384, 0, 0]]).is_err());
        assert!(write_sheet_header(Range([1, 0, 0, 0]), &[]).is_err());
    }

    #[test]
    fn writes_row_headers_and_column_spans_in_order() {
        let bytes = write_cells(
            &[2, 620, 0x32, 2, 1, 4, 1025, 1028, 9, 320, 0x10, 0],
            &[0, 0],
            &[],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let mut record = bytes.as_slice();
        let (id, payload) = read_record(&mut record).unwrap();
        assert_eq!(id, 0);
        assert_eq!(u32_at(payload, 0).unwrap(), 2);
        assert_eq!(u32_at(payload, 13).unwrap(), 2);
        assert_eq!(u32_at(payload, 25).unwrap(), 1025);
        assert_eq!(u32_at(payload, 29).unwrap(), 1028);
        let (_, second) = read_record(&mut record).unwrap();
        assert_eq!(second.len(), 17);
        assert_eq!(u32_at(second, 0).unwrap(), 9);
        assert!(write_cells(&[2, 620, 0, 1, 1000, 1100], &[0], &[], &[], &[], &[]).is_err());
        assert!(write_cells(&[2, 620, 0, 2, 1, 4], &[0], &[], &[], &[], &[]).is_err());
    }

    #[test]
    fn writes_cell_batch_with_formula_and_utf16_text() {
        let rows = [1, 320, 0, 1, 0, 1];
        let cells = [
            [0, 0, 6, 1, 0, u32::MAX, 0, 0, 0],
            [1, 0, 9, 0, 3, u32::MAX, 0, 0, 0],
        ];
        let tokens = [0x1e, 8, 0];
        let bytes = write_cells(&rows, &[2], &cells, &[8.0], &[0x65e5], &tokens).unwrap();
        let result = SheetProjection::read(&bytes, None, false, false, true).unwrap();
        assert_eq!(result.cells.values.len(), 32);
        assert_eq!(result.cells.text, [0xe5, 0x65]);
        assert_eq!(
            f64::from_le_bytes(read_at(&result.cells.values, 24).unwrap()),
            8.0
        );
        assert_eq!(&result.cells.formulas[20..23], &tokens);
        assert!(write_cells(&rows, &[3], &cells, &[8.0], &[0x65e5], &tokens).is_err());
        assert!(write_cells(&rows, &[2], &cells, &[8.0], &[], &tokens).is_err());
    }

    #[test]
    fn reads_calculation_chain_with_flags_and_validates_coordinates() {
        let mut bytes = Vec::new();
        write_record(&mut bytes, 0x210, &[]);
        let mut cell = [0; 13];
        cell[0] = 2;
        cell[4] = 6;
        cell[8] = 3;
        cell[12] = 10;
        write_record(&mut bytes, 0x3f, &cell);
        write_record(&mut bytes, 0x211, &[]);
        assert_eq!(read_calculation_chain(&bytes).unwrap(), [[2, 6, 3, 10]]);
        cell[9] = 64;
        write_record(&mut bytes, 0x3f, &cell);
        assert!(read_calculation_chain(&bytes).is_err());
    }

    #[test]
    fn rejects_invalid_headers_and_truncated_payloads() {
        for bytes in [&[0x80, 0x80][..], &[0xa1, 1, 16], &[0xa1, 1, 1, 0]] {
            assert!(SheetTail::read(bytes).is_err());
        }
    }
}
