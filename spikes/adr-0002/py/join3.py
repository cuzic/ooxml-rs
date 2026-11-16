import sys, json, os, collections, xml.etree.ElementTree as ET
XS='{http://www.w3.org/2001/XMLSchema}'
xsd_dir, sdk_dir = sys.argv[1], sys.argv[2]
def nsmap(path):
    m={}
    for ev,(p,u) in ET.iterparse(path, events=['start-ns']): m[p]=u
    return m
cts={}; ags={}; gattrs={}
for f in sorted(os.listdir(xsd_dir)):
    if not f.endswith('.xsd'): continue
    p=os.path.join(xsd_dir,f); root=ET.parse(p).getroot(); tns=root.get('targetNamespace'); nm=nsmap(p); nm['__q']=root.get('attributeFormDefault')=='qualified'; nm['__t']=tns
    for ag in root.findall(XS+'attributeGroup'): ags[(tns,ag.get('name'))]=(ag,nm,tns)
    for a in root.findall(XS+'attribute'): gattrs[(tns,a.get('name'))]=a.get('default')
    for ct in root.findall(XS+'complexType'): cts[(tns,ct.get('name'))]=(ct,nm,tns,f)
def res(q,nm,tns):
    p,_,l=q.rpartition(':'); return ((nm.get(p) or ('http://www.w3.org/XML/1998/namespace' if p=='xml' else 'UNBOUND:'+p)) if p else nm.get('',tns), l)
def direct(node,nm,tns,how):
    out={}
    for a in node.iter(XS+'attribute'):
        if a.get('name'):
            key=((nm['__t'] if nm.get('__q') or a.get('form')=='qualified' else ''),a.get('name'))
            if a.get('default') is not None: out[key]=(a.get('default'),how)
        elif a.get('ref'):
            key=res(a.get('ref'),nm,tns)
            if a.get('default') is not None: out[key]=(a.get('default'),how+'+ref-use')
            elif gattrs.get(key) is not None: out[key]=(gattrs[key],how+'+ref-global')
    for g in node.iter(XS+'attributeGroup'):
        if g.get('ref'):
            k=res(g.get('ref'),nm,tns); ag,anm,atns=ags[k]
            for kk,v in direct(ag,anm,atns,'agroup').items(): out.setdefault(kk,v)
    return out
xdef={}
for (tns,n),(ct,nm,t,f) in cts.items():
    d=direct(ct,nm,tns,'ct')
    for e in ct.iter():
        if e.tag==XS+'extension' and e.get('base'):
            k=res(e.get('base'),nm,tns)
            if k in cts:
                b=cts[k]; 
                for kk,v in direct(b[0],b[1],b[2],'ext').items(): d.setdefault(kk,v)
    for kk,v in d.items(): xdef[(tns,n,kk)]=v
how=collections.Counter(v[1] for v in xdef.values())
print('xsd (ct,attr) defaults incl ext/agroup/ref:',len(xdef), dict(how))
pfx={e['Prefix']:e['Uri'] for e in json.load(open(os.path.join(sdk_dir,'namespaces.json')))}
hit=set(); n_sdk=0; missing_ct=collections.Counter(); joined=0; joined_how=collections.Counter()
xsd_ns={k[0] for k in cts}
for f in sorted(os.listdir(os.path.join(sdk_dir,'schemas'))):
    j=json.load(open(os.path.join(sdk_dir,'schemas',f)))
    for t in j.get('Types',[]):
        typ=t['Name'].split('/')[0]
        if ':' not in typ: continue
        p,ctn=typ.split(':',1); ns=pfx.get(p)
        for a in t.get('Attributes',[]):
            n_sdk+=1
            if ns not in xsd_ns: continue
            if (ns,ctn) not in cts: missing_ct[(p,ctn)]+=1; continue
            ap,_,al=a['QName'].rpartition(':'); key=(pfx.get(ap,'') if ap else '', al)
            v=xdef.get((ns,ctn,key))
            if v: joined+=1; hit.add((ns,ctn,key)); joined_how[v[1]]+=1
print('sdk attrs',n_sdk,'joined (ns-correct)',joined, dict(joined_how))
print('xsd defaults never reached by any sdk attr:', len(set(xdef)-hit))
miss=sorted(set(xdef)-hit)
byct=collections.Counter((k[0].split('/')[-2] if '/' in k[0] else k[0], k[1]) for k in miss)
print(' top cts:', byct.most_common(25))
print('sdk attrs on types absent from ECMA XSD (ECMA ns):', sum(missing_ct.values()), 'types', len(missing_ct)); print('  e.g.', missing_ct.most_common(12))
print('MISSING detail:')
for k in miss[:60]: print('  ',k[0].rsplit('/',2)[-2:],k[1],k[2],xdef[k][0])
