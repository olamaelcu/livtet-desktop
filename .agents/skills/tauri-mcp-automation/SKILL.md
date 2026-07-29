---
name: tauri-mcp-automation
description: Use when driving a Tauri desktop app via the @hypothesi/tauri-mcp-server MCP bridge — symptoms include `HTMLInputElement.value setter can only be used on instances of HTMLInputElement`, `SyntaxError: Unexpected token '('` from `tauri_webview_execute_js`, web components (WebAwesome, Lit, etc.) missing in screenshots, or needing to verify a reactive state change after a click.
---

# Tauri MCP Automation

## Overview

The Tauri MCP bridge wraps every script you send through `tauri_webview_execute_js` and exposes browser-driver functions that don't always work on custom elements (WebAwesome, Lit, Shoelace, etc.). This skill documents the workarounds that actually work.

## When to Use

- You need to fill a text input or click a button in a running Tauri app
- `tauri_webview_execute_js` times out with `SyntaxError: Unexpected token '('`
- `webview_keyboard` fails with `The HTMLInputElement.value setter can only be used on instances of HTMLInputElement`
- Screenshots show inputs/buttons missing even though the DOM contains them
- You're driving a Svelte/Lit/WebAwesome app and need to verify a reactive change

## Fill Inputs (WebAwesome, Lit, any custom element)

```js
// Set the property on the element directly, then dispatch a bubbling input event
const el = document.querySelector('wa-input');
el.value = 'Computer Bot';
el.dispatchEvent(new Event('input', { bubbles: true }));
```

For a plain `<input>` (React-controlled, etc.), use the native value setter so frameworks notice the change:

```js
const el = document.querySelector('input[name=email]');
const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
setter.call(el, '[email protected]');
el.dispatchEvent(new Event('input', { bubbles: true }));
```

## Click Buttons

JS `.click()` is more reliable for IPC roundtrip tests than the visual tool:

```js
document.querySelector('wa-button').click();
```

Use `tauri_webview_interact` action=`click` when you also need to confirm the click landed at the right coordinates.

## Verify State (Screenshots Lie on Shadow DOM)

`html2canvas` cannot render `<wa-*>` or other shadow-DOM custom elements. After a click, the screenshot may look unchanged even though the app re-rendered. Verify by reading the DOM directly:

```js
document.querySelector('wa-callout').textContent
```

Or use `tauri_webview_dom_snapshot` type=`structure` to see what appeared.

## execute_js Script Gotcha

The bridge expects a bare statement, not an IIFE. `() => { ... }()` throws `SyntaxError: Unexpected token '('. Expected a ';' following a return statement.` Always end with a literal:

```js
// BAD — bridge rejects
(() => { return document.title; })()

// GOOD — bare statement, trailing literal
document.title; 1
```

## Common Mistakes

- Using `tauri_webview_keyboard` type=... on a custom element — crashes with `HTMLInputElement.value setter`. Use the property-setter pattern above.
- Wrapping JS in an arrow IIFE — bridge rejects. Use bare statements.
- Forgetting `bubbles: true` on the input event — Svelte/React won't pick up the change.
- Trusting screenshots for shadow-DOM elements — render comes back blank. Read `textContent` directly.
- Reading only the DOM snapshot `[ref=eN]` after a click — the same ref can refer to a different element once Svelte re-renders. Re-snapshot before every interaction.
