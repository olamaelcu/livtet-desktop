/// <reference types="svelte" />

declare global {
  namespace App {
    // interface Error {}
    // interface Locals {}
    // interface PageData {}
    // interface PageState {}
    // interface Platform {}
  }

  namespace svelteHTML {
    interface IntrinsicElements {
      'wa-button': import('svelte/elements').HTMLButtonAttributes & {
        variant?: 'brand' | 'neutral' | 'success' | 'warning' | 'danger' | 'text' | 'default'
        size?: 's' | 'm' | 'l' | 'small' | 'medium' | 'large'
        appearance?: 'accent' | 'filled' | 'outlined' | 'plain'
        type?: 'button' | 'submit' | 'reset'
        disabled?: boolean
        loading?: boolean
        pill?: boolean
        href?: string
        target?: string
      }
      'wa-input': import('svelte/elements').HTMLInputAttributes & {
        label?: string
        hint?: string
        placeholder?: string
        value?: string | number
        type?: string
        size?: 's' | 'm' | 'l' | 'small' | 'medium' | 'large'
        appearance?: string
        pill?: boolean
        required?: boolean
        disabled?: boolean
      }
      'wa-card': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        appearance?: 'accent' | 'filled' | 'outlined' | 'plain'
        withHeader?: boolean
        withFooter?: boolean
      }
      'wa-icon': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        name?: string
        family?: string
        variant?: string
        library?: string
      }
      'wa-callout': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        variant?: 'brand' | 'neutral' | 'success' | 'warning' | 'danger'
        appearance?: 'accent' | 'filled' | 'outlined' | 'plain'
        size?: 's' | 'm' | 'l' | 'small' | 'medium' | 'large'
      }
      'wa-divider': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        orientation?: 'horizontal' | 'vertical'
        spacing?: string
      }
      'wa-spinner': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        size?: string
        variant?: string
      }
    }
  }
}

export {}
