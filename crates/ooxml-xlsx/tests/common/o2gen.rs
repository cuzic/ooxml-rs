//! O2 (ADR-0003 §7.1): random workbooks in an evaluable subset, rendered to
//! formula text from an AST, and a tiny test-only evaluator over the AST.
//! It never uses ooxml-xlsx's tokenizer or dependency code. Shared by
//! `tests/phase1_o2.rs` and `examples/o3_inputs.rs`.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Cursor, Write};

/// xorshift64*: deterministic, no dependency.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    pub fn coin(&mut self) -> bool {
        self.below(2) == 0
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u64) as usize]
    }
}

/// Value cells live in columns A–C (0..3), rows 1–6; formula cells in D–F.
pub const VCOLS: u32 = 3;
pub const ROWS: u32 = 6;

#[derive(Clone, Debug)]
pub enum Ast {
    Num(f64),
    /// sheet index (None: own sheet), row, col
    Ref(Option<usize>, u32, u32),
    /// SUM/MIN/MAX over a rectangle on a sheet (or own sheet)
    Agg(&'static str, Option<usize>, (u32, u32, u32, u32)),
    /// SUM over a 3-D span of one cell
    Sum3d(usize, usize, u32, u32),
    /// SUM over a whole value column
    SumCol(Option<usize>, u32),
    Bin(char, Box<Ast>, Box<Ast>),
    If(Box<Ast>, Box<Ast>, Box<Ast>, Box<Ast>),
    /// INDEX(single-column range, k)
    Index(Option<usize>, u32, u32, u32, u32),
    /// SUMIF(range col c rows r1..=r2, ">t", sum cell (sr, sc)) — resized
    SumIf(Option<usize>, u32, u32, u32, f64, u32, u32),
    /// A defined name: (index into names, qualified by sheet?)
    Name(usize, bool),
}

#[derive(Clone, Debug)]
pub struct NameDef {
    pub name: String,
    pub local: Option<usize>,
    pub sheet: usize,
    pub row: u32,
    pub col: u32,
}

#[derive(Clone, Debug)]
pub struct Book {
    pub sheets: Vec<String>,
    /// (sheet, row, col) -> value
    pub values: BTreeMap<(usize, u32, u32), f64>,
    /// formula cells in evaluation order: (sheet, row, col, ast)
    pub formulas: Vec<(usize, u32, u32, Ast)>,
    pub names: Vec<NameDef>,
    /// rendering seed (case, `$`, spaces, quotes)
    pub style: u64,
}

const SHEETS: &[&str] = &["Sheet1", "Data", "Q1 2024", "It's"];

pub fn generate(seed: u64) -> Book {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let n_sheets = 2 + r.below(2) as usize;
    let mut sheets: Vec<String> = SHEETS.iter().map(|s| s.to_string()).collect();
    for i in (1..sheets.len()).rev() {
        let j = r.below(i as u64 + 1) as usize;
        sheets.swap(i, j);
    }
    sheets.truncate(n_sheets);
    let mut values = BTreeMap::new();
    for s in 0..n_sheets {
        for row in 0..ROWS {
            for col in 0..VCOLS {
                if r.below(4) != 0 {
                    values.insert((s, row, col), (r.below(19) as f64) - 9.0);
                }
            }
        }
    }
    let mut names = Vec::new();
    for k in 0..3 {
        let local = if k == 0 { None } else { Some(r.below(n_sheets as u64) as usize) };
        // Same text for a global and a local name sometimes (shadowing).
        let name = if k == 2 && r.coin() { names.first().map_or("Rate".to_string(), |n: &NameDef| n.name.clone()) } else { format!("Nm{k}") };
        if names.iter().any(|n: &NameDef| n.name == name && n.local == local) {
            continue;
        }
        names.push(NameDef { name, local, sheet: r.below(n_sheets as u64) as usize, row: r.below(ROWS as u64) as u32, col: r.below(VCOLS as u64) as u32 });
    }
    let mut formulas: Vec<(usize, u32, u32, Ast)> = Vec::new();
    let n_f = 3 + r.below(8) as usize;
    let mut used = std::collections::BTreeSet::new();
    for _ in 0..n_f {
        let s = r.below(n_sheets as u64) as usize;
        let (row, col) = (r.below(ROWS as u64) as u32, VCOLS + r.below(3) as u32);
        if !used.insert((s, row, col)) {
            continue;
        }
        let ast = gen_ast(&mut r, n_sheets, s, &formulas, &names, 3);
        formulas.push((s, row, col, ast));
    }
    Book { sheets, values, formulas, names, style: r.next() }
}

fn sheet_opt(r: &mut Rng, n: usize) -> Option<usize> {
    if r.coin() { None } else { Some(r.below(n as u64) as usize) }
}

fn gen_ast(r: &mut Rng, n: usize, own: usize, earlier: &[(usize, u32, u32, Ast)], names: &[NameDef], depth: u32) -> Ast {
    let leaf = depth == 0 || r.below(3) == 0;
    if leaf {
        return match r.below(4) {
            0 => Ast::Num((r.below(9) as f64) + 1.0),
            1 if !earlier.is_empty() => {
                let e = r.pick(earlier);
                let (s, row, col) = (e.0, e.1, e.2);
                Ast::Ref(if s == own && r.coin() { None } else { Some(s) }, row, col)
            }
            2 if !names.is_empty() => {
                let i = r.below(names.len() as u64) as usize;
                Ast::Name(i, names[i].local.is_some() && r.coin())
            }
            _ => Ast::Ref(sheet_opt(r, n), r.below(ROWS as u64) as u32, r.below(VCOLS as u64) as u32),
        };
    }
    let sub = |r: &mut Rng| Box::new(gen_ast(r, n, own, earlier, names, depth - 1));
    match r.below(8) {
        0 => {
            let (r1, r2) = ordered(r.below(ROWS as u64) as u32, r.below(ROWS as u64) as u32);
            let (c1, c2) = ordered(r.below(VCOLS as u64) as u32, r.below(VCOLS as u64) as u32);
            Ast::Agg(r.pick(&["SUM", "MIN", "MAX"]), sheet_opt(r, n), (r1, c1, r2, c2))
        }
        1 => Ast::Sum3d(r.below(n as u64) as usize, r.below(n as u64) as usize, r.below(ROWS as u64) as u32, r.below(VCOLS as u64) as u32),
        2 => Ast::SumCol(sheet_opt(r, n), r.below(VCOLS as u64) as u32),
        3 => {
            let (a, b, x, y) = (sub(r), sub(r), sub(r), sub(r));
            Ast::If(a, b, x, y)
        }
        4 => {
            let (r1, r2) = ordered(r.below(ROWS as u64) as u32, r.below(ROWS as u64) as u32);
            let k = 1 + r.below((r2 - r1 + 1) as u64) as u32;
            Ast::Index(sheet_opt(r, n), r.below(VCOLS as u64) as u32, r1, r2, k)
        }
        5 => {
            let (r1, r2) = ordered(r.below(ROWS as u64) as u32, r.below(ROWS as u64) as u32);
            Ast::SumIf(sheet_opt(r, n), r.below(VCOLS as u64) as u32, r1, r2, (r.below(7) as f64) - 3.0, r.below(ROWS as u64) as u32, r.below(VCOLS as u64) as u32)
        }
        _ => Ast::Bin(*r.pick(&['+', '-', '*', '/']), sub(r), sub(r)),
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

fn ordered(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

pub fn col_letter(c: u32) -> char {
    (b'A' + c as u8) as char
}

impl Book {
    fn style_bit(&self, salt: u64) -> bool {
        let mut x = self.style ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x & 1 == 1
    }

    /// A sheet prefix with random case and optional quotes.
    fn sheet_ref(&self, s: usize, salt: u64) -> String {
        let name = &self.sheets[s];
        let name = if self.style_bit(salt) { name.to_lowercase() } else { name.clone() };
        let simple = name.chars().all(|c| c.is_ascii_alphanumeric()) && !name.chars().next().unwrap().is_ascii_digit();
        if simple && !self.style_bit(salt + 1) {
            format!("{name}!")
        } else {
            format!("'{}'!", name.replace('\'', "''"))
        }
    }

    fn cell(&self, row: u32, col: u32, salt: u64) -> String {
        let d1 = if self.style_bit(salt + 7) { "$" } else { "" };
        let d2 = if self.style_bit(salt + 9) { "$" } else { "" };
        format!("{d1}{}{d2}{}", col_letter(col), row + 1)
    }

    fn prefix(&self, s: Option<usize>, salt: u64) -> String {
        s.map(|s| self.sheet_ref(s, salt)).unwrap_or_default()
    }

    pub fn render(&self, a: &Ast, salt: u64) -> String {
        let sp = if self.style_bit(salt + 3) { " " } else { "" };
        match a {
            Ast::Num(n) => format!("{n}"),
            Ast::Ref(s, row, col) => format!("{}{}", self.prefix(*s, salt), self.cell(*row, *col, salt)),
            Ast::Agg(f, s, (r1, c1, r2, c2)) => format!(
                "{f}({}{}:{})",
                self.prefix(*s, salt),
                self.cell(*r1, *c1, salt),
                self.cell(*r2, *c2, salt + 11)
            ),
            Ast::Sum3d(a, b, row, col) => {
                let (x, y) = (&self.sheets[*a], &self.sheets[*b]);
                let p = if (x.chars().chain(y.chars())).all(|c| c.is_ascii_alphanumeric()) {
                    format!("{x}:{y}!")
                } else {
                    format!("'{}:{}'!", x.replace('\'', "''"), y.replace('\'', "''"))
                };
                format!("SUM({p}{})", self.cell(*row, *col, salt))
            }
            Ast::SumCol(s, c) => format!("SUM({}{}:{})", self.prefix(*s, salt), col_letter(*c), col_letter(*c)),
            Ast::Bin(op, x, y) => format!("({}{sp}{op}{sp}{})", self.render(x, salt * 3 + 1), self.render(y, salt * 3 + 2)),
            Ast::If(a, b, x, y) => format!(
                "IF({}>{},{},{})",
                self.render(a, salt * 5 + 1),
                self.render(b, salt * 5 + 2),
                self.render(x, salt * 5 + 3),
                self.render(y, salt * 5 + 4)
            ),
            Ast::Index(s, c, r1, r2, k) => format!(
                "INDEX({}{}:{},{k})",
                self.prefix(*s, salt),
                self.cell(*r1, *c, salt),
                self.cell(*r2, *c, salt + 13)
            ),
            Ast::SumIf(s, c, r1, r2, t, sr, sc) => {
                let p = self.prefix(*s, salt);
                format!("SUMIF({p}{}:{},\">{t}\",{p}{})", self.cell(*r1, *c, salt), self.cell(*r2, *c, salt + 5), self.cell(*sr, *sc, salt + 17))
            }
            Ast::Name(i, qualified) => {
                let n = &self.names[*i];
                if *qualified {
                    format!("{}{}", self.sheet_ref(n.local.unwrap(), salt), n.name)
                } else {
                    n.name.clone()
                }
            }
        }
    }

    /// The name a use resolves to (Excel: local name of the using sheet,
    /// else global; a qualified use: that sheet's local name).
    fn resolve(&self, i: usize, qualified: bool, own: usize) -> Option<&NameDef> {
        let text = &self.names[i].name;
        if qualified {
            return Some(&self.names[i]);
        }
        self.names
            .iter()
            .find(|n| &n.name == text && n.local == Some(own))
            .or_else(|| self.names.iter().find(|n| &n.name == text && n.local.is_none()))
    }

    /// Evaluates every formula; `None` = an error value.
    pub fn eval(&self, values: &BTreeMap<(usize, u32, u32), f64>) -> Vec<Option<f64>> {
        let mut cells: BTreeMap<(usize, u32, u32), Option<f64>> = values.iter().map(|(k, v)| (*k, Some(*v))).collect();
        let mut out = Vec::new();
        for (s, row, col, a) in &self.formulas {
            let v = self.ev(a, *s, &cells);
            cells.insert((*s, *row, *col), v);
            out.push(v);
        }
        out
    }

    fn get(cells: &BTreeMap<(usize, u32, u32), Option<f64>>, k: (usize, u32, u32)) -> Option<f64> {
        cells.get(&k).copied().unwrap_or(Some(0.0))
    }

    fn ev(&self, a: &Ast, own: usize, cells: &BTreeMap<(usize, u32, u32), Option<f64>>) -> Option<f64> {
        let sh = |s: &Option<usize>| s.unwrap_or(own);
        let finite = |x: f64| x.is_finite().then_some(x);
        match a {
            Ast::Num(n) => Some(*n),
            Ast::Ref(s, row, col) => Self::get(cells, (sh(s), *row, *col)),
            Ast::Agg(f, s, (r1, c1, r2, c2)) => {
                let mut xs = Vec::new();
                for row in *r1..=*r2 {
                    for col in *c1..=*c2 {
                        if let Some(v) = cells.get(&(sh(s), row, col)) {
                            xs.push((*v)?);
                        }
                    }
                }
                Some(match *f {
                    "SUM" => xs.iter().sum(),
                    "MIN" => xs.iter().copied().fold(f64::INFINITY, f64::min).pipe(|m| if xs.is_empty() { 0.0 } else { m }),
                    _ => xs.iter().copied().fold(f64::NEG_INFINITY, f64::max).pipe(|m| if xs.is_empty() { 0.0 } else { m }),
                })
            }
            Ast::Sum3d(a, b, row, col) => {
                let mut t = 0.0;
                for s in (*a).min(*b)..=(*a).max(*b) {
                    t += Self::get(cells, (s, *row, *col))?;
                }
                Some(t)
            }
            Ast::SumCol(s, c) => {
                let mut t = 0.0;
                for ((s2, _, c2), v) in cells.iter() {
                    if *s2 == sh(s) && c2 == c {
                        t += (*v)?;
                    }
                }
                Some(t)
            }
            Ast::Bin(op, x, y) => {
                let (x, y) = (self.ev(x, own, cells)?, self.ev(y, own, cells)?);
                match op {
                    '+' => finite(x + y),
                    '-' => finite(x - y),
                    '*' => finite(x * y),
                    _ => (y != 0.0).then(|| x / y).and_then(finite),
                }
            }
            Ast::If(a, b, x, y) => {
                if self.ev(a, own, cells)? > self.ev(b, own, cells)? { self.ev(x, own, cells) } else { self.ev(y, own, cells) }
            }
            Ast::Index(s, c, r1, _, k) => Self::get(cells, (sh(s), r1 + k - 1, *c)),
            Ast::SumIf(s, c, r1, r2, t, sr, sc) => {
                let mut sum = 0.0;
                for i in 0..=(r2 - r1) {
                    let crit = Self::get(cells, (sh(s), r1 + i, *c))?;
                    if crit > *t {
                        sum += Self::get(cells, (sh(s), sr + i, *sc))?;
                    }
                }
                Some(sum)
            }
            Ast::Name(i, q) => {
                let n = self.resolve(*i, *q, own)?;
                Self::get(cells, (n.sheet, n.row, n.col))
            }
        }
    }

    /// The workbook as `.xlsx` bytes, cached values from [`Self::eval`].
    pub fn to_xlsx(&self) -> Vec<u8> {
        let results = self.eval(&self.values);
        let mut per_sheet: Vec<BTreeMap<(u32, u32), String>> = vec![BTreeMap::new(); self.sheets.len()];
        for ((s, row, col), v) in &self.values {
            per_sheet[*s].insert((*row, *col), format!(r#"<c r="{}{}"><v>{v}</v></c>"#, col_letter(*col), row + 1));
        }
        for (i, (s, row, col, a)) in self.formulas.iter().enumerate() {
            let f = xml_escape(&self.render(a, i as u64 * 1000 + 7));
            let v = match results[i] {
                Some(x) => format!("<v>{x}</v>"),
                None => "<v>#DIV/0!</v>".to_string(),
            };
            let t = if results[i].is_none() { r#" t="e""# } else { "" };
            per_sheet[*s].insert((*row, *col), format!(r#"<c r="{}{}"{t}><f>{f}</f>{v}</c>"#, col_letter(*col), row + 1));
        }
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        let sml = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#);
        let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
        let mut sheets = String::new();
        let mut parts = Vec::new();
        for (i, cells) in per_sheet.iter().enumerate() {
            let mut rows: BTreeMap<u32, String> = BTreeMap::new();
            for ((row, _), c) in cells {
                rows.entry(*row).or_default().push_str(c);
            }
            let mut sd = String::new();
            for (row, cs) in rows {
                let _ = write!(sd, r#"<row r="{}">{cs}</row>"#, row + 1);
            }
            parts.push((format!("xl/worksheets/sheet{}.xml", i + 1), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{sml}"><sheetData>{sd}</sheetData></worksheet>"#)));
            let _ = write!(ct, r#"<Override PartName="/xl/worksheets/sheet{}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#, i + 1);
            let _ = write!(rels, r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{}.xml"/>"#, i + 1, i + 1);
            let _ = write!(sheets, r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#, xml_escape(&self.sheets[i]), i + 1, i + 1);
        }
        ct.push_str("</Types>");
        rels.push_str("</Relationships>");
        let mut dn = String::new();
        for n in &self.names {
            let local = n.local.map(|l| format!(r#" localSheetId="{l}""#)).unwrap_or_default();
            let target = format!("'{}'!${}${}", self.sheets[n.sheet].replace('\'', "''"), col_letter(n.col), n.row + 1);
            let _ = write!(dn, r#"<definedName name="{}"{local}>{}</definedName>"#, n.name, xml_escape(&target));
        }
        let dn = if dn.is_empty() { dn } else { format!("<definedNames>{dn}</definedNames>") };
        let wb = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{sml}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheets}</sheets>{dn}<calcPr calcId="191029"/></workbook>"#);
        let top = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
        let mut all = vec![
            ("[Content_Types].xml".to_string(), ct),
            ("_rels/.rels".to_string(), top.to_string()),
            ("xl/workbook.xml".to_string(), wb),
            ("xl/_rels/workbook.xml.rels".to_string(), rels),
        ];
        all.extend(parts);
        for (n, b) in all {
            zw.start_file(n, o).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
