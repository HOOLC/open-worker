"""Script-free SVG motion assets, including a real equal-topology path morph."""
from pathlib import Path
import json,math,re,xml.etree.ElementTree as ET
import resvg_py
from wordmarks import OPTIONS,svg
R=Path(__file__).resolve().parents[1]
brand=ET.parse(R/'assets/brand/mark.svg').getroot()
D=next(e.get('d') for e in brand.iter() if e.tag.endswith('path'))
letters=OPTIONS['fold']['letters'];INK='#24272B'
def shell(body,w=400,h=160,title='Zork 动效'):
 return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img"><title>{title}</title>{body}</svg>'
def anim(kind,values,dur,begin='0s'):
 return f'<animateTransform attributeName="transform" type="{kind}" values="{values}" dur="{dur}s" begin="{begin}" fill="freeze" calcMode="spline" keyTimes="0;.35;.65;1" keySplines=".2 .7 .2 1;.2 .7 .2 1;.2 .7 .2 1"/>'
icon=f'<g transform="translate(136 16)"><g>{anim("rotate","0 64 64;-7 64 64;3 64 64;0 64 64",.68)}<path d="{D}" fill="{INK}" fill-rule="evenodd"/></g></g>'
(R/'motion/icon.svg').write_text(shell(icon,title='Zork · 图标独立动效'))
word='<g transform="translate(44 30) scale(2.3)" fill="'+INK+'">'
for i,(letter,d) in enumerate(letters):word+=f'<g>{anim("translate","0 0;0 -2.5;0 .5;0 0",.48,f"{i*.065}s")}<path d="{d}" fill-rule="evenodd"/></g>'
word+='</g>'
(R/'motion/wordmark.svg').write_text(shell(word,title='Zork · 字标独立动效'))
linked=f'<g transform="translate(13 16)"><g>{anim("translate","0 0;11 0;2 0;0 0",.7)}<path d="{D}" fill="{INK}" fill-rule="evenodd"/></g></g><g transform="translate(158 37) scale(1.85)" fill="{INK}">'
for i,(_,d) in enumerate(letters):linked+=f'<g>{anim("translate","0 0;2 -1.8;-.5 0;0 0",.5,f"{.14+i*.055}s")}<path d="{d}" fill-rule="evenodd"/></g>'
linked+='</g>'
(R/'motion/linked.svg').write_text(shell(linked,w=440,title='Zork · 图标与字标联动'))

def flatten(d):
 tokens=re.findall(r'[MLHVQCZ]|[-+]?(?:\d*\.)?\d+',d);i=0;command='';p=(0.,0.);points=[];first=None
 while i<len(tokens):
  if tokens[i].isalpha():command=tokens[i];i+=1
  if command=='Z':
   if first:points.append(first)
   continue
  n={'M':2,'L':2,'H':1,'V':1,'Q':4,'C':6}[command];v=list(map(float,tokens[i:i+n]));i+=n
  if command in ['M','L']:p=tuple(v);points.append(p);first=first or p
  elif command=='H':p=(v[0],p[1]);points.append(p)
  elif command=='V':p=(p[0],v[0]);points.append(p)
  elif command=='Q':
   a=p;b=(v[0],v[1]);c=(v[2],v[3])
   for step in range(1,13):
    t=step/12;points.append(tuple((1-t)**2*a[k]+2*(1-t)*t*b[k]+t*t*c[k] for k in [0,1]))
   p=c
  else:
   a=p;b=tuple(v[:2]);c=tuple(v[2:4]);e=tuple(v[4:])
   for step in range(1,13):
    t=step/12;points.append(tuple((1-t)**3*a[k]+3*(1-t)**2*t*b[k]+3*(1-t)*t*t*c[k]+t**3*e[k] for k in [0,1]))
   p=e
 return points

def sample(points,n=128,closed=True):
 if closed and points[-1]!=points[0]:points=points+[points[0]]
 lengths=[0.]
 for a,b in zip(points,points[1:]):lengths.append(lengths[-1]+math.dist(a,b))
 out=[];j=0
 for i in range(n):
  d=i*lengths[-1]/n
  while j<len(lengths)-2 and lengths[j+1]<d:j+=1
  t=(d-lengths[j])/(lengths[j+1]-lengths[j] or 1);out.append(tuple(points[j][k]*(1-t)+points[j+1][k]*t for k in [0,1]))
 return out

def area(p):return sum(a[0]*b[1]-b[0]*a[1] for a,b in zip(p,p[1:]+p[:1]))
def normalized(p):
 x=sum(v[0] for v in p)/len(p);y=sum(v[1] for v in p)/len(p);s=max(max(v[0] for v in p)-min(v[0] for v in p),max(v[1] for v in p)-min(v[1] for v in p),1);return [((a-x)/s,(b-y)/s) for a,b in p]
def align(a,b):
 if area(a)*area(b)<0:a=list(reversed(a))
 an,bn=normalized(a),normalized(b);n=len(a);shift=min(range(n),key=lambda j:sum((an[(i+j)%n][0]-bn[i][0])**2+(an[(i+j)%n][1]-bn[i][1])**2 for i in range(n)))
 return a[shift:]+a[:shift]
def transform(p,x,y,s):return [(x+a*s,y+b*s) for a,b in p]
def data(p):return 'M'+' L'.join(f'{x:.3f} {y:.3f}' for x,y in p)+' Z'
def morph_path(a,b,fill,dur=1.2):
 return f'<path fill="{fill}" d="{data(a)}"><animate attributeName="d" values="{data(a)};{data(b)}" dur="{dur}s" fill="freeze" calcMode="spline" keyTimes="0;1" keySplines=".25 .1 .2 1"/></path>'
outer=flatten(D.split('Z')[0]+'Z')
def clip(poly,axis,value,greater):
 out=[]
 for a,b in zip(poly,poly[1:]+poly[:1]):
  ia=a[axis]>=value if greater else a[axis]<=value
  ib=b[axis]>=value if greater else b[axis]<=value
  if ia:out.append(a)
  if ia!=ib:
   t=(value-a[axis])/(b[axis]-a[axis]);out.append(tuple(a[k]+(b[k]-a[k])*t for k in [0,1]))
 return out

def landmark_samples(poly,landmarks,counts):
 if poly[0]==poly[-1]:poly=poly[:-1]
 indices=[min(range(len(poly)),key=lambda i:math.dist(poly[i],point)) for point in landmarks]
 assert all(math.dist(poly[i],point)<1e-6 for i,point in zip(indices,landmarks)), 'missing notch landmark'
 result=[]
 for j,start in enumerate(indices):
  end=indices[(j+1)%len(indices)]
  arc=poly[start:end+1] if end>start else poly[start:]+poly[:end+1]
  result.extend(sample(arc,counts[j],False))
 return result

def eased(x):
 x=max(0,min(1,x));return x*x*(3-2*x)
def lerp(a,b,t):return [(u[0]*(1-t)+v[0]*t,u[1]*(1-t)+v[1]*t) for u,v in zip(a,b)]
def pill(cx,cy):return sample(flatten(f'M{cx-6} {cy-4}H{cx+6}Q{cx+10} {cy-4} {cx+10} {cy}Q{cx+10} {cy+4} {cx+6} {cy+4}H{cx-6}Q{cx-10} {cy+4} {cx-10} {cy}Q{cx-10} {cy-4} {cx-6} {cy-4}Z'))
origin=(80.,14.)
upper=clip(outer,1,74,False)
sources=[clip(outer,1,74,True),clip(upper,0,64,False),clip(clip(upper,0,64,True),0,85,False),clip(upper,0,85,True)]
targets=[transform(flatten(d.split('Z')[0]+'Z'),44,30,2.3) for _,d in letters]
icon_notch=transform([(65,111),(72,96),(41,110)],*origin,1)
z_notch=transform([(18,40),(21,35),(10,40)],44,30,2.3)
contours=[]
for i,((letter,_),source,target) in enumerate(zip(letters,sources,targets)):
 source=transform(source,*origin,1)
 if area(source)*area(target)<0:source=list(reversed(source))
 if i==0:
  source=landmark_samples(source,icon_notch,[12,24,92]);target=landmark_samples(target,z_notch,[12,24,92])
 else:
  target=sample(target);source=align(sample(source),target)
 contours.append({'id':letter,'from':source,'to':target})
ocounter=transform(sample(flatten(letters[1][1].split('Z')[1]+'Z')),44,30,2.3)
eyes=[transform(pill(cx,65),*origin,1) for cx in [43,84]]
eyes[0]=align(eyes[0],ocounter)
DURATION=2000
# Spring anticipation, an asymmetric arc and a soft landing make a single hop.
def spring(q):
 q=max(0,min(1,q));return 1-math.exp(-6*q)*math.cos(9*q)
def hop(ms):
 if ms<240:
  q=eased(ms/240);return 0,0,1+.075*q,1-.18*q,0
 if ms<740:
  q=(ms-240)/500;release=max(0,1-q*5)
  stretch=math.sin(math.pi*min(1,q*3))*max(0,1-q*2)
  return 110*(1-(1-q)**1.35),28*q-132*q*(1-q),1+.075*release-.045*stretch,1-.18*release+.13*stretch,8*math.sin(math.pi*q)
 if ms<990:
  q=(ms-740)/250;impact=math.sin(math.pi*min(1,q*2))*max(0,1-q*1.2)
  recoil=math.sin(math.pi*max(0,(q-.35)/.65))*max(0,1-q)
  return 110+3*math.sin(math.pi*q)*(1-q),28-5*recoil,1+.14*impact,1-.25*impact,3*math.sin(math.pi*q)*(1-q)
 return 110,28,1,1,0

def upper_at(points,ms):
 dx,dy,sx,sy,angle=hop(ms);angle=math.radians(angle);ca,sa=math.cos(angle),math.sin(angle)
 return [(144+(x-144)*sx*ca-(y-88)*sy*sa+dx,88+(x-144)*sx*sa+(y-88)*sy*ca+dy) for x,y in points]
def rounded(points,amount):
 n=len(points);return [tuple(p[k]*(1-amount)+sum(points[(i+j)%n][k] for j in range(-3,4))/7*amount for k in [0,1]) for i,p in enumerate(points)]
def z_at(ms):
 c=contours[0];q=max(0,min(1,(ms-820)/880));p=eased(q)
 result=[]
 for a,b in zip(c['from'],c['to']):
  x=a[0]*(1-p)+b[0]*p-9*math.sin(math.pi*q)
  bulge=math.sin(math.pi*q)*math.sin((a[0]-92)/104*math.pi*1.7-q*math.pi*2)*5
  result.append((x,a[1]*(1-p)+b[1]*p+bulge))
 return rounded(result,math.sin(math.pi*q)*.8)
def letter_at(index,ms):
 c=contours[index];q=max(0,min(1,(ms-(1010+(index-1)*55))/(650+(index-1)*40)))
 p=eased(q);points=lerp(upper_at(c['from'],ms),c['to'],p)
 # Each piece briefly passes its resting spot, then softly catches up.
 overshoot=math.sin(math.pi*max(0,(q-.58)/.42))*max(0,(q-.58)/.42)*2
 dx=[0,-6,3,7][index]*overshoot;dy=-[0,3,4,5][index]*overshoot
 bend=math.sin(math.pi*q)*(-3 if index==1 else 3)
 cx=sum(x for x,y in points)/128;cy=122
 theta=math.radians(bend);ca,sa=math.cos(theta),math.sin(theta)
 points=[(cx+(x-cx)*ca-(y-cy)*sa+dx,cy+(x-cx)*sa+(y-cy)*ca+dy) for x,y in points]
 softness=math.sin(math.pi*q)*.9
 smooth=rounded(points,max(softness,eased((ms-250)/120)*(1-p)*.9))
 points=[smooth[j] if q>0 or (c['from'][j][1]>79 and (c['from'][j][0]<102 or c['from'][j][0]>185)) else pt for j,pt in enumerate(points)]
 return points,q,(dx,dy,theta,cx,cy)
def frame(ms):
 items=[{'id':'z','fill':'ink','points':z_at(ms)}]
 details=[]
 for i in [1,2,3]:
  points,q,detail=letter_at(i,ms);details.append((q,detail));items.append({'id':contours[i]['id'],'fill':'ink','points':points})
 q,detail=details[0];dx,dy,theta,cx,cy=detail;p=eased(q)
 left=lerp(upper_at(eyes[0],ms),ocounter,p);ca,sa=math.cos(theta),math.sin(theta)
 left=[(cx+(x-cx)*ca-(y-cy)*sa+dx,cy+(x-cx)*sa+(y-cy)*ca+dy) for x,y in left]
 # A quick soft blink during the crouch; left eye subsequently opens into o.
 blink=1-.7*math.sin(math.pi*max(0,min(1,(ms-90)/140)))
 if ms<240:
  cyeye=sum(y for x,y in left)/128;left=[(x,cyeye+(y-cyeye)*blink) for x,y in left]
 items.append({'id':'eye-left','fill':'counter','points':left})
 right=upper_at(eyes[1],ms);center=upper_at([(164,79)],ms)*128
 right=lerp(right,center,eased((ms-950)/190))
 if ms<240:
  cyeye=sum(y for x,y in right)/128;right=[(x,cyeye+(y-cyeye)*blink) for x,y in right]
 items.append({'id':'eye-right','fill':'counter','points':right})
 # Centre the complete visible silhouette through every phase, not just its box.
 ink=[p for c in items[:4] for p in c['points']];shift=200-(min(x for x,y in ink)+max(x for x,y in ink))/2
 for item in items:item['points']=[[round(x+shift,4),round(y-2,4)] for x,y in item['points']]
 return {'time_ms':ms,'contours':items}
frames=[frame(ms) for ms in sorted(set(list(range(0,DURATION+1,25))+[240,250,485,740,820,990,1010,1030,1090,1390,1490,1700,DURATION]))]
notch={'meaning':'bottom mask contour crawls left into Z; notch anchors stay on the same contour','vertex_indices':[0,12,36],'icon':[frames[0]['contours'][0]['points'][i] for i in [0,12,36]],'wordmark':[frames[-1]['contours'][0]['points'][i] for i in [0,12,36]],'frames':[{'time_ms':f['time_ms'],'points':[f['contours'][0]['points'][i] for i in [0,12,36]]} for f in frames]}
geometry={'mode':'rounded-hop-unfold','viewBox':[0,0,400,160],'durationMs':DURATION,'topology':'4 rounded body contours + 2 counters; 128 corresponding vertices each','frames':frames,'notch':notch,'choreography':{'anticipate':[0,240],'upper_hop':[240,740],'land':[740,990],'lower_crawl_to_z':[820,1700],'upper_separate_to_ork':[1010,1850],'settle':[1850,2000]},'centering':'visible body bounds centered on x=200 throughout'}
(R/'motion/morph-points.json').write_text(json.dumps(geometry,separators=(',',':'))+'\n')
def animated_path(index,fill):
 values=';'.join(data(f['contours'][index]['points']) for f in frames)
 times=';'.join(f'{f["time_ms"]/DURATION:.8f}' for f in frames)
 stroke=f' stroke="{fill}" stroke-width=".3" stroke-linejoin="round"' if fill==INK else ''
 return f'<path fill="{fill}"{stroke} d="{data(frames[0]["contours"][index]["points"])}"><animate attributeName="d" values="{values}" keyTimes="{times}" dur="2s" fill="freeze" calcMode="linear"/></path>'
def solid_shape(f):return ''.join(data(c['points']) for c in f['contours'][:4])
values=';'.join(solid_shape(f) for f in frames)
times=';'.join(f'{f["time_ms"]/DURATION:.8f}' for f in frames)
solid=f'<path fill="{INK}" d="{solid_shape(frames[0])}"><animate attributeName="d" values="{values}" keyTimes="{times}" dur="2s" fill="freeze" calcMode="linear"/></path>'
body='<defs><mask id="zork-morph-counters"><rect width="400" height="160" fill="white"/>'+''.join(animated_path(i,'black') for i in [4,5])+'</mask></defs><g mask="url(#zork-morph-counters)">'+solid+'</g>'
(R/'motion/icon-to-wordmark.svg').write_text(shell(body,title='Zork · 上半跳落，下半蠕动成 Z，再分开为 ork'))
(R/'motion/spec.json').write_text(json.dumps({'icon':{'durationMs':680,'file':'icon.svg'},'wordmark':{'durationMs':675,'file':'wordmark.svg'},'linked':{'durationMs':805,'file':'linked.svg'},'morph':{'durationMs':DURATION,'file':'icon-to-wordmark.svg','geometry':'morph-points.json','choreography':geometry['choreography'],'correspondence':'bottom -> Z; upper left -> o; upper right -> rk; upper group jumps before separation'},'loop':False,'reducedMotion':'static endpoints; no autoplay','status':'shared SVG and native GPUI sampled geometry'},indent=2)+'\n')
parts=['<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="690"><rect width="1200" height="690" fill="#F6F3EA"/>']
for j,(ms,label) in enumerate([(0,'0 · Together'),(220,'220 · Crouch'),(325,'325 · Takeoff'),(520,'520 · Airborne'),(800,'800 · Soft landing'),(1160,'1160 · Unfold'),(1380,'1380 · Unfold'),(1680,'1680 · Settle'),(2000,'2000 · Zork')]):
 x=j%3*400;y=j//3*230;f=frame(ms);maskid=f'm{j}'
 parts.append(f'<g transform="translate({x} {y})"><defs><mask id="{maskid}"><rect width="400" height="160" fill="white"/>')
 for c in f['contours'][4:]:parts.append(f'<path d="{data(c["points"])}" fill="black"/>')
 parts.append(f'</mask></defs><g mask="url(#{maskid})">')
 parts.append(f'<path d="{solid_shape(f)}" fill="{INK}"/>')
 parts.append(f'</g><text x="35" y="198" font-family="Arial" font-size="14" fill="#62656B">{label}</text></g>')
parts.append('</svg>');(R/'motion/morph-proof.png').write_bytes(resvg_py.svg_to_bytes(svg_string=''.join(parts)))
print('Upper hop / lower crawl: SVG, shared frames, and nine-phase proof ready.')
