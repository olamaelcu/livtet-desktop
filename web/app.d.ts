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
      'wa-input': Omit<import('svelte/elements').HTMLInputAttributes, 'size' | 'value' | 'type'> & {
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
      'wa-drawer': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        open?: boolean
        label?: string
        placement?: 'top' | 'end' | 'bottom' | 'start'
        'onwa-after-hide'?: (event: Event) => void
      }
      'wa-spinner': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        size?: string
        variant?: string
      }
      'wa-select': Omit<import('svelte/elements').HTMLAttributes<HTMLElement>, 'size' | 'value'> & {
        label?: string
        hint?: string
        placeholder?: string
        value?: string | string[]
        multiple?: boolean
        'with-clear'?: boolean
        'max-options-visible'?: number
        size?: 'xs' | 's' | 'm' | 'l' | 'xl' | 'small' | 'medium' | 'large'
        appearance?: 'filled' | 'outlined' | 'filled-outlined'
        pill?: boolean
        disabled?: boolean
        required?: boolean
        placement?: 'top' | 'bottom'
        getTag?: (option: WaOptionElement, index: number) => string | HTMLElement
        onchange?: (event: Event) => void
        oninput?: (event: Event) => void
        'onwa-clear'?: (event: Event) => void
      }
      'wa-option': Omit<
        import('svelte/elements').HTMLAttributes<HTMLElement>,
        'value' | 'label'
      > & {
        value?: string
        label?: string
        selected?: boolean
        disabled?: boolean
      }
      'wa-switch': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        checked?: boolean
        disabled?: boolean
        size?: 's' | 'm' | 'l' | 'small' | 'medium' | 'large'
        'onwa-change'?: (event: Event) => void
      }
      'wa-avatar': Omit<import('svelte/elements').HTMLAttributes<HTMLElement>, 'label'> & {
        image?: string
        initials?: string
        label?: string
        loading?: 'eager' | 'lazy'
        shape?: 'circle' | 'square' | 'rounded'
      }
      'wa-tab-group': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        active?: string
        placement?: 'top' | 'bottom' | 'start' | 'end'
        activation?: 'auto' | 'manual'
        'without-scroll-controls'?: boolean
        'onwa-tab-show'?: (event: CustomEvent<{ name: string }>) => void
        'onwa-tab-hide'?: (event: CustomEvent<{ name: string }>) => void
      }
      'wa-tab': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        panel?: string
        active?: boolean
        disabled?: boolean
      }
      'wa-tab-panel': import('svelte/elements').HTMLAttributes<HTMLElement> & {
        name?: string
        active?: boolean
      }
    }
  }

  interface WaSelectElement extends HTMLElement {
    value: string | string[]
    getTag?: (option: WaOptionElement, index: number) => string | HTMLElement
  }

  interface WaSwitchElement extends HTMLElement {
    checked: boolean
  }

  interface WaOptionElement extends HTMLElement {
    value: string
    label: string
    selected: boolean
    disabled: boolean
  }
}

export {}
