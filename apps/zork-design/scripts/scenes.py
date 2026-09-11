"""Minimal interface state SVGs, reusing the functional icon system."""
from pathlib import Path
import re
ROOT=Path(__file__).resolve().parents[1]
STATES={
 'node-setup':('添加设备','product/node'),
 'first-conversation':('开始对话','interface/task-chat'),
 'inbox-clear':('暂无待办','interface/inbox'),
 'review-ready':('等待审阅','product/review'),
 'task-accepted':('已完成','product/completed'),
 'device-offline':('设备离线','product/offline'),
 'files-empty':('暂无文件','product/workspace'),
 'hero':('协作','product/handoff'),
}
for name,(label,source) in STATES.items():
 svg=(ROOT/f'assets/icons/{source}.svg').read_text()
 svg=re.sub(r'<title>.*?</title>',f'<title>{label}</title>',svg,flags=re.S)
 (ROOT/f'assets/concepts/illustrations/{name}.svg').write_text(svg)
print('Built eight minimal SVG state symbols from the functional icon system.')
