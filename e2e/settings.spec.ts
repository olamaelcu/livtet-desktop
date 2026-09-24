import { expect, test } from './fixtures';

test.describe('Settings', () => {
  test.use({ mode: 'browser' });

  test('renders sync status from the daemon', async ({ tauriPage }) => {
    await tauriPage.goto('/settings');
    await expect(tauriPage.getByText(/Daemon:/)).toBeVisible();
    await expect(tauriPage.getByText('Pixel 9')).toBeVisible();
  });

  test('lists every keyboard shortcut', async ({ tauriPage }) => {
    await tauriPage.goto('/settings');
    const labels = [
      'Open command palette',
      'Focus search',
      'Go to Library',
      'Go to Settings',
      'Refresh data',
    ];
    for (const label of labels) {
      await expect(tauriPage.getByText(label, { exact: true }).first()).toBeVisible();
    }
  });

  test('shows a pairing QR code and credentials after pairing a device', async ({
    tauriPage,
  }) => {
    await tauriPage.goto('/settings');
    await tauriPage.getByText('Pair a device', { exact: true }).click();

    await expect(tauriPage.locator('wa-qr-code.pairing-qr')).toBeVisible();

    const pairing = await tauriPage.evaluate<{
      qrValue: string;
      qrLabel: string;
      uri: string;
      token: string;
      expires: boolean;
    }>(`(() => {
      const read = (el) => (el ? (el.value ?? el.getAttribute('value') ?? '') : 'missing');
      const qr = document.querySelector('wa-qr-code.pairing-qr');
      const inputs = Array.from(document.querySelectorAll('wa-input'));
      const byLabel = (label) => {
        const el = inputs.find((node) => (node.label ?? node.getAttribute('label')) === label);
        return read(el);
      };
      return {
        qrValue: read(qr),
        qrLabel: qr ? (qr.label ?? qr.getAttribute('label') ?? '') : '',
        uri: byLabel('Pairing URI'),
        token: byLabel('Pairing token'),
        expires: document.body.innerText.includes('Expires'),
      };
    })()`);

    expect(pairing.qrValue).toBe('livtet://pair?token=PAIR-TOKEN-123');
    expect(pairing.qrLabel).toBe('Scan to pair a device');
    expect(pairing.uri).toBe('livtet://pair?token=PAIR-TOKEN-123');
    expect(pairing.token).toBe('PAIR-TOKEN-123');
    expect(pairing.expires).toBe(true);
  });
});
