import sys, json, os, re, collections
exec(open(os.path.join(os.path.dirname(sys.argv[0]),'join2.py')).read().split("print('xsd (ct,attr)")[0])
# fix wml qualified: rebuild xdef with qualified-form awareness
pfx={e['Prefix']:e['Uri'] for e in json.load(open(os.path.join(sdk_dir,'namespaces.json')))}
enums=collections.defaultdict(set)
for f in os.listdir(os.path.join(sdk_dir,'schemas')):
    for e in json.load(open(os.path.join(sdk_dir,'schemas',f))).get('Enums',[]):
        enums[e['Name']] |= {x['Value'] for x in e['Facets']}
types=collections.Counter(); bad=[]
xsd_ns={k[0] for k in cts}
for f in sorted(os.listdir(os.path.join(sdk_dir,'schemas'))):
    for t in json.load(open(os.path.join(sdk_dir,'schemas',f))).get('Types',[]):
        typ=t['Name'].split('/')[0]
        if ':' not in typ: continue
        p,ctn=typ.split(':',1); ns=pfx.get(p)
        if (ns,ctn) not in cts: continue
        for a in t.get('Attributes',[]):
            ap,_,al=a['QName'].rpartition(':'); key=(pfx.get(ap,'') if ap else '', al)
            v=xdef.get((ns,ctn,key)) or xdef.get((ns,ctn,('',al)))
            if not v: continue
            d=v[0]; ty=a['Type']; types[ty.split('<')[0]]+=1
            m=re.match(r'EnumValue<.*\.(\w+)>',ty)
            ok=True
            if m: ok = d in enums.get(m.group(1),set())
            elif ty in('UInt32Value','UInt16Value','ByteValue','UInt64Value'): ok=d.isdigit()
            elif ty in('Int32Value','Int16Value','Int64Value','SByteValue'): ok=bool(re.fullmatch(r'-?\d+',d))
            elif ty=='BooleanValue': ok=d in('true','false','0','1')
            elif ty=='OnOffValue': ok=d in('true','false','0','1','on','off')
            elif ty=='TrueFalseValue': ok=d in('t','f','true','false')
            elif ty=='DoubleValue': ok=bool(re.fullmatch(r'-?(\d+(\.\d*)?|\.\d+)([eE][-+]?\d+)?|INF|-INF|NaN',d))
            if not ok: bad.append((t['Name'],a['QName'],ty,d))
print(types.most_common())
print('defaults not valid for SDK type:',len(bad))
for b in bad: print('  ',b)
