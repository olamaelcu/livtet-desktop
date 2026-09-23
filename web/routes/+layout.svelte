<script>
import '../app.css'
import '../app.wa.js'

import { Toaster } from 'svelte-sonner'
import { page } from '$app/state'

let { children } = $props()

let pageTitle = $derived(page.data.pageTitle ?? 'Livtet')
</script>

<wa-page>
  <header slot="header">
    <span>{pageTitle}</span>
    <wa-button-group>
      <wa-button href="/" variant="brand">
        <wa-icon name="home"></wa-icon>
        Livtet
      </wa-button>
      <wa-button href="/">
        <wa-icon name="home"></wa-icon>
        Help
      </wa-button>
    </wa-button-group>
  </header>
  <aside  slot="navigation">
    <wa-button-group orientation="vertical" label="Navigation">
      <wa-button href="/library">
        Library
      </wa-button>
      <wa-button href="/settings">
        Settings
      </wa-button>
    </wa-button-group>
  </aside>
  {@render children?.()}
</wa-page>

<Toaster theme="system" />

<style>
  aside {
    padding-top: var(--wa-space-xs);
  }
  header { 
    > span {
      flex:1 1;
    }
  }
</style>
