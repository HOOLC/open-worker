"""Render the mobile source documents without duplicating their authority."""
from pathlib import Path
import re
import markdown


def build_mobile(root: Path, shell):
    mobile = root / 'mobile'
    for path in sorted(mobile.rglob('*.md')):
        text = path.read_text()
        text = re.sub(r'\]\(([^)]+)\.md\)', r'](\1.html)', text)
        # The shared root README is the handbook entry, not a separate HTML page.
        text = text.replace('](../README.html)', '](../index.html)')
        body = markdown.markdown(text, extensions=['tables', 'fenced_code', 'toc'])
        prefix = '../' * len(path.parent.relative_to(root).parts)
        title = path.read_text().splitlines()[0].removeprefix('# ')
        if path == mobile / 'README.md':
            preview = '''<div class="mobile-reading-preview"><iframe src="prototype/index.html#nav" title="移动端 nav7 导航与群聊交互样稿" loading="lazy"></iframe><div><span class="status">nav7 · 当前交互原型</span><h2>为触摸单独定义交互。</h2><p>48 px 导航行，只有按压反馈；聊天列表不保留上次对话的选中状态。设备设置能沿原路返回对话。</p><div class="actions"><a class="button primary" href="prototype/index.html#nav">完整原型 ↗</a><a class="button" href="prototype/index.html#chat/guide">群聊</a><a class="button" href="prototype/index.html#device/mini1">设备设置</a></div><p class="muted">原型使用示例数据。实际手机键盘、选择手柄和浏览器视觉复核仍待完成。</p></div></div>'''
            end = body.index('</h1>') + len('</h1>')
            body = body[:end] + preview + body[end:]
            (mobile / 'index.html').write_text(shell(title, body, prefix, '10-mobile'))
        path.with_suffix('.html').write_text(shell(title, body, prefix, '10-mobile'))
