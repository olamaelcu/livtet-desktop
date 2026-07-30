<script>
  import "@awesome.me/webawesome/dist/styles/webawesome.css";
  import "../app.wa.js";
  import "../app.css";

  let { children } = $props();
</script>

<wa-page>
  <nav class="main-header">
  </nav>
  <main>
    {@render children()}
  </main>
</wa-page>

<style>

</style>
