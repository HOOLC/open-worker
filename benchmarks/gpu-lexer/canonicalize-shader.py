"""Use bottom-updated loops, as in Tint's MSL, instead of Naga continuing blocks."""
from pathlib import Path
import re
root=Path(__file__).resolve().parent
s=(root/'assets/tiny.wgsl').read_text()
def match_end(text,start,left,right):
    depth=0
    for i in range(start,len(text)):
        if text[i]==left:depth+=1
        elif text[i]==right:
            depth-=1
            if not depth:return i
    raise ValueError('unbalanced shader')
count=0
def transform(text):
    global count
    output='';offset=0
    while m:=re.search(r'\bfor\s*\(',text[offset:]):
        start=offset+m.start();paren=offset+m.end()-1
        end=match_end(text,paren,'(',')')
        header=text[paren+1:end].split(';');assert len(header)==3
        body_start=end+1
        while text[body_start].isspace():body_start+=1
        assert text[body_start]=='{'
        body_end=match_end(text,body_start,'{','}')
        body=transform(text[body_start+1:body_end])
        if 'continue' not in body:
            init,condition,update=header
            replacement='{'+init+';loop{if(!('+condition+')){break;}'+body+update+';}}'
            count+=1
        else:
            replacement=text[start:body_start+1]+body+'}'
        output+=text[offset:start]+replacement;offset=body_end+1
    return output+text[offset:]
(root/'assets/canonical.wgsl').write_text(transform(s))
print('Canonicalized loops:',count)
