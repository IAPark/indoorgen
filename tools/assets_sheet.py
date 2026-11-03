# assets_sheet.py OUT preview.sh: two views of each asset in OUT/a.glb (from OUT/a.cams.json), labelled, into OUT/sheet*.jpg
import json,math,os,subprocess,sys,glob
from PIL import Image,ImageDraw
out,pv=sys.argv[1:3]; A=json.load(open(out+'/a.cams.json'))
close=float(os.environ.get('CLOSE','1')); W,H=map(int,os.environ.get('RES','480x270').split('x'))
views=[(-0.7,0.6),(2.3,0.35)]
args=[]
for a in A:
    d=max(a['r']*2.1,0.25)/close
    for az,el in views:
        e=[a['c'][0]+d*math.cos(az)*math.cos(el),a['c'][1]+d*math.sin(az)*math.cos(el),a['c'][2]+d*math.sin(el)]
        args+=['--at',','.join('%.3f'%v for v in e+a['c']+[28])]
r=out+'/r'; os.makedirs(r,exist_ok=True)
for f in glob.glob(r+'/*.png'): os.remove(f)
subprocess.run([pv,out+'/a.glb']+args+['--res',f'{W}x{H}','--ss','2','-o',r],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True)
for f in glob.glob(out+'/sheet*.jpg'): os.remove(f)
C=4; rows=5; per=C*rows; cells=[]
for i,a in enumerate(A):
    for v in range(2): cells.append((a,glob.glob(f'{r}/*_at{i*2+v}.png')))
for s in range(0,len(cells),per):
    p=cells[s:s+per]; im=Image.new('RGB',(W*C,H*((len(p)+C-1)//C))); d=ImageDraw.Draw(im)
    for i,(a,f) in enumerate(p):
        x,y=(i%C)*W,(i//C)*H
        if f: im.paste(Image.open(f[0]).convert('RGB'),(x,y))
        d.text((x+4,y+3),f"{a['class']} {a['seed']} {a['ms']:.0f}ms",fill=(255,255,0))
    im.save(f'{out}/sheet{s//per:02d}.jpg',quality=86)
print(len(A),'assets',(len(cells)+per-1)//per,'sheets in',out)
