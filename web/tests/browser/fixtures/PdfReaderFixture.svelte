<script lang="ts">
// Renders the real PDF reader route inside the app frame, so the layout
// contract is asserted against the page itself rather than a hand-copy of it.
// pdf.js and the Tauri IPC are mocked by the test.
import AppFrame from '../../../lib/layout/AppFrame.svelte'
import PdfReaderPage from '../../../routes/reader/pdf/[editionId]/+page.svelte'
</script>

<AppFrame>
  <PdfReaderPage />
</AppFrame>
