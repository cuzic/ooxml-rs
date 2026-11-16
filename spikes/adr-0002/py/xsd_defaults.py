import sys, json, os, xml.etree.ElementTree as ET, collections
XS='{http://www.w3.org/2001/XMLSchema}'
xsd_dir, sdk_dir = sys.argv[1], sys.argv[2]
# collect: (targetNS, complexType name) -> {attr local/qname: default}
defaults={}; agroups={}; ctypes={}
for f in os.listdir(xsd_dir):
    if not f.endswith('.xsd'): continue
    root=ET.parse(os.path.join(xsd_dir,f)).getroot(); tns=root.get('targetNamespace')
    for ag in root.findall(XS+'attributeGroup'): agroups[(tns,ag.get('name'))]=ag
    for ct in root.findall(XS+'complexType'): ctypes[(tns,ct.get('name'))]=(ct,root)
def resolve_q(qn, root):
    if ':' in qn:
        p,l=qn.split(':'); 
        # find ns by prefix on root attributes is not available in ET; use known map below
        return p,l
    return None,qn
def attrs_of(ct, tns, seen=None):
    out={}
    for a in ct.iter(XS+'attribute'):
        if a.get('default') is not None:
            name=a.get('name') or a.get('ref')
            out[name]=a.get('default')
    for ref in ct.iter(XS+'attributeGroup'):
        r=ref.get('ref')
        if r:
            l=r.split(':')[-1]
            for (ns,n),ag in agroups.items():
                if n==l:
                    for a in ag.iter(XS+'attribute'):
                        if a.get('default') is not None: out[a.get('name') or a.get('ref')]=a.get('default')
    return out
total_xsd=0; per_ct={}
for (tns,n),(ct,root) in ctypes.items():
    d=attrs_of(ct,tns)
    if d: per_ct[(tns,n)]=d; total_xsd+=len(d)
# SDK side
ns=json.load(open(os.path.join(sdk_dir,'namespaces.json')))
pfx={e['Prefix']:e['Uri'] for e in ns} if isinstance(ns,list) else None
matched=0; sdk_attrs_with_xsd_ct=0; examples=[]
for f in os.listdir(os.path.join(sdk_dir,'schemas')):
    j=json.load(open(os.path.join(sdk_dir,'schemas',f)))
    for t in j.get('Types',[]):
        typ=t['Name'].split('/')[0]
        if ':' not in typ: continue
        p,ctn=typ.split(':',1)
        key=(pfx.get(p) if pfx else None, ctn)
        if key not in ctypes: continue
        d=per_ct.get(key,{})
        for a in t.get('Attributes',[]):
            q=a['QName']; local=q.split(':')[-1] if q.startswith(':') else q
            sdk_attrs_with_xsd_ct+=1
            if local in d or q in d:
                matched+=1
                if typ=='x:CT_Cell': examples.append((t['Name'],q,d.get(local)))
print('xsd complexTypes', len(ctypes), 'xsd defaults', total_xsd)
print('sdk attrs whose type exists in ECMA xsd', sdk_attrs_with_xsd_ct, 'matched to a default', matched)
print('CT_Cell', examples)
