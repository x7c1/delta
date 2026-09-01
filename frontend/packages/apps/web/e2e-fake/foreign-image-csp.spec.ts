import { test, expect } from './support/fixtures';
import { startNewSession } from './support/app';

/**
 * Assistant markdown is attacker-influenceable, so an external image reference
 * in it (`![](https://example.invalid/x.png)`) must NOT cause the browser to
 * reach out to the foreign host — such a request would be an exfiltration
 * channel (its path/query could carry data). The app's Content-Security-Policy
 * (`img-src 'self' data:`, set as a <meta> in index.html and mirrored as a Vite
 * dev response header) makes the real browser block the load.
 *
 * Proving the block needs the right signal. react-markdown emits a real
 * `<img src>` into the DOM, and Chromium/Playwright still surface a
 * `request` + `requestfailed` for a CSP-blocked resource — so "no request
 * event fired" is NOT a reliable discriminator. `example.invalid` also never
 * resolves, so "no response arrived" would be true even with the CSP disabled.
 * The deterministic proof is the browser's own `securitypolicyviolation`
 * event: it fires (with the `img-src` directive and the foreign blocked URI)
 * exactly when the CSP stops the fetch. We assert that fired, that no response
 * ever came back from the host, and that the request never escaped to the
 * network (a network attempt would fail with a DNS error rather than a CSP
 * block).
 *
 * Scenario `foreign-image`: the fake replies with markdown containing the
 * foreign image, then stops.
 */
test('a foreign image referenced by assistant markdown is blocked by the CSP and never fetched', async ({
  page,
}) => {
  // Record every CSP violation the browser reports, before any app code runs.
  await page.addInitScript(() => {
    const store: Array<{ directive: string; blockedURI: string }> = [];
    (window as unknown as { __cspViolations: typeof store }).__cspViolations =
      store;
    document.addEventListener('securitypolicyviolation', (event) => {
      store.push({
        directive: event.effectiveDirective || event.violatedDirective,
        blockedURI: event.blockedURI,
      });
    });
  });

  // A real exfiltration would complete a response from the host; the security
  // property is that none ever does.
  const foreignResponses: string[] = [];
  page.on('response', (response) => {
    if (response.url().includes('example.invalid')) {
      foreignResponses.push(response.url());
    }
  });
  // If the request somehow escaped the CSP and reached the network, it would
  // fail with a DNS error (example.invalid is unresolvable) rather than a CSP
  // block — that failure text is how we tell "blocked before dispatch" from
  // "dispatched and leaked a DNS lookup for the host".
  const foreignFailures: string[] = [];
  page.on('requestfailed', (request) => {
    if (request.url().includes('example.invalid')) {
      foreignFailures.push(request.failure()?.errorText ?? '');
    }
  });

  await page.goto('/');
  await startNewSession(page, 'foreign-image please render this');

  // The assistant reply is persisted; its markdown renders a real <img> whose
  // src points at the foreign host.
  await expect(page.getByText('End of message.', { exact: true })).toBeVisible();
  const img = page.locator('img[src="https://example.invalid/x.png"]');
  await expect(img).toHaveCount(1);

  // The CSP reported the block for the foreign image — the direct proof that
  // `img-src 'self' data:` is enforced by the real browser.
  await expect
    .poll(
      () =>
        page.evaluate(
          () =>
            (
              window as unknown as {
                __cspViolations: Array<{ directive: string; blockedURI: string }>;
              }
            ).__cspViolations,
        ),
      { timeout: 15_000 },
    )
    .toContainEqual(
      expect.objectContaining({
        blockedURI: expect.stringContaining('example.invalid'),
      }),
    );

  // No data left the browser: no response ever came back from the host, and the
  // request never reached the network (any failure must be the CSP block, not a
  // DNS lookup that would prove the host was actually contacted).
  expect(foreignResponses).toEqual([]);
  for (const errorText of foreignFailures) {
    expect(errorText).not.toContain('NAME_NOT_RESOLVED');
  }
});
