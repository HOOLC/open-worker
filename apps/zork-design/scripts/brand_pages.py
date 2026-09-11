"""Comparison and independent motion previews for the brand wordmark work."""
from pathlib import Path
import markdown
import json

def build_brand_pages(root,shell):
    rows=[]
    for key,title,description in [('fold','折角 v2 · 当前使用','折口移到 Z 的底部，o、r、k 使用相近的切面比例。单色也保留识别点。'),('arc','回转 v2 · 备选','o 更圆，r 更安静，k 的末端向下回转。整体表情更柔和。')]:
        sizes=''.join(f'<span><img src="zork-{key}-v2.svg" style="width:{width}px" alt="Zork {title}"><small>{width}px</small></span>' for width in [170,85,70,56])
        rows.append(f'<section><h2>{title}</h2><p>{description}</p><div class="wordmark-large"><img src="zork-{key}-v2.svg" alt="{title}"></div><div class="wordmark-sizes">{sizes}</div><div class="wordmark-dark"><img src="zork-{key}-v2-reverse.svg" alt="{title}反白"></div><p><a href="zork-{key}-v2.svg">单色 SVG</a> · <a href="zork-{key}-v2-reverse.svg">反白 SVG</a></p></section>')
    body='<h1>Zork 定制字标</h1><p>两条可比较的字形方向。之前 Z 中的独立橙色块已从这两版移除；品牌伙伴的原轮廓保留。当前仍是探索稿。</p><p><a href="../motion/index.html">查看四种品牌动效 →</a></p>'+''.join(rows)
    (root/'wordmark/index.html').write_text(shell('定制字标',body,'../'))
    modes=[('icon','图标独立动画','伙伴轻轻侧身，再回到原位。',.68),('wordmark','Zork 独立动画','四个字母依次轻抬，保持占位宽度。',.675),('linked','联动动画','伙伴靠近字标，响应沿字母依次传递。',.805),('icon-to-wordmark','图标变成名字','轻轻蓄力、圆润弹开；下半部蠕动成 Z，上半部跳落、展开并回弹成 ork。',2.0)]
    cards=[]
    for name,title,description,duration in modes:
        svg=(root/f'motion/{name}.svg').read_text()
        notch_control='<label class="notch-control"><input type="checkbox" data-notch> 标出缺口对应位置</label>' if name=='icon-to-wordmark' else ''
        cards.append(f'<section class="motion-card" data-duration="{duration}" data-mode="{name}"><h2>{title}</h2><p>{description}</p><div class="motion-stage">{svg}</div><div class="motion-controls"><button type="button" class="button primary" data-play>播放</button><button type="button" class="button" data-reverse>反向</button><a href="{name}.svg">独立 SVG ↗</a></div><label class="scrubber">进度 <input type="range" min="0" max="1000" value="0" aria-label="{title}进度"><output>0%</output></label>{notch_control}</section>')
    intro='<h1>四种品牌动画</h1><p>图标、字标、联动与图形转换分别维护，可单独播放、反向和拖动进度。当前字形使用折角 v2。</p><div class="actions"><button class="button" id="play-all">播放全部</button><label><input id="slow" type="checkbox"> 慢速查看</label><a href="spec.json">参数</a> · <a href="morph-points.json">变形路径数据</a></div><p id="motion-note" class="muted" hidden>系统开启了减少动态效果，当前显示静态结果；仍可手动检查进度。</p>'
    script='''<script>
const notch=NOTCH_DATA;
function notchPositions(card,value){const group=card.querySelector('.notch-guides');if(!group)return;const frames=notch.frames,ms=value*frames[frames.length-1].time_ms;let i=0;while(i<frames.length-2&&frames[i+1].time_ms<ms)i++;const a=frames[i],b=frames[i+1],t=Math.max(0,Math.min(1,(ms-a.time_ms)/(b.time_ms-a.time_ms)));[...group.children].forEach((circle,j)=>{circle.setAttribute('cx',a.points[j][0]*(1-t)+b.points[j][0]*t);circle.setAttribute('cy',a.points[j][1]*(1-t)+b.points[j][1]*t);});}
const cards=[...document.querySelectorAll('.motion-card')];const reduced=matchMedia('(prefers-reduced-motion: reduce)');
function seek(card,value){const svg=card.querySelector('svg');svg.pauseAnimations();svg.setCurrentTime(Number(card.dataset.duration)*value);card.querySelector('input[type=range]').value=Math.round(value*1000);card.querySelector('output').textContent=Math.round(value*100)+'%';notchPositions(card,value);}
function stop(card){if(card.frame)cancelAnimationFrame(card.frame);card.frame=null;}
function play(card,reverse=false){if(reduced.matches)return;stop(card);const duration=Number(card.dataset.duration)*1000*(document.querySelector('#slow').checked?2.5:1);const start=performance.now();function frame(now){const p=Math.min(1,(now-start)/duration);seek(card,reverse?1-p:p);if(p<1)card.frame=requestAnimationFrame(frame);else card.frame=null;}card.frame=requestAnimationFrame(frame);}
cards.forEach(card=>{if(card.dataset.mode==='icon-to-wordmark'){const ns='http://www.w3.org/2000/svg',group=document.createElementNS(ns,'g');group.classList.add('notch-guides');group.style.display='none';for(let i=0;i<3;i++){const circle=document.createElementNS(ns,'circle');circle.setAttribute('r',i===1?'3.2':'2');circle.setAttribute('fill','none');circle.setAttribute('stroke','#E9643B');circle.setAttribute('stroke-width','1.5');group.appendChild(circle);}card.querySelector('svg').appendChild(group);card.querySelector('[data-notch]').addEventListener('change',e=>group.style.display=e.target.checked?'inline':'none');}seek(card,0);card.querySelector('[data-play]').addEventListener('click',()=>play(card));card.querySelector('[data-reverse]').addEventListener('click',()=>play(card,true));card.querySelector('input[type=range]').addEventListener('input',e=>{stop(card);seek(card,Number(e.target.value)/1000);});});
function preferences(){document.querySelector('#motion-note').hidden=!reduced.matches;document.querySelectorAll('[data-play],[data-reverse],#play-all').forEach(b=>b.disabled=reduced.matches);if(reduced.matches)cards.forEach(c=>{stop(c);seek(c,1);});}preferences();reduced.addEventListener('change',preferences);document.querySelector('#play-all').addEventListener('click',()=>cards.forEach(c=>play(c)));document.addEventListener('visibilitychange',()=>{if(document.hidden)cards.forEach(stop);});
</script>'''
    script=script.replace('NOTCH_DATA',json.dumps(json.loads((root/'motion/morph-points.json').read_text())['notch']))
    (root/'motion/index.html').write_text(shell('品牌动画',intro+'<div class="motion-grid">'+''.join(cards)+'</div><p>这四种动效已按同一几何与时序接入原生品牌组件；此页用于单独对照 SVG 样例。<a href="../implementation/index.html">查看原生帧证据 →</a></p>'+script,'../'))
    source=(root/'avatars/generation-brief.md').read_text()
    (root/'avatars/generation-brief.html').write_text(shell('头像生成简报',markdown.markdown(source,extensions=['tables','fenced_code']),'../','11-avatars'))

    source=(root/'implementation/README.md').read_text()
    (root/'implementation/index.html').write_text(shell('原生接入记录',markdown.markdown(source,extensions=['tables','fenced_code']),'../'))
