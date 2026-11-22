"""Corpus evidence for ADR-0020 (inserting next to mc:AlternateContent).

Reads untrusted .xlsx/.xlsm/.docx/.pptx packages with zipfile + xml.etree only.
Parts that carry a DTD or an entity declaration are skipped (no entity is ever
resolved); nothing is extracted or executed; packages are de-duplicated by
SHA-256. Run with `python3 -I` from the repository root:

    python3 -I spikes/ac-rank/ac_rank.py GENERATED_DIR DIR_OR_FILE...

GENERATED_DIR is crates/ooxml-schema/src/generated: the slot tables and the
`ChildOf` impls are read from the generated Rust, so the ranks are exactly the
ones `ooxml-core`'s lens uses (ADR-0002 §4.1), and "an insert the library
supports" means "a typed `C` with `impl ChildOf<P> for C`".

For every element P that has an `mc:AlternateContent` (AC) **direct child**, it
records P's QName, P's generated spec (if any), and for each AC: the branches
(`mc:Choice@Requires`, `mc:Fallback`), the QNames each branch holds, and the
rank interval of each branch.

Rank interval of a branch (the rule ADR-0020 proposes): run ADR-0002's greedy
slot assignment over the branch's *listed* children, starting from the rank in
effect before the AC; lo = the first assigned slot, hi = the last. Unlisted
children are skipped (as direct unlisted children are). A nested AC inside a
branch contributes its own interval. The AC's interval is the union over the
branches that hold a listed element: [min lo, max hi]. An AC none of whose
branches holds a listed element is "unranked" and is treated as unlisted.

Insert simulation, for each (P instance, typed child C of P) with C in exactly
one slot s: today's rule always refuses (UnresolvedSibling). The proposed rule:

  - a sibling (listed element or ranked AC) with interval [lo, hi] may precede
    the new node iff hi <= s and may follow it iff lo >= s;
  - the new node goes before the first sibling that may not precede it; if that
    sibling may not follow it either (lo < s < hi, or a "straddling" AC), the
    insert is refused (`straddle`);
  - occurrence limits count direct children plus, for each AC, the maximum over
    its branches of the matching children in that branch (`limit`; `limit_ac`
    when the limit is reached only because of children inside AC);
  - a child list that admits no greedy assignment is refused (`disorder`).

It also evaluates ADR-0002 §4.3's recorded rule ("rank = minimum slot rank of
the first listed element across branches", one point) and counts the inserts it
would place where some branch makes the result out of order (`old_rule_wrong`).

Output: one '#'-prefixed summary block on stdout (counts per format), then
per-parent and per-(parent, child) tables. Deterministic for a given input.
"""
import collections
import hashlib
import os
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
AC = "{%s}AlternateContent" % MC
CHOICE = "{%s}Choice" % MC
FALLBACK = "{%s}Fallback" % MC
EXTS = {".xlsx": "xlsx", ".xlsm": "xlsx", ".docx": "docx", ".docm": "docx", ".pptx": "pptx", ".pptm": "pptx"}
MAX_PART = 64 * 1024 * 1024

# ---------------------------------------------------------------- generated

def load_generated(gdir):
    """Specs keyed 'module::Enum': qname, slots (or None), child set."""
    ns = {}
    specs = {}
    childof = collections.defaultdict(set)
    texts = {}
    for f in sorted(os.listdir(gdir)):
        if not f.endswith(".rs") or f == "mod.rs":
            continue
        mod = f[:-3]
        t = open(os.path.join(gdir, f), encoding="utf-8").read()
        texts[mod] = t
        for m in re.finditer(r'pub const (NS_[A-Z0-9_]+): &str = "([^"]*)";', t):
            ns[m.group(1)] = m.group(2)
    prefix_of = {u: k[3:].lower() for k, u in ns.items()}
    for mod, t in texts.items():
        for m in re.finditer(r"impl ooxml_core::ElementSpec for (\w+) \{(.*?)\n\}", t, re.S):
            name, body = m.group(1), m.group(2)
            q = re.search(r'const QNAME: ooxml_core::QName = ooxml_core::QName::new\(\s*(NS_\w+),\s*"([^"]+)",?\s*\)', body)
            if not q:
                continue
            qname = "{%s}%s" % (ns[q.group(1)], q.group(2))
            kind = re.search(r"const CONTENT: ooxml_core::Content =\s*ooxml_core::Content::(\w+)", body).group(1)
            slots = None
            if kind == "Slots":
                slots = []
                for s in re.finditer(r"ooxml_core::Slot \{(.*?)\n\s*\},", body, re.S):
                    sb = s.group(1)
                    names = tuple("{%s}%s" % (ns[a], b) for a, b in re.findall(r'QName::new\(\s*(NS_\w+),\s*"([^"]+)",?\s*\)', sb))
                    wild = "wildcard: Some(" in sb
                    me = re.search(r"max_each: (None|Some\((\d+)\))", sb)
                    mt = re.search(r"max_total: (None|Some\((\d+)\))", sb)
                    slots.append({
                        "names": names,
                        "wild": wild,
                        "max_each": int(me.group(2)) if me.group(2) else None,
                        "max_total": int(mt.group(2)) if mt.group(2) else None,
                    })
            specs["%s::%s" % (mod, name)] = {"qname": qname, "kind": kind, "slots": slots, "sdk": re.search(r'SDK_NAME: &\'static str =\s*"([^"]+)"', body).group(1)}
        for m in re.finditer(r"^impl ooxml_core::ChildOf<(\w+)> for ([\w:]+) \{\}", t, re.M):
            parent = "%s::%s" % (mod, m.group(1))
            child = m.group(2)
            child = "%s::%s" % (child.split("::")[1], child.split("::")[2]) if child.startswith("super::") else "%s::%s" % (mod, child)
            childof[parent].add(child)
    by_q = collections.defaultdict(list)
    for k, v in specs.items():
        by_q[v["qname"]].append(k)
    return specs, childof, by_q, prefix_of


def short(q, prefix_of):
    if q.startswith("{"):
        u, l = q[1:].split("}", 1)
        if u == MC:
            return "mc:" + l
        p = prefix_of.get(u)
        if p is None:
            p = re.sub(r"[^a-z0-9]", "", u.rsplit("/", 1)[-1].lower())[:12] or "?"
            p = "?" + p
        return "%s:%s" % (p, l)
    return q


# ---------------------------------------------------------------- slot logic

def candidates(slots, q):
    named = [i for i, s in enumerate(slots) if q in s["names"]]
    if named:
        return named
    return [i for i, s in enumerate(slots) if s["wild"]]


class Disorder(Exception):
    pass


def interval(el, slots, cur):
    """(lo, hi, per-branch info) of an AC whose rank starts at `cur`, or None
    when no branch holds a listed element. Raises Disorder."""
    branches = []
    for b in el:
        if b.tag not in (CHOICE, FALLBACK):
            continue
        c = cur
        lo = hi = None
        for ch in b:
            if not isinstance(ch.tag, str):
                continue
            if ch.tag == AC:
                r = interval(ch, slots, c)
                if r is None:
                    continue
                l, h = r[0], r[1]
            else:
                cands = candidates(slots, ch.tag)
                if not cands:
                    continue
                f = [r for r in cands if r >= c]
                if not f:
                    raise Disorder()
                l = h = f[0]
            if lo is None:
                lo = l
            hi = h
            c = h
        branches.append((lo, hi))
    ranked = [b for b in branches if b[0] is not None]
    if not ranked:
        return None
    return (min(b[0] for b in ranked), max(b[1] for b in ranked), branches)


def old_point(el, slots):
    """ADR-0002 §4.3's recorded rule: min over branches of the first listed
    element's (lowest) slot."""
    best = None
    for b in el:
        if b.tag not in (CHOICE, FALLBACK):
            continue
        for ch in b:
            if isinstance(ch.tag, str) and ch.tag != AC and candidates(slots, ch.tag):
                r = min(candidates(slots, ch.tag))
                best = r if best is None else min(best, r)
                break
    return best


def branch_counts(el, slots, s, q):
    """(max over branches of children in slot s, ... of children named q)."""
    best_s = best_q = 0
    for b in el:
        if b.tag not in (CHOICE, FALLBACK):
            continue
        ns_ = nq = 0
        for ch in b:
            if not isinstance(ch.tag, str):
                continue
            if ch.tag == AC:
                a, c = branch_counts(ch, slots, s, q)
                ns_ += a
                nq += c
            elif s in candidates(slots, ch.tag):
                ns_ += 1
                nq += ch.tag == q
        best_s, best_q = max(best_s, ns_), max(best_q, nq)
    return best_s, best_q


def assign(parent, slots):
    """Greedy assignment of the children: list of (index, kind, lo, hi, el)."""
    out = []
    cur = 0
    for i, ch in enumerate(parent):
        if not isinstance(ch.tag, str):
            continue
        if ch.tag == AC:
            r = interval(ch, slots, cur)
            if r is None:
                continue
            out.append((i, "ac", r[0], r[1], ch))
            cur = r[1]
            continue
        cands = candidates(slots, ch.tag)
        if not cands:
            continue
        f = [r for r in cands if r >= cur]
        if not f:
            raise Disorder()
        out.append((i, "el", f[0], f[0], ch))
        cur = f[0]
    return out


def simulate(parent, slots, s, q):
    """Outcome of appending a q child in slot s under the proposed rule."""
    try:
        seq = assign(parent, slots)
    except Disorder:
        return "disorder"
    same_s = same_q = ac_s = ac_q = 0
    at = None
    for i, kind, lo, hi, el in seq:
        if at is None and hi > s:
            if lo < s:
                return "straddle"
            at = i
        if kind == "el":
            if lo == s:
                same_s += 1
                same_q += el.tag == q
        elif lo <= s <= hi:
            a, b = branch_counts(el, slots, s, q)
            ac_s += a
            ac_q += b
    sl = slots[s]
    def over(n_s, n_q):
        return (sl["max_each"] is not None and n_q >= sl["max_each"]) or (sl["max_total"] is not None and n_s >= sl["max_total"])
    if over(same_s + ac_s, same_q + ac_q):
        return "limit" if over(same_s, same_q) else "limit_ac"
    return "ok"


def old_rule_wrong(parent, slots, s):
    """Would ADR-0002's point rule put the new node where some branch makes the
    order invalid? (Only meaningful when the proposed rule says ok.)"""
    pos_old = None
    cur = 0
    for i, ch in enumerate(parent):
        if not isinstance(ch.tag, str):
            continue
        if ch.tag == AC:
            r = old_point(ch, slots)
            if r is None:
                continue
            rank = r
        else:
            c = candidates(slots, ch.tag)
            if not c:
                continue
            f = [r for r in c if r >= cur]
            if not f:
                return False
            rank = f[0]
        cur = rank
        if pos_old is None and rank > s:
            pos_old = i
    # validity of the old position under the union intervals
    for i, kind, lo, hi, el in assign(parent, slots):
        before_new = pos_old is None or i < pos_old
        if before_new and hi > s:
            return True
        if not before_new and lo < s:
            return True
    return False


# ---------------------------------------------------------------- walk

def resolve(by_q, specs, childof, q, parent_spec):
    cands = by_q.get(q, [])
    if parent_spec is None and len(cands) > 1:
        # a part root: prefer specs that are nobody's child (p:CT_Slide/p:sld
        # over p:CT_OutlineViewSlideEntry/p:sld)
        children = set().union(*childof.values())
        roots = [c for c in cands if c not in children]
        cands = roots or cands
        with_slots = [c for c in cands if specs[c]["kind"] == "Slots"]
        cands = with_slots or cands
    if parent_spec is not None:
        c2 = [c for c in cands if c in childof.get(parent_spec, ())]
        if c2:
            cands = c2
        elif parent_spec in specs:
            cands = [] if specs[parent_spec]["kind"] == "Slots" else cands
    if not cands:
        return None
    if len(cands) == 1:
        return cands[0]
    first = specs[cands[0]]
    if all(specs[c]["slots"] == first["slots"] for c in cands):
        return cands[0]
    return "?ambiguous"


class Stats:
    def __init__(self):
        self.c = collections.Counter()
        self.files = collections.defaultdict(set)
        self.parent = collections.defaultdict(collections.Counter)
        self.wrapped = collections.defaultdict(collections.Counter)
        self.pairs = collections.defaultdict(collections.Counter)

    def add(self, key, f, n=1):
        self.c[key] += n
        self.files[key].add(f)


def walk(root, fmt, fname, part, st, G, decl_prefixes):
    specs, childof, by_q, prefix_of = G
    sh = lambda q: short(q, prefix_of)

    def visit(el, spec, depth):
        acs = [ch for ch in el if isinstance(ch.tag, str) and ch.tag == AC]
        if acs:
            pq = sh(el.tag)
            status = "not-generated" if spec is None else ("ambiguous" if spec == "?ambiguous" else specs[spec]["kind"])
            st.add("%s parents_with_ac" % fmt, fname)
            st.add("%s ac_children" % fmt, fname, len(acs))
            st.parent[(fmt, pq)]["instances"] += 1
            st.parent[(fmt, pq)]["acs"] += len(acs)
            st.parent[(fmt, pq)]["status:" + status] += 1
            st.parent[(fmt, pq)]["file:" + fname] = 1
            slots = specs[spec]["slots"] if status == "Slots" else None
            for a in acs:
                ac_stats(a, fmt, fname, pq, slots, st, sh, decl_prefixes, el)
            if slots is not None:
                pairs(el, spec, fmt, fname, pq, st, sh, G)
        for ch in el:
            if not isinstance(ch.tag, str):
                continue
            if ch.tag == AC:
                for b in ch:
                    if isinstance(b.tag, str) and b.tag in (CHOICE, FALLBACK):
                        for g in b:
                            if isinstance(g.tag, str):
                                if g.tag == AC:
                                    visit_ac_nested(g, spec, depth + 1)
                                else:
                                    visit(g, resolve(by_q, specs, childof, g.tag, spec if spec != "?ambiguous" else None), depth + 1)
                continue
            visit(ch, resolve(by_q, specs, childof, ch.tag, spec if spec != "?ambiguous" else None), depth + 1)

    def visit_ac_nested(a, spec, depth):
        for b in a:
            if isinstance(b.tag, str) and b.tag in (CHOICE, FALLBACK):
                for g in b:
                    if isinstance(g.tag, str):
                        if g.tag == AC:
                            visit_ac_nested(g, spec, depth + 1)
                        else:
                            visit(g, resolve(by_q, specs, childof, g.tag, spec if spec != "?ambiguous" else None), depth + 1)

    visit(root, resolve(by_q, specs, childof, root.tag, None), 0)


def ac_stats(a, fmt, fname, pq, slots, st, sh, decl_prefixes, parent):
    branches = [b for b in a if isinstance(b.tag, str)]
    other = [b for b in branches if b.tag not in (CHOICE, FALLBACK)]
    if other:
        st.add("%s ac_with_non_branch_child" % fmt, fname)
    choices = [b for b in branches if b.tag == CHOICE]
    fallbacks = [b for b in branches if b.tag == FALLBACK]
    st.add("%s ac_choices=%d_fallbacks=%d" % (fmt, len(choices), len(fallbacks)), fname)
    for b in choices:
        for p in (b.get("Requires") or "").split():
            st.add("%s requires=%s" % (fmt, p), fname)
            if p not in decl_prefixes:
                st.add("%s requires_undeclared_prefix" % fmt, fname)
    for b in choices + fallbacks:
        kids = [g for g in b if isinstance(g.tag, str)]
        if not kids:
            st.add("%s branch_empty" % fmt, fname)
        if any(g.tag == AC for g in kids):
            st.add("%s branch_with_nested_ac" % fmt, fname)
        names = tuple(sh(g.tag) for g in kids)
        st.wrapped[(fmt, pq)]["%s:%s" % ("C" if b.tag == CHOICE else "F", " ".join(dict.fromkeys(names)) or "(empty)")] += 1
    if slots is None:
        return
    # rank evidence (cur = the rank before this AC, from the greedy prefix)
    cur = 0
    try:
        for ch in parent:
            if ch is a:
                break
            if not isinstance(ch.tag, str):
                continue
            if ch.tag == AC:
                r = interval(ch, slots, cur)
                if r is not None:
                    cur = r[1]
                continue
            c = candidates(slots, ch.tag)
            if c:
                f = [x for x in c if x >= cur]
                if not f:
                    raise Disorder()
                cur = f[0]
        r = interval(a, slots, cur)
    except Disorder:
        st.add("%s ac_rank_disorder" % fmt, fname)
        return
    st.add("%s ac_in_generated_parent" % fmt, fname)
    if r is None:
        st.add("%s ac_unranked(only_unlisted_or_empty)" % fmt, fname)
        return
    lo, hi, br = r
    ranked = [x for x in br if x[0] is not None]
    if len(ranked) < len(br):
        st.add("%s ac_some_branch_unranked" % fmt, fname)
    if len(set(ranked)) > 1:
        st.add("%s ac_branches_disagree" % fmt, fname)
        st.parent[(fmt, pq)]["disagree"] += 1
    if any(x[0] != x[1] for x in ranked):
        st.add("%s ac_branch_spans_slots" % fmt, fname)
        st.parent[(fmt, pq)]["spans"] += 1
    if lo == hi:
        st.add("%s ac_single_rank" % fmt, fname)
    # does the recorded point rule differ from the interval?
    if old_point(a, slots) != lo or lo != hi:
        st.add("%s ac_point_rule_loses_information" % fmt, fname)


def pairs(el, spec, fmt, fname, pq, st, sh, G):
    specs, childof, by_q, prefix_of = G
    slots = specs[spec]["slots"]
    for c in sorted(childof.get(spec, ())):
        q = specs[c]["qname"]
        ss = candidates(slots, q)
        if len(ss) != 1:
            st.add("%s pair_ambiguous_slot" % fmt, fname)
            continue
        s = ss[0]
        out = simulate(el, slots, s, q)
        if all(x["wild"] and not x["names"] for x in slots):
            # a wildcard-only parent (a:graphicData): every child is in the
            # one `any` slot; counted apart so it does not swamp the totals
            st.add("%s wildcard_parent_pair_%s" % (fmt, out), fname)
            st.pairs[(fmt, pq, "(any typed child)")][out] += 1
            continue
        st.add("%s pair_total" % fmt, fname)
        st.add("%s pair_%s" % (fmt, out), fname)
        st.pairs[(fmt, pq, sh(q))][out] += 1
        if out == "ok" and old_rule_wrong(el, slots, s):
            st.add("%s pair_old_rule_wrong" % fmt, fname)
            st.pairs[(fmt, pq, sh(q))]["old_rule_wrong"] += 1


# ---------------------------------------------------------------- packages

def declared_prefixes(data):
    out = set()
    try:
        for ev, (p, u) in ET.iterparse(_bytes_io(data), events=("start-ns",)):
            out.add(p)
    except ET.ParseError:
        pass
    return out


def _bytes_io(data):
    import io
    return io.BytesIO(data)


def scan(path, fmt, st, G, seen):
    try:
        raw = open(path, "rb").read()
    except OSError:
        return
    h = hashlib.sha256(raw).hexdigest()
    if h in seen:
        st.c["%s duplicate_packages" % fmt] += 1
        return
    seen.add(h)
    fname = os.path.basename(path)
    try:
        z = zipfile.ZipFile(_bytes_io(raw))
        names = z.namelist()
    except Exception:
        st.c["%s unreadable_packages" % fmt] += 1
        return
    st.add("%s packages" % fmt, fname)
    for n in names:
        if not n.endswith(".xml") or n.endswith(".rels"):
            continue
        try:
            info = z.getinfo(n)
            if info.file_size > MAX_PART:
                continue
            data = z.read(n)
        except Exception:
            continue
        if b"<!DOCTYPE" in data or b"<!ENTITY" in data:
            st.c["%s parts_with_dtd_skipped" % fmt] += 1
            continue
        if MC.encode() not in data:
            continue
        try:
            root = ET.fromstring(data)
        except ET.ParseError:
            st.c["%s parts_unparsable" % fmt] += 1
            continue
        if not any(e.tag == AC for e in root.iter()):
            continue
        st.add("%s packages_with_ac" % fmt, fname)
        walk(root, fmt, fname, n, st, G, declared_prefixes(data))


def main():
    gdir = sys.argv[1]
    G = load_generated(gdir)
    st = Stats()
    seen = set()
    paths = []
    for a in sys.argv[2:]:
        if os.path.isdir(a):
            for dp, dn, fn in os.walk(a):
                dn.sort()
                for f in sorted(fn):
                    paths.append(os.path.join(dp, f))
        else:
            paths.append(a)
    for p in paths:
        fmt = EXTS.get(os.path.splitext(p)[1].lower())
        if fmt:
            scan(p, fmt, st, G, seen)
    print("# ---- counters: key  files=N  total=M")
    for k in sorted(st.c):
        print("# %s\tfiles=%d\ttotal=%d" % (k, len(st.files.get(k, ())), st.c[k]))
    print("\n## parents with an AC child: fmt parent instances acs files status... disagree spans")
    for (fmt, pq), c in sorted(st.parent.items(), key=lambda kv: (kv[0][0], -kv[1]["instances"])):
        files = sum(1 for k in c if k.startswith("file:"))
        status = ",".join("%s=%d" % (k[7:], v) for k, v in sorted(c.items()) if k.startswith("status:"))
        print("%s\t%s\tinst=%d\tac=%d\tfiles=%d\t%s\tdisagree=%d\tspans=%d" % (fmt, pq, c["instances"], c["acs"], files, status, c["disagree"], c["spans"]))
    print("\n## branch contents per parent (C = mc:Choice, F = mc:Fallback; element QNames in order, de-duplicated)")
    for (fmt, pq), c in sorted(st.wrapped.items()):
        for k, v in c.most_common(8):
            print("%s\t%s\t%d\t%s" % (fmt, pq, v, k[:160]))
        if len(c) > 8:
            print("%s\t%s\t...\t(%d more forms)" % (fmt, pq, len(c) - 8))
    print("\n## typed inserts (parent, child): outcomes under the proposed rule (today: all refused)")
    for (fmt, pq, cq), c in sorted(st.pairs.items()):
        print("%s\t%s\t%s\t%s" % (fmt, pq, cq, " ".join("%s=%d" % kv for kv in sorted(c.items()))))


if __name__ == "__main__":
    main()
