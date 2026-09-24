import { describe, expect, it } from 'vitest'
import { COMMAND_BY_ID, COMMANDS } from './commands'

describe('command registry', () => {
  it('indexes every command by id', () => {
    for (const command of COMMANDS) {
      expect(COMMAND_BY_ID[command.id]).toBe(command)
    }
  })

  it('uses unique ids', () => {
    const ids = COMMANDS.map((command) => command.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('ships unique default hotkeys', () => {
    const hotkeys = COMMANDS.map((command) => command.defaultHotkey)
    expect(new Set(hotkeys).size).toBe(hotkeys.length)
  })
})
