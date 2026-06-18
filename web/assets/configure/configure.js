// TEMPLATE (the cloud-init text) is injected as a global by the layout.
const TEXT_FIELDS = ['server_url', 'server_name'];
const RELAY_URL = 'https://relay.theavalanche.net';
const INVITE_DOMAIN = 'go.theavalanche.net';
const GH_REPO = 'lincolnq/avalanche';
// Fallback when releases can't be fetched (offline, rate-limited, or none
// published yet). The dropdown is editable regardless.
const DEFAULT_RELEASE_TAG = '0.1.0';

// One high-entropy bootstrap secret per page load. Stable across re-renders so
// the env file and the invite link always agree. It gates registration while
// the server runs closed (the default) and auto-disables once a gatekeeper
// Project is installed (see docs/24).
const SHARED_SECRET = (() => {
  // 16 bytes = 128 bits, ample for a rate-limited bootstrap credential, and
  // keeps the invite token (and its QR) compact.
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return btoa(String.fromCharCode(...bytes))
    .replace(/=+$/, '').replace(/\+/g, '-').replace(/\//g, '_');
})();

function b64url(s) {
  return btoa(s).replace(/=+$/, '').replace(/\+/g, '-').replace(/\//g, '_');
}

function inviteUrl(serverUrl) {
  // base64url(JSON) with single-char keys (s=server_url, k=bootstrap_secret),
  // no padding — keeps the QR low-density. The secret lets people register
  // while the server is closed; share this link/QR to onboard your first
  // members, then install a gatekeeper to retire it.
  const payload = JSON.stringify({s: serverUrl, k: SHARED_SECRET});
  return `https://${INVITE_DOMAIN}/i/${b64url(payload)}`;
}

// Keep or drop a `#REGION:NAME:BEGIN ... #REGION:NAME:END` block in the
// template. When kept we drop just the two marker lines; when dropped we remove
// the whole block — so unselected bots leave no trace in the cloud-init.
function applyRegion(text, name, keep) {
  const re = new RegExp(
    `^[ \\t]*#REGION:${name}:BEGIN[^\\n]*\\n([\\s\\S]*?)^[ \\t]*#REGION:${name}:END[^\\n]*\\n`,
    'gm'
  );
  return text.replace(re, keep ? '$1' : '');
}

// True while the Server URL is empty, unparseable, or still an example
// placeholder — we keep the copy buttons disabled so nobody deploys with
// av.example.org baked in. Matches the hostname so a real domain that merely
// contains "example" (e.g. myexample.org) isn't caught.
function isPlaceholderServer(u) {
  const raw = (u || '').trim();
  if (!raw) return true;
  let host;
  try {
    host = new URL(raw.includes('://') ? raw : `https://${raw}`).hostname.toLowerCase();
  } catch {
    return true;
  }
  return host === 'example.org' || host === 'example.com'
    || host.endsWith('.example.org') || host.endsWith('.example.com');
}

function render() {
  const values = Object.fromEntries(
    TEXT_FIELDS.map(f => [f, document.getElementById(f).value.trim()])
  );
  const releaseTag = document.getElementById('release_tag').value.trim() || DEFAULT_RELEASE_TAG;
  const adminbot = document.getElementById('install_adminbot').checked;
  const testbot = document.getElementById('install_testbot').checked;
  const url = inviteUrl(values.server_url);

  let cloudinit = TEMPLATE;
  // NODE runtime is only needed when at least one bot is installed.
  cloudinit = applyRegion(cloudinit, 'NODE', adminbot || testbot);
  cloudinit = applyRegion(cloudinit, 'ADMINBOT', adminbot);
  cloudinit = applyRegion(cloudinit, 'TESTBOT', testbot);
  cloudinit = cloudinit
    // Release tag appears more than once (header + env), so replace globally.
    .replaceAll('__RELEASE_TAG__', releaseTag)
    .replace('__SERVER_URL__', values.server_url)
    .replace('__SERVER_NAME__', values.server_name)
    .replace('__RELAY_URL__', RELAY_URL)
    .replace('__REGISTRATION_SHARED_SECRET__', SHARED_SECRET)
    .replace('__INVITE_URL__', url);
  document.getElementById('cloudinit_out').value = cloudinit;

  const link = document.getElementById('invite_link');
  link.href = url;
  link.textContent = url;

  const qr = qrcode(0, 'M');
  qr.addData(url);
  qr.make();
  document.getElementById('qr').innerHTML = qr.createSvgTag({cellSize: 5, margin: 2});

  // Both copy buttons embed the Server URL, so gate them on a real domain.
  const placeholder = isPlaceholderServer(values.server_url);
  document.getElementById('copy_cloudinit').disabled = placeholder;
  document.getElementById('copy_invite').disabled = placeholder;
  document.getElementById('copy_cloudinit_status').textContent =
    placeholder ? 'Enter your real Server URL above' : '';
}

function wireCopy(btnId, statusId, getValue) {
  document.getElementById(btnId).addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(getValue());
      const s = document.getElementById(statusId);
      s.textContent = 'copied';
      setTimeout(() => { s.textContent = ''; }, 1500);
    } catch (e) {
      alert('Copy failed: ' + e.message);
    }
  });
}

// Populate the release dropdown from the last few GitHub releases. We use the
// `/releases` LIST endpoint, not `releases/latest`: the latter excludes drafts
// AND prereleases (404s when the newest release is either). The list, for an
// unauthenticated caller, returns published releases (prereleases included)
// newest-first. (api.github.com sends permissive CORS headers.)
async function loadReleases() {
  const el = document.getElementById('release_tag');
  const status = document.getElementById('release_tag_status');
  try {
    const r = await fetch(`https://api.github.com/repos/${GH_REPO}/releases?per_page=10`, {
      headers: {Accept: 'application/vnd.github+json'},
    });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const tags = (await r.json())
      .filter(rel => !rel.draft)
      .map(rel => rel.tag_name)
      .filter(Boolean);
    if (tags.length) {
      el.innerHTML = '';
      for (const t of tags) {
        const o = document.createElement('option');
        o.value = t;
        o.textContent = t;
        el.appendChild(o);
      }
      el.value = tags[0]; // newest
      render();
      if (status) status.textContent = '';
    } else if (status) {
      status.textContent = `no releases found — using ${el.value}`;
    }
  } catch (e) {
    if (status) status.textContent = `couldn't reach GitHub — using ${el.value}`;
  }
}

TEXT_FIELDS.forEach(f => {
  document.getElementById(f).addEventListener('input', render);
});
['release_tag', 'install_adminbot', 'install_testbot'].forEach(id => {
  document.getElementById(id).addEventListener('change', render);
});
wireCopy('copy_cloudinit', 'copy_cloudinit_status',
         () => document.getElementById('cloudinit_out').value);
wireCopy('copy_invite', 'copy_invite_status',
         () => document.getElementById('invite_link').href);
render();
loadReleases();
