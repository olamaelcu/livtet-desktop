import { expect, test } from 'vitest'
import { sanitizeHtml } from '$lib/html'

test('keeps the formatting a description legitimately uses', () => {
  expect(sanitizeHtml('<p>Hello <strong>world</strong> — <em>again</em></p>')).toBe(
    '<p>Hello <strong>world</strong> — <em>again</em></p>',
  )
  expect(sanitizeHtml('<ol><li>one</li><li>two</li></ol>')).toBe(
    '<ol><li>one</li><li>two</li></ol>',
  )
})

test('drops scripts together with their content', () => {
  expect(sanitizeHtml('before<script>alert(1)</script>after')).toBe('beforeafter')
  expect(sanitizeHtml('<p><svg><script>alert(1)</script></svg></p>')).toBe('<p></p>')
  expect(sanitizeHtml('<style>p { color: red }</style><p>x</p>')).toBe('<p>x</p>')
})

test('strips every attribute outside the allowlist', () => {
  expect(sanitizeHtml('<p onclick="alert(1)" class="c" style="color:red" id="i">hi</p>')).toBe(
    '<p>hi</p>',
  )
})

test('unwraps elements it does not know instead of losing their text', () => {
  expect(sanitizeHtml('<section><p>x</p></section>')).toBe('<p>x</p>')
  expect(sanitizeHtml('<constructor>ok</constructor>')).toBe('ok')
})

test('keeps http, https, mailto and relative links', () => {
  expect(sanitizeHtml('<a href="https://example.org/a" title="t">x</a>')).toBe(
    '<a href="https://example.org/a" title="t">x</a>',
  )
  expect(sanitizeHtml('<a href="mailto:a@example.org">x</a>')).toBe(
    '<a href="mailto:a@example.org">x</a>',
  )
  expect(sanitizeHtml('<a href="/library">x</a>')).toBe('<a href="/library">x</a>')
})

test('drops unsafe link schemes, including obfuscated ones', () => {
  expect(sanitizeHtml('<a href="javascript:alert(1)">x</a>')).toBe('<a>x</a>')
  expect(sanitizeHtml('<a href="JaVaScRiPt:alert(1)">x</a>')).toBe('<a>x</a>')
  expect(sanitizeHtml('<a href="java\nscript:alert(1)">x</a>')).toBe('<a>x</a>')
  expect(sanitizeHtml('<a href="data:text/html,<script>alert(1)</script>">x</a>')).toBe('<a>x</a>')
})

test('drops elements that fetch or navigate by themselves', () => {
  expect(sanitizeHtml('<img src="x" onerror="alert(1)">kept')).toBe('kept')
  expect(sanitizeHtml('<iframe src="https://example.org"></iframe>kept')).toBe('kept')
  // The subtree goes with the element: text inside a form is not "kept".
  expect(sanitizeHtml('<form><input name="a">gone</form>kept')).toBe('kept')
})

test('removes comments and tolerates markup with no content', () => {
  expect(sanitizeHtml('<p>a<!-- hidden -->b</p>')).toBe('<p>ab</p>')
  expect(sanitizeHtml('')).toBe('')
})
