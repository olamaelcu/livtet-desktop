/**
 * Sanitizing renderer for catalog HTML.
 *
 * Edition descriptions are HTML: they come from EPUB metadata, and (once the
 * book index supplies them) from third-party records. Rendering that with
 * `{@html}` puts attacker-controlled markup inside a WebView that holds
 * `window.__TAURI__`, so it is parsed into an inert document, walked against an
 * allowlist, and re-serialized. `{@html sanitizeHtml(html)}` is the only safe
 * way to render catalog HTML.
 *
 * The browser's Sanitizer API (`Element.prototype.setHTML`) is not available in
 * the WebKitGTK WebView Tauri ships on Linux, so this parser-based path is the
 * portable one.
 */

/** Elements kept as-is; their attributes still go through the allowlist. */
const ALLOWED_ELEMENTS: Record<string, true> = {
  a: true,
  b: true,
  blockquote: true,
  br: true,
  code: true,
  del: true,
  div: true,
  em: true,
  h1: true,
  h2: true,
  h3: true,
  h4: true,
  h5: true,
  h6: true,
  hr: true,
  i: true,
  ins: true,
  li: true,
  ol: true,
  p: true,
  pre: true,
  s: true,
  small: true,
  span: true,
  strong: true,
  sub: true,
  sup: true,
  u: true,
  ul: true,
}

/**
 * Elements dropped together with their content. Anything that can execute,
 * fetch, or navigate on its own belongs here: removing the subtree is what
 * makes the result inert, not just swapping the tag for its text.
 */
const DROPPED_ELEMENTS: Record<string, true> = {
  applet: true,
  audio: true,
  base: true,
  button: true,
  canvas: true,
  embed: true,
  form: true,
  frame: true,
  frameset: true,
  head: true,
  iframe: true,
  img: true,
  input: true,
  link: true,
  math: true,
  meta: true,
  noscript: true,
  object: true,
  option: true,
  picture: true,
  script: true,
  select: true,
  source: true,
  style: true,
  svg: true,
  template: true,
  textarea: true,
  title: true,
  track: true,
  video: true,
}

/** Attributes kept per element; every other attribute is stripped. */
const ALLOWED_ATTRIBUTES: Record<string, Record<string, true>> = {
  a: { href: true, title: true },
}

/** URL schemes a link may keep, so `javascript:` and `data:` cannot ride along. */
const SAFE_PROTOCOLS: Record<string, true> = {
  'http:': true,
  'https:': true,
  'mailto:': true,
}

/**
 * Return `html` as markup that is safe to assign to `innerHTML`: unknown
 * wrappers keep their text, dangerous elements lose their subtree, and every
 * attribute outside the allowlist is removed.
 */
export function sanitizeHtml(html: string): string {
  if (html.length === 0) return ''
  const parsed = new DOMParser().parseFromString(html, 'text/html')
  sanitizeChildren(parsed.body)
  return parsed.body.innerHTML
}

function sanitizeChildren(parent: Element): void {
  // Snapshot first: the loop removes and unwraps nodes as it goes.
  for (const node of [...parent.childNodes]) {
    if (node.nodeType === Node.TEXT_NODE) continue
    if (node.nodeType !== Node.ELEMENT_NODE) {
      // Comments carry nothing worth keeping, and processing instructions and
      // CDATA sections do not belong in body markup.
      node.remove()
      continue
    }

    const element = node as Element
    const tag = element.tagName.toLowerCase()
    if (Object.hasOwn(DROPPED_ELEMENTS, tag)) {
      element.remove()
      continue
    }
    if (!Object.hasOwn(ALLOWED_ELEMENTS, tag)) {
      sanitizeChildren(element)
      element.replaceWith(...element.childNodes)
      continue
    }
    sanitizeAttributes(element, tag)
    sanitizeChildren(element)
  }
}

function sanitizeAttributes(element: Element, tag: string): void {
  const allowed = Object.hasOwn(ALLOWED_ATTRIBUTES, tag) ? ALLOWED_ATTRIBUTES[tag] : null
  for (const attribute of [...element.attributes]) {
    const name = attribute.name.toLowerCase()
    if (!allowed || !Object.hasOwn(allowed, name)) {
      element.removeAttribute(attribute.name)
      continue
    }
    if (name === 'href' && !isSafeUrl(attribute.value)) {
      element.removeAttribute(attribute.name)
    }
  }
}

function isSafeUrl(value: string): boolean {
  try {
    // `new URL` strips embedded tabs and newlines and lowercases the scheme, so
    // `Java\nScript:` resolves to `javascript:` and is rejected here.
    return Object.hasOwn(SAFE_PROTOCOLS, new URL(value, document.baseURI).protocol)
  } catch {
    return false
  }
}
