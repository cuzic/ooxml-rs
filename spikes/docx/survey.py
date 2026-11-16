"""Feature survey of .docx files for docs/roadmap/docx.md and ADR-0004.

Usage: python3 -I survey.py <dir of .docx> [--json out.json]

Input files are untrusted data: this script only reads them with zipfile and
ElementTree (no external entities are resolved by ElementTree; DTDs are not
fetched). It never writes into the input directory and never executes content.

For every file it reports which Word features occur, where, and how often,
plus the cross-part ID facts ADR-0004 relies on (duplicate bookmark ids,
paraId collisions, comment id agreement between parts, sectPr placement).
"""
import sys, os, re, json, zipfile, collections
import xml.etree.ElementTree as ET

W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
W_STRICT = 'http://purl.oclc.org/ooxml/wordprocessingml/main'
W14 = 'http://schemas.microsoft.com/office/word/2010/wordml'
W15 = 'http://schemas.microsoft.com/office/word/2012/wordml'
MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006'
M = 'http://schemas.openxmlformats.org/officeDocument/2006/math'
WP = 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
C = 'http://schemas.openxmlformats.org/drawingml/2006/chart'
XML = 'http://www.w3.org/XML/1998/namespace'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
V = 'urn:schemas-microsoft-com:vml'


def w(l): return '{%s}%s' % (W, l)


RANGE_MARKERS = {w(x) for x in (
    'bookmarkStart', 'bookmarkEnd', 'moveFromRangeStart', 'moveFromRangeEnd',
    'moveToRangeStart', 'moveToRangeEnd', 'commentRangeStart', 'commentRangeEnd',
    'customXmlInsRangeStart', 'customXmlInsRangeEnd', 'customXmlDelRangeStart',
    'customXmlDelRangeEnd', 'customXmlMoveFromRangeStart', 'customXmlMoveFromRangeEnd',
    'customXmlMoveToRangeStart', 'customXmlMoveToRangeEnd', 'permStart', 'permEnd',
    'proofErr')}
TRACK = {w(x) for x in ('ins', 'del', 'moveFrom', 'moveTo')}
PROP_TRACK = {w(x) for x in ('rPrChange', 'pPrChange', 'sectPrChange', 'tblPrChange',
                             'trPrChange', 'tcPrChange', 'tblGridChange', 'numberingChange')}
STORY_RE = re.compile(r'word/(document|header\d*|footer\d*|footnotes|endnotes|comments|glossary/document)\.xml$')


def text_of_run(r):
    out = []
    for c in r:
        if c.tag == w('t'): out.append(c.text or '')
        elif c.tag == w('tab'): out.append('\t')
        elif c.tag in (w('br'), w('cr')): out.append('\n')
    return ''.join(out)


def survey(path):
    f = collections.Counter()  # feature -> count
    facts = {}
    try:
        z = zipfile.ZipFile(path)
        names = z.namelist()
    except Exception as e:
        return {'error': 'zip: %s' % e}
    facts['parts'] = sorted(n for n in names if n.startswith('word/') and not n.startswith('word/media/'))
    facts['media'] = sum(1 for n in names if n.startswith('word/media/'))
    ct = z.read('[Content_Types].xml').decode('utf-8', 'replace') if '[Content_Types].xml' in names else ''
    facts['main_ct'] = re.findall(r'PartName="/word/document\.xml" ContentType="([^"]+)"', ct)
    if 'word/document.xml' not in names:
        facts['note'] = 'no word/document.xml'
    bm_ids = collections.Counter(); bm_names = collections.Counter()
    para_ids = collections.Counter(); comment_ref_ids = set(); comment_def_ids = set()
    fn_ref = set(); fn_def = set(); en_ref = set(); en_def = set()
    multi_run_paras = 0; text_paras = 0; split_words = 0
    for pn in names:
        if not (STORY_RE.match(pn) or pn in ('word/styles.xml', 'word/numbering.xml', 'word/settings.xml',
                                               'word/commentsExtended.xml', 'word/commentsIds.xml',
                                               'word/commentsExtensible.xml', 'word/people.xml')):
            continue
        raw = z.read(pn)
        if b'<!DOCTYPE' in raw[:2000] or b'<!ENTITY' in raw[:5000]:
            f['doctype_or_entity'] += 1
            continue
        head = raw[:4000].decode('utf-8', 'replace')
        if W_STRICT in head: f['strict_ns'] += 1
        if 'mc:Ignorable' in head: f['mc_ignorable:' + pn.split('/')[-1]] += 1
        try:
            root = ET.fromstring(raw)
        except Exception as e:
            f['xml_error'] += 1
            continue
        story = STORY_RE.match(pn)
        parent = {}
        for el in root.iter():
            for c in el: parent[c] = el
        for el in root.iter():
            t = el.tag
            if not isinstance(t, str): continue
            ns, _, local = t[1:].partition('}')
            if pn == 'word/settings.xml':
                if t == w('updateFields'): f['settings:updateFields'] += 1
                if t == w('trackRevisions'): f['settings:trackRevisions'] += 1
                if t == w('rsids'): f['settings:rsids'] += 1; f['settings:rsid_count'] += len(list(el))
                if t == w('documentProtection'): f['settings:documentProtection'] += 1
                if t == w('evenAndOddHeaders'): f['settings:evenAndOddHeaders'] += 1
                if t == w('compat'): f['settings:compat'] += 1
                if t == '{%s}docId' % W14 or t == '{%s}docId' % W15: f['settings:docId'] += 1
                continue
            if pn == 'word/styles.xml':
                if t == w('style'):
                    f['styles:style'] += 1
                    if el.find(w('basedOn')) is not None: f['styles:basedOn'] += 1
                    if el.find(w('link')) is not None: f['styles:link'] += 1
                if t == w('docDefaults'): f['styles:docDefaults'] += 1
                if t == w('latentStyles'): f['styles:latentStyles'] += 1
                continue
            if pn == 'word/numbering.xml':
                if t == w('abstractNum'): f['numbering:abstractNum'] += 1
                if t == w('num'): f['numbering:num'] += 1
                if t == w('lvlOverride'): f['numbering:lvlOverride'] += 1
                if t == w('numStyleLink') or t == w('styleLink'): f['numbering:styleLink'] += 1
                continue
            if pn in ('word/commentsExtended.xml', 'word/commentsIds.xml', 'word/commentsExtensible.xml', 'word/people.xml'):
                f['part:' + pn.split('/')[-1]] += 1
                break
            # --- story parts
            if pn == 'word/comments.xml' and t == w('comment'):
                comment_def_ids.add(el.get(w('id')))
            if pn == 'word/footnotes.xml' and t == w('footnote') and el.get(w('type')) is None:
                fn_def.add(el.get(w('id')))
            if pn == 'word/endnotes.xml' and t == w('endnote') and el.get(w('type')) is None:
                en_def.add(el.get(w('id')))
            if t == w('p'):
                pid = el.get('{%s}paraId' % W14)
                if pid: para_ids[pid] += 1
                runs = [r for r in el if r.tag == w('r') and text_of_run(r)]
                if runs:
                    text_paras += 1
                    if len(runs) > 1:
                        multi_run_paras += 1
                        # a word split across run boundary: run ends and next starts with word chars
                        for a, b in zip(runs, runs[1:]):
                            ta, tb = text_of_run(a), text_of_run(b)
                            if ta and tb and ta[-1].isalnum() and tb[0].isalnum(): split_words += 1
                if el.find(w('pPr') + '/' + w('sectPr')) is not None: f['sectPr_in_pPr'] += 1
            if t == w('body'):
                kids = list(el)
                if kids and kids[-1].tag == w('sectPr'): f['body_sectPr_last'] += 1
                elif any(k.tag == w('sectPr') for k in kids): f['body_sectPr_not_last'] += 1
                else: f['body_no_sectPr'] += 1
                for k in kids:
                    if k.tag == '{%s}AlternateContent' % MC: f['AC_in_body'] += 1
                    if k.tag not in (w('p'), w('tbl'), w('sdt'), w('sectPr'), w('bookmarkStart'), w('bookmarkEnd'), w('customXml')) and k.tag not in RANGE_MARKERS and k.tag not in TRACK:
                        f['body_child:' + k.tag.split('}')[1]] += 1
            if t == w('t'):
                if el.get('{%s}space' % XML) == 'preserve': f['t_preserve'] += 1
                f['t'] += 1
                if el.text and el.text != el.text.strip() and el.get('{%s}space' % XML) != 'preserve':
                    f['t_ws_without_preserve'] += 1
            if t == w('fldSimple'):
                f['fldSimple'] += 1
                instr = (el.get(w('instr')) or '').strip().split(' ')[0].upper()
                f['field:' + instr] += 1
                if el.find('.//' + w('t')) is not None: f['fldSimple_cached_text'] += 1
            if t == w('fldChar'):
                f['fldChar:' + (el.get(w('fldCharType')) or '?')] += 1
                if el.get(w('dirty')) in ('1', 'true', 'on'): f['fldChar_dirty'] += 1
                if el.get(w('fldLock')) in ('1', 'true', 'on'): f['fldChar_locked'] += 1
            if t == w('instrText'):
                tok = (el.text or '').strip().split(' ')[0].upper()
                if tok: f['field:' + tok] += 1
            if t in TRACK: f['track:' + local] += 1
            if t in PROP_TRACK: f['track:' + local] += 1
            if t == w('delText'): f['delText'] += 1
            if t == w('ins') and parent.get(el) is not None and parent[el].tag == w('rPr'): f['track:paramark_ins'] += 1
            if t == w('sdt'):
                pt = parent[el].tag.split('}')[1] if el in parent else '?'
                f['sdt_in:' + pt] += 1
                pr = el.find(w('sdtPr'))
                if pr is not None:
                    for c in pr:
                        cl = c.tag.split('}')[1]
                        if cl in ('docPartObj', 'docPartList', 'date', 'dropDownList', 'comboBox', 'text', 'richText',
                                  'picture', 'group', 'citation', 'bibliography', 'equation'):
                            f['sdt_kind:' + cl] += 1
                        if c.tag == '{%s}checkbox' % W14: f['sdt_kind:w14checkbox'] += 1
                        if c.tag == w('dataBinding'): f['sdt_dataBinding'] += 1
            if t == w('customXml'): f['customXml_element'] += 1
            if t == w('smartTag'): f['smartTag'] += 1
            if t == w('bookmarkStart'):
                bm_ids[el.get(w('id'))] += 1; bm_names[el.get(w('name'))] += 1
                if el.get(w('colFirst')) is not None: f['bookmark_colFirst'] += 1
            if t == w('hyperlink'):
                f['hyperlink:' + ('external' if el.get('{%s}id' % R) else 'anchor' if el.get(w('anchor')) else 'other')] += 1
            if t == w('commentRangeStart'): f['commentRangeStart'] += 1
            if t == w('commentReference'): comment_ref_ids.add(el.get(w('id')))
            if t == w('footnoteReference'): fn_ref.add(el.get(w('id')))
            if t == w('endnoteReference'): en_ref.add(el.get(w('id')))
            if t == w('drawing'):
                for k in el:
                    f['drawing:' + k.tag.split('}')[1]] += 1
            if t == '{%s}graphicData' % A:
                f['graphicData:' + (el.get('uri') or '?').rsplit('/', 1)[-1]] += 1
            if t == '{%s}chart' % C: f['chart_ref'] += 1
            if t == w('pict'): f['vml_pict'] += 1
            if t == w('object'): f['ole_object'] += 1
            if t == '{%s}oMath' % M: f['omath'] += 1
            if t == '{%s}oMathPara' % M: f['omathPara'] += 1
            if t == w('altChunk'): f['altChunk'] += 1
            if t == w('subDoc'): f['subDoc'] += 1
            if t == '{%s}AlternateContent' % MC:
                pt = parent[el].tag.split('}')[1] if el in parent else '?'
                f['AC_in:' + pt] += 1
            if t == w('tbl'):
                f['tbl'] += 1
                kids = list(el)
                for k in kids:
                    if k.tag == w('tblPr'): break
                    if k.tag in RANGE_MARKERS: f['tbl_marker_before_tblPr'] += 1
                p = parent.get(el)
                if p is not None and p.tag == w('tc'): f['tbl_nested'] += 1
            if t == w('gridSpan'): f['tbl_gridSpan'] += 1
            if t == w('vMerge'): f['tbl_vMerge'] += 1
            if t == w('hMerge'): f['tbl_hMerge'] += 1
            if t == w('numPr'): f['numPr'] += 1
            if t == w('pStyle'): f['pStyle'] += 1
            if t == w('rStyle'): f['rStyle'] += 1
            if t == w('sectPr'): f['sectPr'] += 1
            if t in (w('headerReference'), w('footerReference')):
                f[local + ':' + (el.get(w('type')) or '?')] += 1
            if t == w('titlePg'): f['titlePg'] += 1
            if t == w('rPr'):
                for c in el:
                    if c.tag in (w('b'), w('i'), w('strike'), w('caps'), w('vanish')):
                        v = c.get(w('val'))
                        f['onoff:' + ('absent' if v is None else v)] += 1
            if t == w('lastRenderedPageBreak'): f['lastRenderedPageBreak'] += 1
            if t == w('br') and el.get(w('type')) == 'page': f['br_page'] += 1
            if t == w('sym'): f['sym'] += 1
            if t == w('ruby'): f['ruby'] += 1
            if t == w('permStart'): f['permStart'] += 1
            if t == w('dir') or t == w('bdo'): f['dir_bdo'] += 1
            if t == w('rsidR') or el.get(w('rsidR')) is not None: f['rsid_attr'] += 1
            if t == w('txbxContent'): f['txbxContent'] += 1
            if t == w('framePr'): f['framePr'] += 1
            if t == w('ffData'): f['legacy_formfield'] += 1
    facts['features'] = dict(sorted(f.items()))
    facts['bookmark_dup_ids'] = sorted(k for k, v in bm_ids.items() if v > 1 and k is not None)
    facts['bookmark_dup_names'] = sorted(k for k, v in bm_names.items() if v > 1 and k is not None)
    facts['bookmark_count'] = sum(bm_ids.values())
    facts['paraId_count'] = sum(para_ids.values())
    facts['paraId_dups'] = sum(1 for v in para_ids.values() if v > 1)
    facts['comments'] = {'refs': len(comment_ref_ids), 'defs': len(comment_def_ids),
                         'ref_without_def': len(comment_ref_ids - comment_def_ids)}
    facts['footnotes'] = {'refs': len(fn_ref), 'defs': len(fn_def), 'ref_without_def': len(fn_ref - fn_def)}
    facts['endnotes'] = {'refs': len(en_ref), 'defs': len(en_def), 'ref_without_def': len(en_ref - en_def)}
    facts['paras'] = {'with_text': text_paras, 'multi_run': multi_run_paras, 'word_split_at_run_boundary': split_words}
    return facts


def main():
    d = sys.argv[1]
    out = {}
    for fn in sorted(os.listdir(d)):
        if not fn.lower().endswith(('.docx', '.docm', '.dotx', '.dotm')): continue
        out[fn] = survey(os.path.join(d, fn))
    if '--json' in sys.argv:
        json.dump(out, open(sys.argv[sys.argv.index('--json') + 1], 'w'), indent=1, sort_keys=True)
    # corpus-wide summary
    tot = collections.Counter(); files_with = collections.Counter()
    agg = collections.Counter()
    for fn, r in out.items():
        if 'error' in r: tot['__zip_error'] += 1; continue
        for k, v in r.get('features', {}).items():
            tot[k] += v; files_with[k] += 1
        agg['paras_with_text'] += r['paras']['with_text']; agg['multi_run'] += r['paras']['multi_run']
        agg['word_split'] += r['paras']['word_split_at_run_boundary']
        agg['files_bm_dup_id'] += bool(r['bookmark_dup_ids']); agg['files_bm_dup_name'] += bool(r['bookmark_dup_names'])
        agg['paraId'] += r['paraId_count']; agg['files_paraId_dup'] += bool(r['paraId_dups'])
        agg['files_comment_ref_without_def'] += bool(r['comments']['ref_without_def'])
    print('files: %d' % len(out))
    for k, v in sorted(agg.items()): print('  %-40s %d' % (k, v))
    print('%-45s %8s %6s' % ('feature', 'count', 'files'))
    for k in sorted(tot): print('%-45s %8d %6d' % (k, tot[k], files_with[k]))


if __name__ == '__main__':
    main()
