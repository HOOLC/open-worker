"""Rebuild the local design handbook and SVG inspection sheets."""
from pathlib import Path
import hashlib
import html
import json
import math
import re
import xml.etree.ElementTree as ET
import markdown
import resvg_py

ROOT = Path(__file__).resolve().parents[1]
DOCS = sorted((ROOT / 'docs').glob('*.md'))
GROUPS = [('brand','品牌标志','当前方向 · 字标待评审'),('avatars','动物头像','当前方向'),('icons/product','产品图标','当前方向'),('icons/interface','界面图标','当前方向'),('providers','供应商','外部身份'),('concepts/app-icon','App 图标','提案'),('concepts/illustrations','SVG 状态','复用功能图标')]
CURATED_COUNT=sum(len(list((ROOT/'assets'/group).glob('*.svg'))) for group,_,_ in GROUPS)
TITLES = {p.stem: p.read_text().splitlines()[0].removeprefix('# ') for p in DOCS}

HEADER_WORDMARK=(ROOT/'assets/brand/zork-wordmark-draft.svg').read_text().strip().replace('<svg ', '<svg class="home-wordmark" ',1)

def shell(title, body, prefix='', active=''):
    nav = ''.join(f'<a href="{prefix}docs/{p.stem}.html"'+(' aria-current="page"' if active==p.stem else '')+f'>{i:02} {html.escape(TITLES[p.stem])}</a>' for i,p in enumerate(DOCS,1))
    return f'''<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{html.escape(title)} · Zork Design</title><link rel="icon" href="{prefix}assets/brand/mark.svg"><link rel="stylesheet" href="{prefix}handbook.css"></head><body><header><a class="home" href="{prefix}index.html"><img src="{prefix}assets/brand/mark.svg" alt="">{HEADER_WORDMARK}<span>Design</span></a><nav><a href="{prefix}index.html#identity">品牌</a><a href="{prefix}index.html#product">产品</a><a href="{prefix}assets/index.html">素材</a><a href="{prefix}mobile/index.html">移动端</a><a href="{prefix}prototype/gui.html">交互样稿 ↗</a></nav></header><div class="reading-layout"><aside>{nav}<a href="{prefix}README.md">Markdown 目录 ↗</a></aside><main class="article">{body}</main></div><footer>2026.09.06 · 本地设计资料 · 未定稿内容均标记为提案</footer></body></html>'''

for path in DOCS:
    source=path.read_text()
    source=re.sub(r'\]\(([^)]+)\.md\)',r'](\1.html)',source)
    body=markdown.markdown(source,extensions=['tables','fenced_code','toc'])
    (path.with_suffix('.html')).write_text(shell(TITLES[path.stem],body,'../',path.stem))

from native_inventory import build_native_inventory
native_inventory=build_native_inventory(shell)
from brand_pages import build_brand_pages
build_brand_pages(ROOT,shell)
# A visual, searchable catalog keeps originals one click away.
galleries=[]
for group,label,status in GROUPS:
    items=[]
    for path in sorted((ROOT/'assets'/group).glob('*.svg')):
        relative=path.relative_to(ROOT/'assets').as_posix()
        items.append(f'<a class="asset" href="{relative}" data-name="{path.stem} {label}"><span class="asset-image" style="background:{'#24272B' if 'reverse' in path.stem else '#F3F2ED'}"><img src="{relative}" alt="{html.escape(path.stem)}" loading="lazy"></span><b>{path.stem}</b><small>SVG · {status}</small></a>')
    galleries.append(f'<section><h2>{label} <small>{len(items)}</small></h2><div class="asset-grid">'+''.join(items)+'</div></section>')
search=f'''<h1>设计素材</h1><p>{CURATED_COUNT} 个整理后的设计 SVG；另有实际原生资源全量清单，可查看两个代码版本和引用位置。</p><div class="actions"><a class="button primary" href="native/index.html">原生 SVG 完整清单</a><a class="button" href="../motion/index.html">四种动画</a><a class="button" href="../docs/11-avatars.html">头像生成规范</a></div><input id="asset-search" class="search" type="search" aria-label="搜索素材" placeholder="搜索名称或分类，例如 fox、品牌、图标"><p><a href="manifest.json">来源与 SHA-256 清单</a> · <a href="../previews/materials.png">素材检视图</a></p>'''
asset_script='''<script>document.querySelector('#asset-search').addEventListener('input',e=>{const q=e.target.value.trim().toLowerCase();document.querySelectorAll('[data-name]').forEach(a=>{a.hidden=!a.dataset.name.toLowerCase().includes(q)});});</script>'''
(ROOT/'assets/index.html').write_text(shell('素材库',search+''.join(galleries)+asset_script,'../'))

avatars=''.join(f'<a href="assets/avatars/{p.name}"><img src="assets/avatars/{p.name}" alt="{p.stem}" loading="lazy"><span>{p.stem}</span></a>' for p in sorted((ROOT/'assets/avatars').glob('*.svg')))
wordmark=(ROOT/'assets/brand/zork-wordmark-draft.svg').read_text().replace('<svg ','<svg class="wordmark" ',1)
chapter_links=''.join(f'<a href="docs/{p.stem}.html"><span>{i:02}</span><b>{TITLES[p.stem]}</b><span>↗</span></a>' for i,p in enumerate(DOCS,1))
icons=''.join(f'<a href="assets/icons/product/{p.name}" title="{p.stem}"><img src="assets/icons/product/{p.name}" alt="{p.stem}"><span>{p.stem}</span></a>' for p in sorted((ROOT/'assets/icons/product').glob('*.svg')))
brand=f'''<section id="identity"><div class="section-title"><span>01 / IDENTITY</span><h2>折角伙伴，安静地接着做。</h2></div><div class="identity"><div class="mark-stage"><img src="assets/brand/mark-reverse.svg" alt="折角伙伴反白标志"></div><div class="word-stage"><div class="brand-motion"><img class="mascot" src="assets/brand/mark.svg" alt="折角伙伴">{wordmark}</div><span class="status">字标提案 · 待评审</span><p>名称统一为 <b>Zork</b>。悬停可看图标与字标联动。v2 将折角放入 Z 的底部轮廓，另有更柔和的回转候选，均未定稿。</p><a href="wordmark/index.html">比较字标方案 →</a> · <a href="motion/index.html">四种品牌动画 →</a></div></div><div class="swatches"><div style="--swatch:#24272B"><i></i><b>炭墨</b><code>#24272B</code></div><div style="--swatch:#F6F3EA"><i></i><b>暖纸</b><code>#F6F3EA</code></div><div style="--swatch:#E9643B"><i></i><b>柿橙</b><code>#E9643B</code></div></div><p class="muted">颜色负责品牌辨识；人物、角色和工作状态各有独立语义。宣传语、深色主题与平台 App 图标仍保留为提案。</p></section>'''
product='''<section id="product"><div class="section-title"><span>02 / PRODUCT</span><h2>设备 → Leader → Task</h2></div><p>以长期协作和群聊式任务为核心。层级通过缩进表达，整行反馈保持统一；信息和配置始终属于明确的设备。</p><div class="rules"><div><b>30 px</b><span>统一导航行高</span></div><div><b>44 px</b><span>紧凑会话头</span></div><div><b>200–420 px</b><span>可调侧栏宽度</span></div><div><b>32–160 px</b><span>输入框自动增高</span></div></div><div class="prototype-frame"><div class="frame-label"><span>当前交互样稿 · 示例数据</span><a href="prototype/gui.html">全屏体验 ↗</a></div><iframe src="prototype/gui.html" title="Zork 当前群聊式工作台样稿" loading="lazy"></iframe></div><p class="muted">已统一 hover / active，选中和未读不改变字重；设置按设备组织，模型连接先选接入方式再选供应商。</p><a href="docs/04-components.html">组件与交互规范 →</a></section>'''
mobile_section='''<section id="mobile"><div class="section-title"><span>02B / MOBILE</span><h2>同一套产品关系，适合触摸的界面。</h2></div><div class="mobile-overview"><iframe src="mobile/prototype/index.html#nav" title="Zork 移动端 nav7 原型" loading="lazy"></iframe><div><span class="status">移动端 nav7</span><h3>导航与对话，各占一屏。</h3><p>整行按压反馈、48 px 导航行、并排底部入口。聊天列表不保留持续选中；从设备设置返回时恢复原对话、草稿与阅读位置。</p><div class="actions"><a class="button primary" href="mobile/index.html">移动端设计入口</a><a class="button" href="mobile/prototype/index.html#chat/guide">打开群聊 ↗</a></div><p><a href="docs/10-mobile.html">桌面与移动端规范对照 →</a></p><p class="muted">完整 HTML 已按用户反馈迭代；初版三屏图片作为历史概念保存，不再作为当前交互真值。</p></div></div></section>'''
from scenes import STATES
state_grid='<div class="state-symbol-grid">'+''.join('<figure>'+ (ROOT/f'assets/concepts/illustrations/{name}.svg').read_text() +f'<figcaption>{label}</figcaption></figure>' for name,(label,_) in STATES.items())+'</div>'
body=f'''<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Zork · 设计规范与品牌素材</title><meta name="description" content="Zork 当前产品概念、视觉规范、交互规则和当前SVG设计素材。"><link rel="icon" href="assets/brand/mark.svg"><link rel="stylesheet" href="handbook.css"></head><body id="top"><header><a class="home" href="#top"><img src="assets/brand/mark.svg" alt="">{HEADER_WORDMARK}<span>Design</span></a><nav><a href="#identity">品牌</a><a href="#product">产品</a><a href="components/index.html">组件库</a><a href="#mobile">移动端</a><a href="#avatars">头像</a><a href="#inventory">规范</a><a href="#delivery">素材</a></nav></header><main class="overview"><div class="intro"><span class="eyebrow">DESIGN SYSTEM / 2026.09.06</span><h1>让协作有清晰的形状。</h1><p>产品概念、视觉语言和交互规则，整理在同一个设计目录中。<br>这是当前规范与素材入口，探索中的部分仍保留明确标记。</p><div class="actions"><a class="button primary" href="prototype/gui.html">打开交互样稿 ↗</a><a class="button" href="docs/01-concepts.html">阅读设计文档</a><a class="button" href="assets/index.html">浏览设计 SVG</a></div></div>{brand}{product}<section id="components"><div class="section-title"><span>RUST / GPUI / WASM</span><h2>设计示例，直接运行组件库。</h2></div><p>桌面客户端、Web 展台和这里的例子共用独立的 zork-ui 包。试着展开菜单或切换组件；样式与交互来自同一份 Rust 实现。</p><iframe src="components/web/index.html?story=dropdown-closed" title="同源 GPUI 下拉菜单示例" loading="lazy" style="width:100%;max-width:560px;height:230px;border:1px solid #DFE0E2;border-radius:10px"></iframe><p><a href="components/index.html">打开可交互组件库与设计对照 →</a></p></section>{mobile_section}<section id="avatars"><div class="section-title"><span>03 / IDENTITY OF PEOPLE</span><h2>角色有职责，个体有样子。</h2></div><p>12 个稳定的动物头像。Leader、Worker、在线和未读用独立信息表达，不靠头像猜测。</p><div class="avatar-strip">{avatars}</div><p><a href="docs/11-avatars.html">头像生成规范与可复用简报 →</a></p><h3>功能图标</h3><div class="icon-strip">{icons}</div><p><a href="assets/native/index.html">查看实际原生 SVG 全量清单与引用位置 →</a></p><p class="muted">产品图标与界面操作图标分别维护。基础功能图标采用 24×24 母版与约 1.7 描边；部分状态图形保留轻量样张，详见素材清单。</p></section><section id="scenes"><div class="section-title"><span>INTERFACE STATES / SVG</span><h2>界面状态，用简洁的 SVG 表达。</h2></div><p>沿用功能图标的线条、圆角和比例，仅在需要引导或说明时出现。</p>{state_grid}<p><a href="docs/12-icons-and-scenes.html">功能图标与状态图形规范 →</a></p></section><section id="inventory"><div class="section-title"><span>04 / GUIDELINES</span><h2>从概念到具体的每一行。</h2></div><div class="chapters">{chapter_links}</div></section><section id="delivery"><div class="section-title"><span>05 / MATERIALS</span><h2>可编辑，也能继续维护。</h2></div><p>素材源文件、设计参数、文档与交互样稿集中在 <code>zork-design</code>。旧字标和早期提案已归档，避免混入当前方案。</p><div class="actions"><a class="button primary" href="assets/index.html">打开素材库</a><a class="button" href="tokens/design-tokens.json">设计 tokens</a><a class="button" href="assets/manifest.json">来源清单</a><a class="button" href="README.md">文件目录</a></div><p class="muted">素材包括主标志、12 个头像、18 个产品图标、34 个界面图标、8 个供应商符号、4 个 App icon 母稿和 8 个状态 SVG。字标、App icon 与插画的定稿状态见各自说明。</p></section></main><footer>设计基线：桌面 nav34 / brand36 · 移动端 nav7 · 本地预览 · 原生源码已接入并验证</footer><script>document.querySelectorAll('a[href="#"]').forEach(a=>a.addEventListener('click',e=>{{e.preventDefault();window.scrollTo({{top:0,behavior:matchMedia('(prefers-reduced-motion: reduce)').matches?'instant':'smooth'}});}}));</script></body></html>'''
(ROOT/'archive/handbook-static.html').write_text(body)
from mobile_pages import build_mobile
build_mobile(ROOT, shell)

# Deterministic contact sheet; always derived from the editable SVG originals.
parts=['<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="960"><rect width="1200" height="960" fill="#F6F3EA"/><text x="48" y="52" fill="#24272B" font-family="Arial" font-size="28">Zork Design / Materials</text>']
def place(path,x,y,w,h):
    element=ET.parse(path).getroot();element.set('x',str(x));element.set('y',str(y));element.set('width',str(w));element.set('height',str(h));parts.append(ET.tostring(element,encoding='unicode'))
place(ROOT/'assets/brand/mark.svg',48,85,110,110)
place(ROOT/'assets/brand/zork-wordmark-draft.svg',202,105,248,88)
parts.append('<text x="205" y="220" fill="#62656B" font-family="Arial" font-size="14">Wordmark / draft</text>')
for i,p in enumerate(sorted((ROOT/'assets/avatars').glob('*.svg'))):
 x=48+(i%6)*188;y=280+(i//6)*160;place(p,x,y,88,88);parts.append(f'<text x="{x}" y="{y+113}" fill="#62656B" font-family="Arial" font-size="14">{p.stem}</text>')
for i,p in enumerate(sorted((ROOT/'assets/icons/product').glob('*.svg'))):
 x=48+(i%9)*128;y=635+(i//9)*108;place(p,x,y,30,30);parts.append(f'<text x="{x}" y="{y+55}" fill="#62656B" font-family="Arial" font-size="12">{p.stem}</text>')
for i,p in enumerate(sorted((ROOT/'assets/providers').glob('*.svg'))):
 x=48+i*142;place(p,x,863,27,27);parts.append(f'<text x="{x}" y="920" fill="#62656B" font-family="Arial" font-size="11">{p.stem}</text>')
parts.append('</svg>')
svg=''.join(parts);(ROOT/'previews/materials.svg').write_text(svg)
(ROOT/'previews/materials.png').write_bytes(resvg_py.svg_to_bytes(svg_string=svg))
# Refresh source hashes after edits; do not overwrite status or provenance.
manifest_path=ROOT/'assets/manifest.json';manifest=json.loads(manifest_path.read_text())
for item in manifest['items']:
    path=ROOT/item['path'];item['sha256']=hashlib.sha256(path.read_bytes()).hexdigest()
manifest['items']=[i for i in manifest['items'] if i['path']!='licenses/Lobe-Icons-MIT.txt']
manifest['items'].append({'path':'licenses/Lobe-Icons-MIT.txt','category':'license','status':'required-attribution','source':'https://raw.githubusercontent.com/lobehub/lobe-icons/master/LICENSE','sha256':hashlib.sha256((ROOT/'licenses/Lobe-Icons-MIT.txt').read_bytes()).hexdigest()})
manifest_path.write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
print(f'Built {len(DOCS)} chapters, visual handbook, asset catalog and inspection sheet.')
