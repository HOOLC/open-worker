# Cue design resources

Reused from the company's Cue checkout at `AFK-surf/Cue`, commit
`ac54574a3` (2026-09-05), per the product owner's instruction to share resources.
No Cue service, JavaScript runtime, or network font download is required by zork.

- `*.svg`: the actual Central Icons components imported by Cue's
  `clients/packages/ui/src/components/icons/index.tsx`, rendered in `raw` mode.
  `icons.json` records each export and package variant. Stroke width `2` becomes
  `1.5`, exactly as Cue's `styles.css` and `tokens/icons.ts` specify. Fill-only
  glyphs use Cue's pre-thinned 1.5 package. Paths are otherwise unchanged.
- `fonts/*.woff2`: original Cue Inter Variable regular and italic font files.
- `fonts/*.ttf`: the same fonts decoded losslessly to SFNT with FontTools for
  macOS CoreText/GPUI. They retain the `Inter Variable` family and variable axes.
- `fonts/LICENSE.txt`: the original Inter SIL Open Font License.

Regenerate on mini1 after Cue's pinned, frozen pnpm install:

```sh
node scripts/sync-cue-assets.mjs ../Cue
uv run --with fonttools --with brotli python - <<'PY'
from pathlib import Path
from fontTools.ttLib import TTFont
for path in Path('crates/zork-gui/assets/cue/fonts').glob('*.woff2'):
    font = TTFont(path)
    font.flavor = None
    font.save(path.with_suffix('.ttf'))
PY
```

Central Icons are shared company design resources; the Inter OFL does not
relicense those icons. Their original package attribution is in `icons.json`.

The Home sample-provider row is rendered from Cue’s `ProviderBrandLogos.tsx` (Linear, Slack, GitHub, Google Drive) and its original circular-tile/mask CSS. Regenerate with `node scripts/render-cue-provider-logos.cjs ../Cue`; it is decorative and does not indicate connected services.

`mascot.svg` is the original `logoPath` and `commaPath` at animation progress 0 from
Cue `clients/packages/ui/src/components/cue-mascot/CueLogoAnimation.tsx`, commit
`e9a817c0c`. It is the participant avatar for the native session history entry.
