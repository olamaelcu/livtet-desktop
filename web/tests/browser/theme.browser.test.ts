import tokens from '@olamaelcu/livtet-branding/tokens.json'
import { expect, test } from 'vitest'
import '../../app.css'

/**
 * The theme is wired by CSS import order alone: Web Awesome's default theme,
 * then the Livtet brand layer, then `native.css`. These assertions compare the
 * computed custom properties against the package's own `tokens.json`, so a
 * palette bump needs no edit here while a dropped or reordered `@import` fails.
 */

const token = (name: string) =>
  getComputedStyle(document.documentElement).getPropertyValue(name).trim()

/** `oklch(L% C H)` as numbers, so formatting differences cannot fail a match. */
function oklch(value: string): [number, number, number] {
  const match = /oklch\(\s*([\d.]+)%\s+([\d.]+)\s+([\d.]+)\s*\)/.exec(value)
  if (!match) throw new Error(`not an oklch colour: ${value}`)
  return [Number(match[1]), Number(match[2]), Number(match[3])]
}

test('the brand layer wins over the default palette', () => {
  const brand = tokens.color.brand.oklch
  // `--wa-color-brand` is the ramp's 50 step, generated from the base colour.
  expect(oklch(token('--wa-color-brand-50'))).toEqual([
    Math.round(brand.lightness * 100),
    brand.chroma,
    brand.hue,
  ])
  expect(token('--wa-color-brand')).toBe(token('--wa-color-brand-50'))
  // The theme maps the focus ring onto the ramp's 60 step, whatever it becomes.
  expect(oklch(token('--wa-color-focus'))).toEqual(oklch(token('--wa-color-brand-60')))
})

test('the brand palette carries both colour schemes', () => {
  const root = document.documentElement
  const { surface, text } = tokens.color

  root.classList.remove('wa-dark', 'wa-light')
  expect(token('--wa-color-surface-default')).toBe(surface.light.default)
  expect(token('--wa-color-text-normal')).toBe(text.light.normal)

  root.classList.add('wa-dark')
  expect(token('--wa-color-surface-default')).toBe(surface.dark.default)
  expect(token('--wa-color-text-normal')).toBe(text.dark.normal)
  root.classList.remove('wa-dark')
})

test('headings keep the variable Lora family', () => {
  // The generated tokens name the static family; only the variable build is
  // installed, so app.css re-declares the heading stack on the variable name.
  expect(token('--wa-font-family-heading')).toContain('Lora Variable')
  expect(token('--wa-font-family-body')).toContain('Work Sans')
  expect(token('--wa-font-family-code')).toContain('JetBrains Mono')
})
