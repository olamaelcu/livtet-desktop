#!/usr/bin/env node
// Smoke test for the incoming-book offer path.
//
// Default (headless): spawns the real sidecar on a scratch database, stands up
// a stub peer whose change feed carries two offered books, pulls from it, then
// walks the exact RPC calls the dialog makes — `offers.list`, `offers.cover`,
// `offers.decide` — and asserts what the reader and the sender each end up
// with. Nothing is seeded except the pairing row: the offers are recorded by
// the same `record_inbound_offer` a live pull runs, and the same
// `offer.received` notification the app turns into `sync://offer-received`
// fires.
//
// `--app-db <path>`: seeds one pending offer straight into a *running* app's
// database and serves the peer, so `<wa-dialog label="Book offered">` can be
// asserted through the Tauri MCP bridge. `offers.list` polls every 30s, so the
// prompt appears within half a minute. Seeded rows and the downloaded file are
// removed on exit either way.
//
// What this covers that the browser tests do not: the daemon's half of the
// protocol over real HTTP (pull, cover fetch, file download, decision POST) and
// the row transitions behind the prompt. What it cannot cover headlessly is the
// webview itself — use `--app-db` for that.

import { execFileSync, spawn } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'
import readline from 'node:readline'

const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'
const BOOK_BYTES = Buffer.from('offered-book-bytes')
/** 1x1 PNG; the daemon passes covers through and only the MIME type matters. */
const COVER_BYTES = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==',
  'base64',
)
const ACCEPTED_OFFER = 'offer-accept-smoke'
const DECLINED_OFFER = 'offer-decline-smoke'
const CHANGED_AT = '2026-11-01 09:00:00'

/** A fresh DbId, as both the ULID string the wire uses and its blob key. */
function newDbId() {
  const bytes = Buffer.alloc(16)
  let millis = BigInt(Date.now())
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = Number(millis & 0xffn)
    millis >>= 8n
  }
  crypto.randomFillSync(bytes, 6, 10)

  let value = 0n
  for (const byte of bytes) value = (value << 8n) | BigInt(byte)
  let text = ''
  for (let index = 0; index < 26; index += 1) {
    text = CROCKFORD[Number(value & 31n)] + text
    value >>= 5n
  }
  return { text, hex: bytes.toString('hex') }
}

function parseArgs(argv) {
  const options = {
    appDb: null,
    booksDir: null,
    coversDir: null,
    daemon: path.join(process.cwd(), 'target/debug/livtet-sync-daemon'),
    timeoutMs: 120_000,
  }
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index]
    const value = argv[index + 1]
    if (flag === '--app-db') {
      options.appDb = path.resolve(value)
      index += 1
    } else if (flag === '--books-dir') {
      options.booksDir = path.resolve(value)
      index += 1
    } else if (flag === '--covers-dir') {
      options.coversDir = path.resolve(value)
      index += 1
    } else if (flag === '--daemon') {
      options.daemon = path.resolve(value)
      index += 1
    } else if (flag === '--timeout') {
      options.timeoutMs = Number(value) * 1000
      index += 1
    } else if (flag === '-h' || flag === '--help') {
      console.log(
        'usage: smoke-incoming-offer.mjs [--app-db PATH] [--books-dir PATH] ' +
          '[--covers-dir PATH] [--daemon PATH] [--timeout SECONDS]',
      )
      process.exit(0)
    } else {
      console.error(`unknown argument: ${flag}`)
      process.exit(2)
    }
  }
  return options
}

const failures = []
const checks = []

function check(ok, description, detail) {
  checks.push({ ok, description })
  if (ok) {
    console.log(`  ok   ${description}`)
  } else {
    console.log(`  FAIL ${description}${detail === undefined ? '' : `: ${detail}`}`)
    failures.push(description)
  }
}

function sql(dbPath, statement) {
  return execFileSync('sqlite3', [dbPath], { input: statement, encoding: 'utf8' }).trim()
}

function readOffer(dbPath, offerId) {
  const [status, decided] = sql(
    dbPath,
    `SELECT status, (decided_at IS NOT NULL) FROM inbound_offers WHERE offer_id = '${offerId}';`,
  ).split('|')
  return { status, decided: decided === '1' }
}

/** The stub peer: serves a change feed, one book, one cover; records decisions. */
async function startPeer() {
  const seen = { files: [], covers: [], decisions: [], other: [], pulls: 0 }
  /** The page every `/sync/changes` call returns; filled in once ids exist. */
  const feed = { changes: [], latest_version: 0 }

  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? '/', 'http://stub')
    let body = ''
    request.on('data', (chunk) => {
      body += chunk
    })
    request.on('end', () => {
      const fileMatch = url.pathname.match(/^\/sync\/files\/(.+)$/)
      const coverMatch = url.pathname.match(/^\/sync\/covers\/(.+)$/)
      const decisionMatch = url.pathname.match(/^\/sync\/offers\/(.+)\/decision$/)

      if (fileMatch) {
        seen.files.push(fileMatch[1])
        response.writeHead(200, { 'content-type': 'application/epub+zip' })
        response.end(BOOK_BYTES)
      } else if (coverMatch) {
        seen.covers.push(coverMatch[1])
        response.writeHead(200, { 'content-type': 'image/png' })
        response.end(COVER_BYTES)
      } else if (decisionMatch && request.method === 'POST') {
        let accepted = false
        try {
          accepted = JSON.parse(body).accepted === true
        } catch {
          accepted = false
        }
        seen.decisions.push({ offerId: decisionMatch[1], accepted })
        response.writeHead(200, { 'content-type': 'application/json' })
        response.end(JSON.stringify({ ok: true }))
      } else if (url.pathname === '/sync/changes') {
        seen.pulls += 1
        response.writeHead(200, { 'content-type': 'application/json' })
        response.end(
          JSON.stringify({
            changes: feed.changes,
            has_more: false,
            latest_version: feed.latest_version,
          }),
        )
      } else {
        seen.other.push(`${request.method} ${url.pathname}`)
        response.writeHead(200, { 'content-type': 'application/json' })
        response.end('{}')
      }
    })
  })

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  return {
    seen,
    feed,
    address: `127.0.0.1:${server.address().port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

/**
 * The change rows a sender appends when it offers a book: the `file_offer`
 * itself plus the catalog rows that describe it, which the pull must hold back.
 */
function offerRows({ offerId, version, toDeviceId, fromDeviceId, title, ids }) {
  const payload = {
    offer_id: offerId,
    inventory_id: ids.inventory,
    edition_id: ids.edition,
    work_id: ids.work,
    to_device_id: toDeviceId,
    title,
    authors: ['Ada Lovelace'],
    file_name: 'offered.epub',
    file_size_bytes: BOOK_BYTES.length,
    has_cover: true,
  }
  return [
    {
      id: version,
      entity_type: 'file_offer',
      entity_id: ids.inventory,
      operation: 'OFFER',
      version,
      payload: JSON.stringify(payload),
      changed_at: CHANGED_AT,
      device_id: fromDeviceId,
    },
    {
      id: version + 1,
      entity_type: 'work',
      entity_id: ids.work,
      operation: 'UPDATE',
      version: version + 1,
      payload: JSON.stringify({ id: ids.work, title, created_at: CHANGED_AT }),
      changed_at: CHANGED_AT,
      device_id: fromDeviceId,
    },
  ]
}

/** The pairing row a completed pairing leaves behind (see `pull_device`). */
function seedPeer(dbPath, { deviceId, name, address }) {
  sql(
    dbPath,
    `INSERT INTO paired_devices (
       device_id, name, listen_on, device_type_id, paired_at, last_sync_at,
       session_token, outbound
     ) VALUES (X'${deviceId.hex}', '${name}', '${address}', NULL, datetime('now'), NULL,
       'smoke-token', 1);`,
  )
}

/** The row a pull writes when it records an offer, for `--app-db` mode. */
function seedOffer(dbPath, { fromDeviceId, title, offerId, ids, hasCover = true }) {
  sql(
    dbPath,
    `INSERT INTO inbound_offers (
       offer_id, from_device_id, inventory_id, edition_id, work_id, title, authors,
       file_name, has_cover, offered_at, received_at, status, deferred_changes
     ) VALUES (
       '${offerId}', '${fromDeviceId}', '${ids.inventory}', '${ids.edition}', '${ids.work}',
       '${title}', '["Ada Lovelace"]', 'offered.epub', ${hasCover ? 1 : 0},
       '${CHANGED_AT}', '${CHANGED_AT}', 'pending', '[]'
     );`,
  )
}

/** NDJSON JSON-RPC over the daemon's stdio, matching `rpc::StdioClient`. */
function rpcClient(child) {
  const pending = new Map()
  const notifications = []
  let nextId = 1

  readline.createInterface({ input: child.stdout }).on('line', (line) => {
    let message
    try {
      message = JSON.parse(line)
    } catch {
      return
    }
    if (message.id != null && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id)
      pending.delete(message.id)
      if (message.error) reject(new Error(`${message.error.code}: ${message.error.message}`))
      else resolve(message.result)
    } else if (message.id == null && message.method) {
      notifications.push(message)
    }
  })

  return {
    notifications,
    request(method, params = {}) {
      const id = nextId
      nextId += 1
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject })
        child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`)
        setTimeout(() => {
          if (pending.delete(id)) reject(new Error(`${method}: no response in 60s`))
        }, 60_000).unref()
      })
    },
    async status(timeoutMs) {
      const deadline = Date.now() + timeoutMs
      for (;;) {
        try {
          return await this.request('status')
        } catch (error) {
          if (Date.now() > deadline) throw error
          await new Promise((resolve) => setTimeout(resolve, 250))
        }
      }
    },
  }
}

function assertOffers(list, expected) {
  const offers = Array.isArray(list) ? list : []
  check(
    offers.length === expected.length,
    `offers.list reports ${expected.length} pending offer(s)`,
    `got ${offers.length}`,
  )
  for (const entry of expected) {
    const offer = offers.find((candidate) => candidate.offer_id === entry.offerId)
    check(Boolean(offer), `offers.list includes ${entry.offerId}`)
    if (!offer) continue
    check(offer.title === entry.title, `title is "${entry.title}"`, offer.title)
    check(
      JSON.stringify(offer.authors) === JSON.stringify(['Ada Lovelace']),
      'authors travel with the offer',
      JSON.stringify(offer.authors),
    )
    check(offer.from_name === 'Smoke Peer', 'sender name is shown', String(offer.from_name))
    check(offer.has_cover === true, 'offer advertises a cover')
  }
}

async function headless(options) {
  if (!fs.existsSync(options.daemon)) {
    throw new Error(
      `missing daemon binary at ${options.daemon}; build it with \`cargo build -p livtet-sync-daemon\``,
    )
  }

  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'livtet-offer-smoke-'))
  const booksDir = path.join(root, 'books')
  const coversDir = path.join(root, 'covers')
  const dbPath = path.join(root, 'livtet.db')
  for (const dir of [booksDir, coversDir]) fs.mkdirSync(dir, { recursive: true })

  const peer = await startPeer()
  const daemon = spawn(
    options.daemon,
    [
      '--db',
      dbPath,
      '--host',
      '127.0.0.1',
      '--port',
      '0',
      '--books-dir',
      booksDir,
      '--covers-dir',
      coversDir,
    ],
    {
      cwd: root,
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        RUST_LOG: process.env.RUST_LOG ?? 'warn',
        LIVTET_DISCOVERY_KEY: '01'.repeat(32),
      },
    },
  )
  const stderr = []
  daemon.stderr.on('data', (chunk) => stderr.push(String(chunk)))

  try {
    const rpc = rpcClient(daemon)
    const status = await rpc.status(30_000)
    check(
      typeof status.device_id === 'string' && status.device_id.length === 26,
      'daemon reports a device id',
      status.device_id,
    )

    const sender = newDbId()
    const accepted = { inventory: newDbId().hex, edition: newDbId().hex, work: newDbId().hex }
    const declined = { inventory: newDbId().hex, edition: newDbId().hex, work: newDbId().hex }

    peer.feed.changes = [
      ...offerRows({
        offerId: ACCEPTED_OFFER,
        version: 1,
        toDeviceId: status.device_id,
        fromDeviceId: sender.text,
        title: 'Held Book',
        ids: accepted,
      }),
      ...offerRows({
        offerId: DECLINED_OFFER,
        version: 3,
        toDeviceId: status.device_id,
        fromDeviceId: sender.text,
        title: 'Unwanted Book',
        ids: declined,
      }),
    ]
    peer.feed.latest_version = 4
    seedPeer(dbPath, { deviceId: sender, name: 'Smoke Peer', address: peer.address })

    console.log('pull records the offer instead of taking it:')
    const pulled = await rpc.request('devices.pull', { device_id: sender.text })
    check(peer.seen.pulls >= 1, 'daemon pulled the change feed')
    check(pulled.offered === 2, 'both offers were recorded', String(pulled.offered))
    check(pulled.applied === 0, 'no catalog row was applied on pull', String(pulled.applied))
    check(
      sql(dbPath, "SELECT count(*) FROM inbound_offers WHERE status = 'pending';") === '2',
      'two offers wait as pending',
    )
    const received = rpc.notifications.filter((entry) => entry.method === 'offer.received')
    check(received.length === 2, 'offer.received fired per offer', String(received.length))
    check(
      received.some((entry) => entry.params?.title === 'Held Book'),
      'the notification carries the title the prompt shows',
      JSON.stringify(received.map((entry) => entry.params?.title)),
    )

    console.log('what the dialog reads:')
    assertOffers(await rpc.request('offers.list'), [
      { offerId: ACCEPTED_OFFER, title: 'Held Book' },
      { offerId: DECLINED_OFFER, title: 'Unwanted Book' },
    ])
    const cover = await rpc.request('offers.cover', { offer_id: ACCEPTED_OFFER })
    check(
      typeof cover?.data_url === 'string' && cover.data_url.startsWith('data:image/png;base64,'),
      'offers.cover hands the dialog a data URL',
      cover?.data_url?.slice(0, 32),
    )
    check(peer.seen.covers.includes(accepted.inventory), 'cover came from the sender over HTTP')

    console.log('accepting:')
    const taken = await rpc.request('offers.decide', { offer_id: ACCEPTED_OFFER, accepted: true })
    check(taken.accepted === true, 'daemon reports the book as accepted')
    check(taken.delivered === true, 'daemon reached the sender', String(taken.delivered))
    check(
      peer.seen.decisions.some((entry) => entry.offerId === ACCEPTED_OFFER && entry.accepted),
      'sender received the acceptance',
      JSON.stringify(peer.seen.decisions),
    )
    check(peer.seen.files.includes(accepted.inventory), 'file downloaded from the sender')
    const landed = path.join(booksDir, `${accepted.inventory}-offered.epub`)
    check(fs.existsSync(landed), 'accepted book is on disk', landed)
    check(
      fs.existsSync(landed) && fs.readFileSync(landed).equals(BOOK_BYTES),
      'downloaded bytes are the sender’s',
    )
    check(taken.applied >= 1, 'the held catalog rows landed with the book', String(taken.applied))
    const acceptedRow = readOffer(dbPath, ACCEPTED_OFFER)
    check(acceptedRow.status === 'accepted', 'offer row marked accepted', acceptedRow.status)

    console.log('declining:')
    const refused = await rpc.request('offers.decide', {
      offer_id: DECLINED_OFFER,
      accepted: false,
    })
    check(refused.accepted === false, 'daemon reports the book as declined')
    check(refused.delivered === true, 'daemon reached the sender with the refusal')
    check(
      peer.seen.decisions.some((entry) => entry.offerId === DECLINED_OFFER && !entry.accepted),
      'sender received the refusal',
      JSON.stringify(peer.seen.decisions),
    )
    check(!peer.seen.files.includes(declined.inventory), 'declined book was never downloaded')
    check(
      !fs.existsSync(path.join(booksDir, `${declined.inventory}-offered.epub`)),
      'declined book never reaches disk',
    )
    check(refused.applied === 0, 'declining applies no catalog row', String(refused.applied))
    const declinedRow = readOffer(dbPath, DECLINED_OFFER)
    check(declinedRow.status === 'rejected', 'offer row marked rejected', declinedRow.status)
    check(declinedRow.decided === true, 'rejected offer records when it was decided')

    console.log('afterwards:')
    check((await rpc.request('offers.list')).length === 0, 'no pending offers remain')
    check(
      peer.seen.decisions.length === 2,
      'the sender was told about each offer exactly once',
      String(peer.seen.decisions.length),
    )
  } catch (error) {
    check(false, 'smoke run completed', error instanceof Error ? error.message : String(error))
    if (stderr.length) console.error(stderr.join('').trim())
  } finally {
    try {
      daemon.stdin.end()
    } catch {}
    await peer.close()
    daemon.kill('SIGTERM')
    fs.rmSync(root, { recursive: true, force: true })
  }
}

async function attach(options) {
  if (!options.appDb || !fs.existsSync(options.appDb)) {
    throw new Error(`--app-db ${options.appDb} does not exist`)
  }
  const pending = sql(
    options.appDb,
    "SELECT count(*) FROM inbound_offers WHERE status = 'pending';",
  )
  check(pending === '0', 'the app has no pending offers before seeding', pending)

  const peer = await startPeer()
  const sender = newDbId()
  const ids = { inventory: newDbId().hex, edition: newDbId().hex, work: newDbId().hex }
  seedPeer(options.appDb, { deviceId: sender, name: 'Smoke Peer', address: peer.address })
  seedOffer(options.appDb, {
    fromDeviceId: sender.text,
    title: 'Smoke Offered Book',
    offerId: ACCEPTED_OFFER,
    ids,
  })

  const booksDir = options.booksDir ?? path.join(path.dirname(options.appDb), 'books')
  const landed = path.join(booksDir, `${ids.inventory}-offered.epub`)

  console.log(
    [
      '',
      `seeded ${ACCEPTED_OFFER} from stub peer ${peer.address}`,
      'the dialog polls every 30s, so <wa-dialog label="Book offered"> opens within 30s',
      'drive it with the Tauri MCP bridge: webview_wait_for → webview_screenshot → webview_interact',
      `waiting up to ${Math.round(options.timeoutMs / 1000)}s for a decision…`,
    ].join('\n'),
  )

  const started = Date.now()
  let decided = null
  while (Date.now() - started < options.timeoutMs && !decided) {
    decided = peer.seen.decisions[0] ?? null
    if (!decided) await new Promise((resolve) => setTimeout(resolve, 500))
  }

  try {
    if (!decided) {
      check(false, 'the reader answered the offer', 'no decision POST reached the stub peer')
    } else {
      check(decided.accepted === true, 'reader accepted the offer', String(decided.accepted))
      check(
        peer.seen.files.includes(ids.inventory),
        'accepting downloaded the file from the sender',
        JSON.stringify(peer.seen.files),
      )
      check(fs.existsSync(landed), 'accepted book is in the app’s books dir', landed)
      const row = readOffer(options.appDb, ACCEPTED_OFFER)
      check(
        row.status === (decided.accepted ? 'accepted' : 'rejected'),
        'offer row matches the answer',
        row.status,
      )
    }
  } finally {
    await peer.close()
    // Leave the app's library exactly as it was found.
    sql(
      options.appDb,
      `DELETE FROM inbound_offers WHERE offer_id = '${ACCEPTED_OFFER}';
       DELETE FROM paired_devices WHERE device_id = X'${sender.hex}';`,
    )
    fs.rmSync(landed, { force: true })
  }
}

const options = parseArgs(process.argv.slice(2))
if (options.appDb) await attach(options)
else await headless(options)

const passed = checks.length - failures.length
console.log(`\n${passed}/${checks.length} checks passed`)
if (failures.length) {
  console.error(`failed: ${failures.join('; ')}`)
  process.exit(1)
}
