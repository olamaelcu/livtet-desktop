
<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import { atprotoKeys } from '$lib/query/keys'
import { atprotoStatus } from '../atproto'
import type { NavItem } from './navItems'

const STORAGE_KEY = 'livtet.layout.toggleIcons'

function loadToggleIcons(): boolean {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    return stored !== null ? stored === 'true' : false
  } catch {
    return false
  }
}

function saveToggleIcons(value: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(value))
  } catch {
    return
  }
}
interface Props {
  items: readonly NavItem[]
  currentPath: string
}

let { items, currentPath }: Props = $props()

/** Icons-only rail state, persisted so it survives route changes and restarts. */
let collapsed = $state(loadCollapsed())

function toggleRail() {
  collapsed = !collapsed
  saveCollapsed(collapsed)
}

const status = createQuery(() => ({
  queryKey: atprotoKeys.status(),
  queryFn: atprotoStatus,
}))

function isActive(href: string) {
  return currentPath === href || currentPath.startsWith(`${href}/`)
}

/**
 * wa-button does not forward `aria-current`; the real link is in its shadow
 * root. Set it there so the state reaches the accessibility tree.
 */
function currentOnLink(current: boolean) {
  return (node: Element) => {
    let live = true
    customElements.whenDefined('wa-button').then(async () => {
      await (node as HTMLElement & { updateComplete?: Promise<unknown> }).updateComplete
      const link = node.shadowRoot?.querySelector('a')
      if (!live || !link) return
      if (current) link.setAttribute('aria-current', 'page')
      else link.removeAttribute('aria-current')
    })
    return () => {
      live = false
    }
  }
}
</script>

<nav
  class="rail"
  aria-label="Primary"
  data-expanded={expanded ? '' : undefined}
  class:collapsed={!expanded || toggleIcons}
>
  {#each items as item (item.href)}
    <wa-button
      href={item.href}
      aria-current={isActive(item.href) ? 'page' : undefined}
      {@attach currentOnLink(isActive(item.href))}
      variant={isActive(item.href) ? 'brand' : 'neutral'}
      appearance={isActive(item.href) ? 'filled' : 'plain'}
      size="m"
    >
      <wa-icon name={item.icon} slot={toggleIcons ? "start":""}></wa-icon>
      <span class="rail__label">{item.label}</span>
    </wa-button>
  {/each}
  <div class="spacer"></div>
  {#if status.data}
    <wa-button href="/settings?tab=account" appearance="filled" variant="brand" size="m">
      <img
        class="nav-avatar"
        src={status.data ? `https://livtet.olamaelcu.net/user/profileImage/${status.data.did}` : ''}
        alt="Profile"
      />
      {#if toggleIcons}<span class="rail__label">Profile</span>{/if}
    </wa-button>
    <wa-button class="rail-menu" appearance="plain" onclick={() => { toggleIcons = !toggleIcons; saveToggleIcons(toggleIcons); }}>
      <wa-icon name="bars"></wa-icon>
      <span class="rail__label">Collapse</span>
    </wa-button>
  {/if}
</nav>

<style>
  .rail {
    --app-rail-width: 5rem;
    --app-rail-width-expanded: 11rem;
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xs);
    padding: var(--wa-space-xs);
    box-sizing: border-box;
    inline-size: var(--app-rail-width);
    min-height: 0;
    overflow: hidden;
    border-inline-end: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-surface-border);
  }

  .spacer { flex: 1 1;}

  .nav-avatar { width: 20px; height: 20px; border-radius: 50%; object-fit: cover; flex: none; vertical-align: middle; }

  .rail.collapsed {
      inline-size: var(--app-rail-width);
    }

  /*
   * Collapsed labels are visually hidden, not display: none: they are the only
   * accessible name of the inner link (wa-button drops a host aria-label).
   */
  .rail.collapsed .rail__label {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    vertical-align: middle;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .rail:not(.collapsed) {
    inline-size: var(--app-rail-width-expanded);
  }

  @container (width < 46rem) {
    .rail,
    .nav-avatar { width: 20px; height: 20px; border-radius: 50%; object-fit: cover; flex: none; }

    .rail {
      inline-size: var(--app-rail-width);
    }

    .rail .rail__label {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    .rail:not(.collapsed) {
      inline-size: var(--app-rail-width);
    }
  }

  .rail-menu { margin: 0 auto;}
</style>
