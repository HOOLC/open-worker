# Agent avatars

Twelve original Zork SVG illustrations approved in the brand review prototype: cat, bunny, bear, fox, panda, chick, dog, owl, koala, penguin, deer and octopus. Source: `artifacts/brand-review-site/src/svg/avatars` in the canonical development checkout. These embedded illustrations require no fonts or network requests.

The stored avatar key is shared across navigation, settings, member lists and delivered-message metadata. Existing six keys retain their identities. Missing legacy keys retain the existing cat fallback until the Agent is edited; delivered messages with no known author use the neutral Zork mark instead of inventing an Agent identity.

`portraits/` contains the same vector portraits without the rounded background plate. Composer presence uses these transparent variants; framed avatars elsewhere retain the original assets.
