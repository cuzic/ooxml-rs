import sys, json, os, collections, importlib.util
sys.argv=[sys.argv[0]]+sys.argv[1:]
sdk, xsd = sys.argv[1], sys.argv[2]
src=open(os.path.join(os.path.dirname(os.path.abspath(sys.argv[3])),'slottable.py')).read().split("pfx = {e['Prefix']")[0]
exec(src)
def group(f):
    if 'spreadsheetml_2006_main' in f: return 'SML core'
    if 'wordprocessingml_2006_main' in f: return 'WML core'
    if 'presentationml_2006_main' in f: return 'PML core'
    if 'drawingml_2006_main' in f: return 'DML core'
    if 'spreadsheetml' in f or 'excel' in f: return 'SML ext'
    if 'office_word' in f or 'wordprocessing' in f: return 'WML ext'
    if 'powerpoint' in f: return 'PML ext'
    if 'chart' in f: return 'Chart'
    if 'diagram' in f: return 'SmartArt'
    if 'math' in f: return 'Math'
    if 'drawing' in f: return 'DML ext/other'
    return 'Other'
stat=collections.defaultdict(lambda: collections.Counter()); uns=collections.defaultdict(list)
for f in sorted(os.listdir(os.path.join(sdk,'schemas'))):
    g=group(f)
    for t in json.load(open(os.path.join(sdk,'schemas',f))).get('Types',[]):
        P=t.get('Particle')
        if not P: continue
        stat[g]['total']+=1
        try:
            sl=lower(P); names=[q for s in sl for q in s if q!='*ANY*']
            
            stat[g]['single' if all(len(s)==1 for s in sl) else 'multi']+=1
        except NS as e:
            stat[g]['unsupported']+=1; uns[g].append((t['Name'],str(e)))
for g in ['SML core','SML ext','WML core','WML ext','PML core','PML ext','DML core','Chart','SmartArt','Math','DML ext/other','Other']:
    c=stat[g]; print(f"{g:14} total {c['total']:4}  single {c['single']:4}  multi {c['multi']:4}  unsupported {c['unsupported']:3}")
for g in ['WML core','WML ext','PML core','PML ext','DML core','Math']:
    print(g, uns[g][:40])
