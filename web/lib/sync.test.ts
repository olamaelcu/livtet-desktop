import { describe, expect, test } from 'vitest'
import { peerEndpoint, syncErrorMessage } from '$lib/sync'

describe('syncErrorMessage', () => {
  test('reports the message from an Unavailable error', () => {
    expect(syncErrorMessage({ Unavailable: { message: 'daemon not running' } })).toBe(
      'daemon not running',
    )
  })

  test('reports the message from an Rpc error', () => {
    expect(syncErrorMessage({ Rpc: { message: 'method not found' } })).toBe('method not found')
  })

  test('reports the message from a Protocol error', () => {
    expect(syncErrorMessage({ Protocol: { message: 'unexpected payload' } })).toBe(
      'unexpected payload',
    )
  })

  test('reports the message from a Storage error', () => {
    expect(syncErrorMessage({ Storage: { message: 'disk full' } })).toBe('disk full')
  })

  test('falls back to the message of a plain Error', () => {
    expect(syncErrorMessage(new Error('socket closed'))).toBe('socket closed')
  })

  test('stringifies anything else', () => {
    expect(syncErrorMessage('plain string')).toBe('plain string')
    expect(syncErrorMessage(42)).toBe('42')
    expect(syncErrorMessage(null)).toBe('null')
  })

  // The guard accepts any object carrying one of the variant keys, but each
  // branch additionally requires the value to be truthy. A variant present but
  // empty therefore falls past all of them and lands on String(error), which
  // renders as "[object Object]" — no use in a toast.
  test('renders an object shape with an empty variant as [object Object]', () => {
    expect(syncErrorMessage({ Rpc: null })).toBe('[object Object]')
  })
})

describe('peerEndpoint', () => {
  function peer(addresses: string[], port = 7710, hostname = 'livtet-abc.local') {
    return {
      node_id: 'node-1',
      device_id: 'dev-1',
      name: 'Laptop',
      device_type: 'desktop',
      version: '0.1.0',
      hostname,
      addresses,
      port,
    }
  }

  test('prefers a routable address over loopback and link-local ones', () => {
    const endpoint = peerEndpoint(peer(['127.0.0.1', '169.254.10.4', '192.168.1.137']))

    expect(endpoint).toBe('192.168.1.137:7710')
  })

  test('brackets an IPv6 address so the request parses', () => {
    const endpoint = peerEndpoint(peer(['fe80::1c2b', '2001:db8::5']))

    expect(endpoint).toBe('[2001:db8::5]:7710')
  })

  test('falls back to the advertised addresses when none is routable', () => {
    expect(peerEndpoint(peer(['127.0.0.1']))).toBe('127.0.0.1:7710')
  })

  test('falls back to the hostname when the peer advertised no address', () => {
    expect(peerEndpoint(peer([]))).toBe('livtet-abc.local:7710')
  })
})
