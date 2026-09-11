"""Transpose dense weights for adjacent GPU lanes; preserve every f32 bit."""
from pathlib import Path
import struct
root=Path(__file__).resolve().parent
s=(root/'assets/tiny.wgsl').read_text()
def replace(a,b):
    global s
    assert s.count(a)==1,(a,s.count(a))
    s=s.replace(a,b)
for a,b in [
 ('37129u+Ba*32u+Z','37129u+Z*32u+Ba'),
 ('38185u+Ba*32u+Z','38185u+Z*32u+Ba'),
 ('let Tc=39241u+k*64u;','let Tc=39241u+k;'),
 ('i[Tc+la]','i[Tc+la*32u]'),('i[Tc+32u+la]','i[Tc+(32u+la)*32u]'),
 ('let yd=35865u+ra*64u;','let yd=35865u+ra;'),
 ('i[yd+U]','i[yd+U*16u]'),('i[yd+32u+U]','i[yd+(32u+U)*16u]'),
 ('let zd=29640u+V*80u;','let zd=29640u+V;'),
 ('i[zd+W]','i[zd+W*72u]'),('i[zd+32u+W]','i[zd+(32u+W)*72u]'),
 ('let ue=29640u+V*80u+64u;','let ue=29640u+V+64u*72u;'),
 ('i[ue+cb]','i[ue+cb*72u]'),
 ('25920u+sa*72u+db','25920u+db*9u+sa'),
 ('25920u+label*72u+hidden','25920u+hidden*9u+label'),
]:replace(a,b)
# Tiny kernel's clearly delimited dense layers.
def region(start,end,pairs):
    global s
    a=s.index(start);b=s.index(end,a);part=s[a:b]
    for old,new in pairs:
        assert old in part,old
        part=part.replace(old,new)
    s=s[:a]+part+s[b:]
region('var mixed=j[token*32u+k].x','let value=f32(f16(tanh(mixed)));',[
 ('39241u+k*64u','39241u+k'),('i[weight+channel]','i[weight+channel*32u]'),('i[weight+32u+channel]','i[weight+(32u+channel)*32u]')])
region('for(var gate=lane;','for(var hidden=lane;',[('35865u+gate*64u','35865u+gate'),('i[weight+channel]','i[weight+channel*16u]'),('i[weight+32u+channel]','i[weight+(32u+channel)*16u]')])
region('for(var hidden=lane;','for(var label=lane;',[('29640u+hidden*80u','29640u+hidden'),('i[weight+channel]','i[weight+channel*72u]'),('i[weight+32u+channel]','i[weight+(32u+channel)*72u]'),('let extra=29640u+hidden+64u;','let extra=29640u+hidden+64u*72u;'),('i[extra+gate]','i[extra+gate*72u]')])
(root/'assets/transposed.wgsl').write_text(s)
raw=(root/'assets/weights.f32').read_bytes()
words=list(struct.unpack('<41321I',raw));transposed=words.copy()
for offset,rows,columns in [(37129,32,32),(38185,32,32),(39241,32,64),(35865,16,64),(29640,72,80),(25920,9,72)]:
    for row in range(rows):
        for column in range(columns):
            transposed[offset+column*rows+row]=words[offset+row*columns+column]
assert sorted(words)==sorted(transposed)
(root/'assets/weights-transposed.f32').write_bytes(struct.pack('<41321I',*transposed))
