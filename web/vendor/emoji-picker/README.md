# Vendored emoji picker

Audience: Infuser developers.

- emoji-picker-element **1.29.1**, Apache-2.0, https://github.com/nolanlawson/emoji-picker-element
- emoji-picker-element-data **1.8.0**, English Emojibase dataset, Apache-2.0, https://github.com/nolanlawson/emoji-picker-element-data
- Original licenses: LICENSE and DATA-LICENSE. Emoji artwork uses the platform font.

Source: versioned npm package tarballs. No runtime CDN or npm build dependency.
Uses picker.js, database.js and en/emojibase/data.json.

Local changes:
1. Extract picker.js baseStyles into picker.css; use a shadow-root stylesheet link for strict CSP. Append the platform emoji font stack. Apply dynamic style expressions through CSSOM instead of setAttribute.
2. Replace database.js internal getEmojiBySearchQuery prefix lookup with an AND substring match on existing normalized tokens, sorted by Unicode order. Names, aliases and keywords share upstream tokenization (minimum two characters).

Updates must preserve both adaptations. Run tests/chat_emoji_journey.mjs under the production CSP. Do not relax CSP.
