<script lang="ts">
import RootLayout from '$routes/+layout.svelte'
</script>

<!-- The layout renders its own query provider and app frame, so it needs only
     a children snippet to mount. -->
<RootLayout>
  {#snippet children()}
    <p data-testid="page-content">routed page</p>
  {/snippet}
</RootLayout>
