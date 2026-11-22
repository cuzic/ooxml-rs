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
rank interval (ADR-0020 revision 2, §3):

- Malformed AC (§3.1): an element child that is not a branch, no `mc:Choice`,
  more than one `mc:Fallback`, a `mc:Fallback` that is not last, a `mc:Choice`
  without `Requires`, or a `Requires` prefix not bound **in scope** at that
  `mc:Choice`. A malformed AC refuses every insert into its parent.
- Branch interval: ADR-0002's greedy slot assignment over the branch's element
  members, starting at `cur`, the slot of the listed sibling before the AC (-1,
  "before every slot", when there is none). Listed members are assigned as
  direct children are. An unlisted member *before the first listed one* sets
  lo = cur (it is attached to the sibling before the AC, ADR-0002 §4.3); an
  unlisted member after a listed one is attached to it. A nested AC takes part
  with its own interval. An empty branch has no interval.
- A **non-empty branch with no listed member** (only unlisted or vendor
  elements, or only an unranked nested AC) makes the AC **unranked**; an
  unranked AC refuses every insert into its parent (ADR-0020 Q2, M4).
- The AC's interval is the union of its branches' intervals. An AC without
  `mc:Fallback` has an implicit empty Fallback (no interval).

Insert simulation, for each (P instance, typed child C of P) with C in exactly
one slot s. Today every one is refused (UnresolvedSibling). Outcomes under
ADR-0020 for a plain append (`insert` / `get_or_insert`):

  - `malformed` / `unranked`: an AC of P is malformed / unranked;
  - `disorder`: the child list (ACs by interval) admits no greedy assignment;
  - `straddle`: the first sibling that may not precede the new node (hi > s)
    may not follow it either (lo < s);
  - `limit` / `limit_ac`: AlreadyPresent from direct children alone / only
    because a branch holds matching children (the count is direct children plus,
    per AC, the maximum over its branches);
  - `ok`: placed. `ok_same_slot_ac` additionally counts the `ok` pairs where an
    AC's interval contains s, or an AC has an empty interval (read as in every
    slot, revision 3): the forms that take a `before` callback or a
    positional index (`insert_by`, `insert_in_slot`, `insert_at`, `move_in_by`,
    their `check_*`) refuse those (ADR-0020 §4.2), only a plain append lands.

It also evaluates ADR-0002 §4.3's recorded point rule and counts `ok` inserts
it would place where some branch makes the order invalid (`old_rule_wrong`), and
counts parents with two or more ACs whose `mc:Choice` `Requires` sets are equal
(`parent_correlated_acs`, ADR-0020 §3.2), with their outcome.

Output: '#'-prefixed counters per format, then per-parent, branch-content and
per-(parent, child) tables. Deterministic for a given input.
"""
import collections
import hashlib
import io
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

QN = r'QName::new\(\s*(NS_\w+),\s*"([^"]+)",?\s*\)'


def load_generated(gdir):
    """Specs keyed 'module::Enum': qname, kind, slots (or None); ChildOf sets."""
    ns, specs, texts = {}, {}, {}
    childof = collections.defaultdict(set)
    for f in sorted(os.listdir(gdir)):
        if not f.endswith(".rs") or f == "mod.rs":
            continue
        t = open(os.path.join(gdir, f), encoding="utf-8").read()
        texts[f[:-3]] = t
        for m in re.finditer(r'pub const (NS_[A-Z0-9_]+): &str = "([^"]*)";', t):
            ns[m.group(1)] = m.group(2)
    prefix_of = {u: k[3:].lower() for k, u in ns.items()}
    for mod, t in texts.items():
        for m in re.finditer(r"impl ooxml_core::ElementSpec for (\w+) \{(.*?)\n\}", t, re.S):
            name, body = m.group(1), m.group(2)
            q = re.search(r"const QNAME: ooxml_core::QName = ooxml_core::" + QN, body)
            if not q:
                continue
            kind = re.search(r"const CONTENT: ooxml_core::Content =\s*ooxml_core::Content::(\w+)", body).group(1)
            slots = None
            if kind == "Slots":
                slots = []
                for s in re.finditer(r"ooxml_core::Slot \{(.*?)\n\s*\},", body, re.S):
                    sb = s.group(1)
                    me = re.search(r"max_each: (None|Some\((\d+)\))", sb)
                    mt = re.search(r"max_total: (None|Some\((\d+)\))", sb)
                    slots.append({
                        "names": tuple("{%s}%s" % (ns[a], b) for a, b in re.findall(QN, sb)),
                        "wild": "wildcard: Some(" in sb,
                        "max_each": int(me.group(2)) if me.group(2) else None,
                        "max_total": int(mt.group(2)) if mt.group(2) else None,
                    })
            specs["%s::%s" % (mod, name)] = {"qname": "{%s}%s" % (ns[q.group(1)], q.group(2)), "kind": kind, "slots": slots}
        for m in re.finditer(r"^impl ooxml_core::ChildOf<(\w+)> for ([\w:]+) \{\}", t, re.M):
            c = m.group(2)
            c = "%s::%s" % tuple(c.split("::")[1:3]) if c.startswith("super::") else "%s::%s" % (mod, c)
            childof["%s::%s" % (mod, m.group(1))].add(c)
    by_q = collections.defaultdict(list)
    for k, v in specs.items():
        by_q[v["qname"]].append(k)
    return specs, childof, by_q, prefix_of


def short(q, prefix_of):
    if not q.startswith("{"):
        return q
    u, l = q[1:].split("}", 1)
    if u == MC:
        return "mc:" + l
    p = prefix_of.get(u)
    if p is None:
        p = "?" + (re.sub(r"[^a-z0-9]", "", u.rsplit("/", 1)[-1].lower())[:12] or "?")
    return "%s:%s" % (p, l)


# ---------------------------------------------------------------- AC rank

class Refuse(Exception):
    """kind: 'disorder' | 'unranked' | 'malformed'"""

    def __init__(self, kind):
        super().__init__(kind)
        self.kind = kind


def candidates(slots, q):
    named = [i for i, s in enumerate(slots) if q in s["names"]]
    return named or [i for i, s in enumerate(slots) if s["wild"]]


def elements(el):
    return [c for c in el if isinstance(c.tag, str)]


def check_wellformed(ac, scope):
    """§3.1. `scope[id(choice)]` = prefixes bound in scope at that mc:Choice."""
    kids = elements(ac)
    if any(k.tag not in (CHOICE, FALLBACK) for k in kids):
        raise Refuse("malformed")
    if not any(k.tag == CHOICE for k in kids):
        raise Refuse("malformed")
    fb = [i for i, k in enumerate(kids) if k.tag == FALLBACK]
    if len(fb) > 1 or (fb and fb[0] != len(kids) - 1):
        raise Refuse("malformed")
    for k in kids:
        if k.tag == CHOICE:
            req = (k.get("Requires") or "").split()
            if not req or any(p not in scope.get(id(k), ()) for p in req):
                raise Refuse("malformed")


def interval(ac, slots, cur, scope):
    """(lo, hi, per-branch intervals) of an AC, assigned from `cur`.
    Raises Refuse('malformed' | 'unranked' | 'disorder')."""
    check_wellformed(ac, scope)
    branches = []
    for b in elements(ac):
        members = elements(b)
        if not members:
            branches.append(None)  # empty branch: no interval
            continue
        c, lo, hi = cur, None, None
        for m in members:
            if m.tag == AC:
                l, h, _ = interval(m, slots, c, scope)
                if l is None:
                    continue  # every branch of the nested AC is empty
            else:
                cands = candidates(slots, m.tag)
                if not cands:
                    if lo is None:
                        lo = cur  # leading unlisted: attached to the sibling before the AC
                    continue
                f = [r for r in cands if r >= c]
                if not f:
                    raise Refuse("disorder")
                l = h = f[0]
            if lo is None:
                lo = l
            hi = h
            c = h
        if hi is None:
            raise Refuse("unranked")  # non-empty, nothing listed (M4)
        branches.append((lo, hi))
    ranked = [x for x in branches if x is not None]
    if not ranked:
        return None, None, branches  # every branch empty: constrains nothing
    return min(x[0] for x in ranked), max(x[1] for x in ranked), branches


def assign(parent, slots, scope):
    """Greedy assignment of P's children: [(index, kind, lo, hi, el)]."""
    out, cur = [], -1
    for i, ch in enumerate(parent):
        if not isinstance(ch.tag, str):
            continue
        if ch.tag == AC:
            lo, hi, _ = interval(ch, slots, cur, scope)
            if lo is None:
                continue
            out.append((i, "ac", lo, hi, ch))
            cur = hi
            continue
        cands = candidates(slots, ch.tag)
        if not cands:
            continue
        f = [r for r in cands if r >= cur]
        if not f:
            raise Refuse("disorder")
        out.append((i, "el", f[0], f[0], ch))
        cur = f[0]
    return out


def branch_counts(ac, slots, s, q):
    """(max over branches of members in slot s, ... of members named q)."""
    best_s = best_q = 0
    for b in elements(ac):
        n_s = n_q = 0
        for m in elements(b):
            if m.tag == AC:
                a, c = branch_counts(m, slots, s, q)
                n_s, n_q = n_s + a, n_q + c
            elif s in candidates(slots, m.tag):
                n_s, n_q = n_s + 1, n_q + (m.tag == q)
        best_s, best_q = max(best_s, n_s), max(best_q, n_q)
    return best_s, best_q


def simulate(parent, slots, s, q, scope):
    """(outcome, same_slot_ac) of appending a q child in slot s."""
    try:
        seq = assign(parent, slots, scope)
    except Refuse as e:
        return e.kind, False
    same_s = same_q = ac_s = ac_q = 0
    at, same_slot_ac = None, False
    for i, kind, lo, hi, el in seq:
        if at is None and hi > s:
            if lo < s:
                return "straddle", False
            at = i
        if kind == "el":
            if lo == s:
                same_s, same_q = same_s + 1, same_q + (el.tag == q)
        elif lo <= s <= hi:
            same_slot_ac = True
            a, b = branch_counts(el, slots, s, q)
            ac_s, ac_q = ac_s + a, ac_q + b
    # §4.2: an AC whose branches are all empty is read as in every slot
    for ch in parent:
        if isinstance(ch.tag, str) and ch.tag == AC and interval(ch, slots, cur_before(parent, ch, slots, scope), scope)[0] is None:
            same_slot_ac = True
    sl = slots[s]

    def over(n_s, n_q):
        return (sl["max_each"] is not None and n_q >= sl["max_each"]) or (sl["max_total"] is not None and n_s >= sl["max_total"])
    if over(same_s + ac_s, same_q + ac_q):
        return ("limit" if over(same_s, same_q) else "limit_ac"), same_slot_ac
    return "ok", same_slot_ac


def old_point(ac, slots):
    """ADR-0002 §4.3's recorded rule: min over branches of the first listed member's lowest slot."""
    best = None
    for b in elements(ac):
        for m in elements(b):
            if m.tag != AC and candidates(slots, m.tag):
                r = min(candidates(slots, m.tag))
                best = r if best is None else min(best, r)
                break
    return best


def old_rule_wrong(parent, slots, s, scope):
    pos_old, cur = None, -1
    for i, ch in enumerate(parent):
        if not isinstance(ch.tag, str):
            continue
        if ch.tag == AC:
            rank = old_point(ch, slots)
            if rank is None:
                continue
        else:
            c = [r for r in candidates(slots, ch.tag) if r >= cur]
            if not candidates(slots, ch.tag):
                continue
            if not c:
                return False
            rank = c[0]
        cur = rank
        if pos_old is None and rank > s:
            pos_old = i
    for i, kind, lo, hi, el in assign(parent, slots, scope):
        before_new = pos_old is None or i < pos_old
        if (before_new and hi > s) or (not before_new and lo < s):
            return True
    return False


# ---------------------------------------------------------------- walk

def resolve(G, q, parent_spec):
    specs, childof, by_q, _ = G
    cands = by_q.get(q, [])
    if parent_spec is None and len(cands) > 1:
        children = set().union(*childof.values())
        cands = [c for c in cands if c not in children] or cands
        cands = [c for c in cands if specs[c]["kind"] == "Slots"] or cands
    if parent_spec not in (None, "?ambiguous"):
        c2 = [c for c in cands if c in childof.get(parent_spec, ())]
        if c2:
            cands = c2
        elif specs[parent_spec]["kind"] == "Slots":
            cands = []
    if not cands:
        return None
    if len(cands) == 1 or all(specs[c]["slots"] == specs[cands[0]]["slots"] for c in cands):
        return cands[0]
    return "?ambiguous"


class Stats:
    def __init__(self):
        self.c = collections.Counter()
        self.files = collections.defaultdict(set)
        self.parent = collections.defaultdict(collections.Counter)
        self.wrapped = collections.defaultdict(collections.Counter)
        self.pairs = collections.defaultdict(collections.Counter)
        self.correlated = []

    def add(self, key, f, n=1):
        self.c[key] += n
        self.files[key].add(f)


def walk(root, fmt, fname, st, G, scope, check_pair=None):
    specs, childof, _, prefix_of = G
    sh = lambda q: short(q, prefix_of)

    def visit(el, spec, path):
        acs = [ch for ch in el if isinstance(ch.tag, str) and ch.tag == AC]
        if acs:
            pq = sh(el.tag)
            status = "not-generated" if spec is None else ("ambiguous" if spec == "?ambiguous" else specs[spec]["kind"])
            st.add("%s parents_with_ac" % fmt, fname)
            st.add("%s ac_children" % fmt, fname, len(acs))
            p = st.parent[(fmt, pq)]
            p["instances"] += 1
            p["acs"] += len(acs)
            p["status:" + status] += 1
            p["file:" + fname] = 1
            slots = specs[spec]["slots"] if status == "Slots" else None
            for a in acs:
                ac_stats(a, fmt, fname, pq, slots, st, sh, scope, el)
            if slots is not None:
                pair_stats(el, spec, fmt, fname, pq, st, sh, G, scope, acs, path, check_pair)
        for i, child in enumerate(elements(el)):
            child_path = (*path, i)
            if child.tag == AC:
                visit_branches(child, spec, child_path)
            else:
                visit(child, resolve(G, child.tag, spec), child_path)

    def visit_branches(ac, spec, path):
        for i, branch in enumerate(elements(ac)):
            for j, member in enumerate(elements(branch)):
                member_path = (*path, i, j)
                if member.tag == AC:
                    visit_branches(member, spec, member_path)
                else:
                    visit(member, resolve(G, member.tag, spec), member_path)

    visit(root, resolve(G, root.tag, None), ())


def cur_before(parent, a, slots, scope):
    """`cur` at AC `a`: the greedy assignment of the siblings before it (as `assign`)."""
    cur = -1
    for ch in parent:
        if ch is a:
            break
        if not isinstance(ch.tag, str):
            continue
        if ch.tag == AC:
            lo, hi, _ = interval(ch, slots, cur, scope)
            if lo is not None:
                cur = hi
            continue
        cands = candidates(slots, ch.tag)
        if cands:
            f = [r for r in cands if r >= cur]
            if not f:
                raise Refuse("disorder")
            cur = f[0]
    return cur


def ac_stats(a, fmt, fname, pq, slots, st, sh, scope, parent):
    kids = elements(a)
    choices = [b for b in kids if b.tag == CHOICE]
    fallbacks = [b for b in kids if b.tag == FALLBACK]
    st.add("%s ac_choices=%d_fallbacks=%d" % (fmt, len(choices), len(fallbacks)), fname)
    for b in choices:
        for pfx in (b.get("Requires") or "").split():
            st.add("%s requires=%s" % (fmt, pfx), fname)
            if pfx not in scope.get(id(b), ()):
                st.add("%s requires_prefix_not_in_scope" % fmt, fname)
    try:
        check_wellformed(a, scope)
    except Refuse:
        st.add("%s ac_malformed" % fmt, fname)
    for b in kids:
        if b.tag not in (CHOICE, FALLBACK):
            continue
        members = elements(b)
        if not members:
            st.add("%s branch_empty" % fmt, fname)
        if any(m.tag == AC for m in members):
            st.add("%s branch_with_nested_ac" % fmt, fname)
        if slots is not None and members:
            listed = [m for m in members if m.tag != AC and candidates(slots, m.tag)]
            if not listed and not any(m.tag == AC for m in members):
                st.add("%s branch_nonempty_unlisted_only" % fmt, fname)
            elif members[0].tag != AC and not candidates(slots, members[0].tag):
                st.add("%s branch_starts_unlisted" % fmt, fname)
        names = " ".join(dict.fromkeys(sh(m.tag) for m in members)) or "(empty)"
        st.wrapped[(fmt, pq)]["%s:%s" % ("C" if b.tag == CHOICE else "F", names)] += 1
    if slots is None:
        return
    st.add("%s ac_in_generated_parent" % fmt, fname)
    try:
        lo, hi, br = interval(a, slots, cur_before(parent, a, slots, scope), scope)
    except Refuse as e:
        st.add("%s ac_%s" % (fmt, e.kind), fname)
        return
    if lo is None:
        st.add("%s ac_all_branches_empty" % fmt, fname)
        return
    ranked = [x for x in br if x is not None]
    if len(set(ranked)) > 1:
        st.add("%s ac_branches_disagree" % fmt, fname)
        st.parent[(fmt, pq)]["disagree"] += 1
    if any(x[0] != x[1] for x in ranked):
        st.add("%s ac_branch_spans_slots" % fmt, fname)
        st.parent[(fmt, pq)]["spans"] += 1
    if lo == hi:
        st.add("%s ac_single_rank" % fmt, fname)


def pair_stats(el, spec, fmt, fname, pq, st, sh, G, scope, acs, path, check_pair):
    specs, childof, _, _ = G
    slots = specs[spec]["slots"]
    reqs = [frozenset((c.get("Requires") or "").split()) for a in acs for c in elements(a) if c.tag == CHOICE]
    correlated = len(acs) > 1 and len(reqs) != len(set(reqs))
    if correlated:
        st.add("%s parent_correlated_acs" % fmt, fname)
    outcomes = collections.Counter()
    for c in sorted(childof.get(spec, ())):
        q = specs[c]["qname"]
        ss = candidates(slots, q)
        if len(ss) != 1:
            st.add("%s pair_ambiguous_slot" % fmt, fname)
            continue
        s = ss[0]
        out, same_slot_ac = simulate(el, slots, s, q, scope)
        outcomes[out] += 1
        if all(x["wild"] and not x["names"] for x in slots):
            # a wildcard-only parent (a:graphicData): counted apart
            st.add("%s wildcard_parent_pair_%s" % (fmt, out), fname)
            st.pairs[(fmt, pq, "(any typed child)")][out] += 1
            continue
        if check_pair is not None:
            check_pair(path, spec, c, out, same_slot_ac)
        st.add("%s pair_total" % fmt, fname)
        st.add("%s pair_%s" % (fmt, out), fname)
        st.pairs[(fmt, pq, sh(q))][out] += 1
        if out == "ok" and same_slot_ac:
            st.add("%s pair_ok_same_slot_ac" % fmt, fname)
            st.pairs[(fmt, pq, sh(q))]["ok_same_slot_ac"] += 1
        if out == "ok" and old_rule_wrong(el, slots, s, scope):
            st.add("%s pair_old_rule_wrong" % fmt, fname)
            st.pairs[(fmt, pq, sh(q))]["old_rule_wrong"] += 1
    if correlated:
        st.correlated.append((fmt, fname, pq, len(acs), " ".join("%s=%d" % kv for kv in sorted(outcomes.items()))))


# ---------------------------------------------------------------- packages

def parse_with_scope(data):
    """Root element, and {id(mc:Choice): prefixes bound in scope there}."""
    scope = {}
    stack = [frozenset(["xml"])]
    pending = []
    root = None
    for ev, obj in ET.iterparse(io.BytesIO(data), events=("start-ns", "start", "end")):
        if ev == "start-ns":
            pending.append(obj[0])
        elif ev == "start":
            cur = stack[-1] | frozenset(pending) if pending else stack[-1]
            pending = []
            stack.append(cur)
            if root is None:
                root = obj
            if obj.tag == CHOICE:
                scope[id(obj)] = cur
        else:
            stack.pop()
    return root, scope


def scan(path, fmt, st, G, seen, part_checker=None):
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
        z = zipfile.ZipFile(io.BytesIO(raw))
        names = z.namelist()
    except Exception:
        st.c["%s unreadable_packages" % fmt] += 1
        return
    st.add("%s packages" % fmt, fname)
    for n in names:
        if not n.endswith(".xml"):
            continue
        try:
            if z.getinfo(n).file_size > MAX_PART:
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
            root, scope = parse_with_scope(data)
        except ET.ParseError:
            st.c["%s parts_unparsable" % fmt] += 1
            continue
        if not any(e.tag == AC for e in root.iter()):
            continue
        st.add("%s packages_with_ac" % fmt, fname)
        check_pair = part_checker(data, fmt, fname, n) if part_checker is not None else None
        walk(root, fmt, fname, st, G, scope, check_pair)


def main():
    G = load_generated(sys.argv[1])
    st, seen, paths = Stats(), set(), []
    for a in sys.argv[2:]:
        if os.path.isdir(a):
            for dp, dn, fn in os.walk(a):
                dn.sort()
                paths.extend(os.path.join(dp, f) for f in sorted(fn))
        else:
            paths.append(a)
    for p in paths:
        fmt = EXTS.get(os.path.splitext(p)[1].lower())
        if fmt:
            scan(p, fmt, st, G, seen)
    print("# ---- counters: key  files=N  total=M")
    for k in sorted(st.c):
        print("# %s\tfiles=%d\ttotal=%d" % (k, len(st.files.get(k, ())), st.c[k]))
    print("\n## parents with an AC child: fmt parent instances acs files status disagree spans")
    for (fmt, pq), c in sorted(st.parent.items(), key=lambda kv: (kv[0][0], -kv[1]["instances"], kv[0][1])):
        files = sum(1 for k in c if k.startswith("file:"))
        status = ",".join("%s=%d" % (k[7:], v) for k, v in sorted(c.items()) if k.startswith("status:"))
        print("%s\t%s\tinst=%d\tac=%d\tfiles=%d\t%s\tdisagree=%d\tspans=%d" % (fmt, pq, c["instances"], c["acs"], files, status, c["disagree"], c["spans"]))
    print("\n## parents with two or more ACs whose mc:Choice Requires are equal (§3.2): fmt file parent acs outcomes")
    for row in sorted(st.correlated):
        print("%s\t%s\t%s\tacs=%d\t%s" % row)
    print("\n## branch contents per parent (C = mc:Choice, F = mc:Fallback; element QNames in order, de-duplicated)")
    for (fmt, pq), c in sorted(st.wrapped.items()):
        for k, v in sorted(c.items(), key=lambda kv: (-kv[1], kv[0]))[:8]:
            print("%s\t%s\t%d\t%s" % (fmt, pq, v, k[:160]))
        if len(c) > 8:
            print("%s\t%s\t...\t(%d more forms)" % (fmt, pq, len(c) - 8))
    print("\n## typed inserts (parent, child): outcomes of a plain append under ADR-0020 (today: all refused)")
    for (fmt, pq, cq), c in sorted(st.pairs.items()):
        print("%s\t%s\t%s\t%s" % (fmt, pq, cq, " ".join("%s=%d" % kv for kv in sorted(c.items()))))


if __name__ == "__main__":
    main()
