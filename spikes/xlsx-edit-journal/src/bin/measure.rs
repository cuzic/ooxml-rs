//! ADR-0024 Test oracle 10 (J0's share): append, truncation replay, Commit
//! and memory on a synthetic sheet. `cargo run --release --bin measure -- [rows]`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use ooxml_xlsx::{Color, Fill, Font, Workbook};
use xlsx_edit_journal_spike::commit;
use xlsx_edit_journal_spike::fixture::large_workbook;
use xlsx_edit_journal_spike::op::apply_seq;
use xlsx_edit_journal_spike::{Api, CellKey, Content, Journal, Op, Patch};

struct Counting;
static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let c = CUR.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(c, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        CUR.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() {
            if n > l.size() {
                let c = CUR.fetch_add(n - l.size(), Relaxed) + n - l.size();
                PEAK.fetch_max(c, Relaxed);
            } else {
                CUR.fetch_sub(l.size() - n, Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

fn mib(b: usize) -> String {
    format!("{:.1} MiB", b as f64 / (1 << 20) as f64)
}
fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1e3)
}
fn us_per(d: Duration, n: usize) -> String {
    format!("{:.1} µs/op", d.as_secs_f64() * 1e6 / n as f64)
}
fn reset_peak() -> usize {
    let c = CUR.load(Relaxed);
    PEAK.store(c, Relaxed);
    c
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u32 {
        (self.next() % n) as u32
    }
}

fn ops(rows: u32, n: usize, seed: u64) -> Vec<Op> {
    let mut r = Rng(seed);
    let font = Font { bold: true, ..Font::default() };
    let fill = Fill::Pattern { pattern: "solid".into(), fg: Some(Color::rgb(0xFFCC00)), bg: None };
    (0..n)
        .map(|i| {
            let k = CellKey::new(0, r.below(rows as u64), r.below(10));
            match i % 4 {
                0 | 1 => Op::Content(k, Content::Number(r.below(1000) as f64)),
                2 => Op::Content(k, Content::String(format!("s{}", r.below(50)))),
                _ if i % 8 == 3 => Op::Style(k, Patch::Font(font.clone())),
                _ => Op::Style(k, Patch::Fill(fill.clone())),
            }
        })
        .collect()
}

fn main() {
    let rows: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(10_000);
    let n_ops: usize = std::env::args().nth(2).and_then(|a| a.parse().ok()).unwrap_or(2_000);
    let base = large_workbook(rows);
    println!("fixture: {rows} rows x 11 cols = {} cells ({} formulas), {} bytes zipped", rows * 11, rows, base.len());

    // shadow (= one parsed workbook)
    let before = reset_peak();
    let t = Instant::now();
    let mut j = Journal::open(base.clone()).unwrap();
    let open = t.elapsed();
    let shadow_mem = CUR.load(Relaxed) - before;
    println!("Journal::open (from_bytes + style registry): {}, heap held {} (peak {})", ms(open), mib(shadow_mem), mib(PEAK.load(Relaxed) - before));
    // touch the sheet once so the index/graph are built before timing appends
    j.normalized(CellKey::new(0, 0, 0)).unwrap();
    let ops = ops(rows, n_ops, 0x9E37_79B9_7F4A_7C15);

    // append: Journal vs bare sequential calls on a Workbook; both warmed by
    // one untimed value write (builds the dependency graph)
    let warm = Op::Content(CellKey::new(0, 0, 0), Content::Number(0.0));
    j.append(Api::Native, warm.clone(), None).unwrap();
    let after_warm = CUR.load(Relaxed);
    let t = Instant::now();
    for op in &ops {
        j.append(Api::Native, op.clone(), None).unwrap();
    }
    let append = t.elapsed();
    let after_appends = CUR.load(Relaxed);
    let wb_before = CUR.load(Relaxed);
    let mut wb = Workbook::from_bytes(base.clone()).unwrap();
    apply_seq(&mut wb, &warm).unwrap();
    let wb_warm = CUR.load(Relaxed) - wb_before;
    let t = Instant::now();
    for op in &ops {
        apply_seq(&mut wb, op).unwrap();
    }
    let seq = t.elapsed();
    println!("append {n_ops} ops (half values, quarter strings, quarter patches): Journal {} ({}), sequential {} ({})", ms(append), us_per(append, n_ops), ms(seq), us_per(seq, n_ops));
    println!("heap: after first write (graph built) {}, after {n_ops} appends {} (log {} entries)", mib(after_warm - before), mib(after_appends - before), j.log().len());

    println!("sequential Workbook heap: after first write {}, after {n_ops} ops {}", mib(wb_warm), mib(CUR.load(Relaxed) - wb_before));
    // sequential save for reference
    let t = Instant::now();
    let seq_bytes = wb.to_bytes().unwrap();
    let seq_save = t.elapsed();
    drop(wb);
    println!("sequential to_bytes: {} ({} bytes)", ms(seq_save), seq_bytes.len());

    // truncation replay: cut half, force the rebuild
    {
        let half = n_ops as u64 / 2;
        let mut j2 = Journal::open(base.clone()).unwrap();
        for op in &ops {
            j2.append(Api::Native, op.clone(), None).unwrap();
        }
        let t = Instant::now();
        j2.truncate(half).unwrap();
        j2.shadow().unwrap();
        println!("truncate to {half} + replay (from_bytes + {half} ops): {}", ms(t.elapsed()));
        let t = Instant::now();
        j2.mark_shadow_stale();
        j2.shadow().unwrap();
        j2.normalized(CellKey::new(0, 0, 0)).unwrap();
        println!("replay of {half} ops incl. first read: {}", ms(t.elapsed()));
    }

    // Commit: phases, then the whole call
    let entries = xlsx_edit_journal_spike::normalize::normalize(j.log());
    let t = Instant::now();
    for _ in 0..10 {
        let e = xlsx_edit_journal_spike::normalize::normalize(j.log());
        let _ = xlsx_edit_journal_spike::normalize::batches(&e);
    }
    let norm = t.elapsed() / 10;
    let bs = xlsx_edit_journal_spike::normalize::batches(&entries);
    println!("normalize + batches: {} ({} log ops -> {} entries, {} batches)", ms(norm), j.log().len(), entries.len(), bs.len());
    j.mark_shadow_stale(); // Commit drops the shadow first (§7 step 2)
    let before_c = reset_peak();
    let t = Instant::now();
    let (bytes, report) = commit::compile(j.base(), j.log(), None).unwrap();
    let commit_t = t.elapsed();
    println!(
        "Commit (open working copy + compile + serialize): {}, peak heap above the log {} ({} write_cells calls, {} single calls, {} no-ops)",
        ms(commit_t),
        mib(PEAK.load(Relaxed) - before_c),
        report.write_cells_calls,
        report.single_calls,
        report.skipped_noops
    );
    println!("  of which: open {}, apply {}, serialize {}", ms(report.t_open), ms(report.t_apply), ms(report.t_serialize));
    let t = Instant::now();
    let mut reopened = Workbook::from_bytes(bytes.clone()).unwrap();
    reopened.worksheet_by_index(0).unwrap().cell(0, 0).unwrap();
    let reopen = t.elapsed();
    println!("reopen of the output (next shadow, §10.1): {}", ms(reopen));
    println!("save total (Commit + reopen) {} vs sequential to_bytes {}: {:.1}x", ms(commit_t + reopen), ms(seq_save), (commit_t + reopen).as_secs_f64() / seq_save.as_secs_f64());
    drop(reopened);

    // a batch-heavy Commit: distinct cells, values only (write_cells path)
    let mut j3 = Journal::open(base.clone()).unwrap();
    let n3 = (rows as usize).min(10_000);
    for r in 0..n3 as u32 {
        j3.append(Api::Native, Op::Content(CellKey::new(0, r, 1), Content::Number(-1.0)), None).unwrap();
    }
    j3.mark_shadow_stale();
    let t = Instant::now();
    let (_, rep3) = commit::compile(j3.base(), j3.log(), None).unwrap();
    println!("Commit of {n3} distinct value writes: {} ({} batch, {} write_cells call; open {}, apply {}, serialize {})", ms(t.elapsed()), rep3.batches, rep3.write_cells_calls, ms(rep3.t_open), ms(rep3.t_apply), ms(rep3.t_serialize));
    let mut wb = Workbook::from_bytes(base.clone()).unwrap();
    let t = Instant::now();
    for r in 0..n3 as u32 {
        wb.worksheet_by_index(0).unwrap().cell_mut(r, 1).unwrap().set_number(-1.0).unwrap();
    }
    let calls = t.elapsed();
    let t = Instant::now();
    let _ = wb.to_bytes().unwrap();
    println!("sequential: {n3} set_number {} + to_bytes {}", ms(calls), ms(t.elapsed()));
}
