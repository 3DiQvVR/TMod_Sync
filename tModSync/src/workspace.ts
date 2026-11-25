import { useEffect, useRef, useState } from 'react'
import { invoke, isTauri } from '@tauri-apps/api/core'

export type Role = 'host' | 'guest'
export type View = 'overview' | 'profile' | 'network' | 'paths' | 'server'
export type ServerStatus = {
  running: boolean
  stopping: boolean
  world: string | null
  port: number | null
  exitCode: number | null
  lines: string[]
}
export type Provider = 'tailscale' | 'zerotier'
export type Settings = {
  dataDir: string
  installDir: string
  workshopDir: string
  branch: string
  provider: Provider
  networkId: string
  port: number
}
export type Room = {
  schemaVersion: 1
  roomName: string
  mods: { name: string; version: string; sha256: string; workshopId?: string }[]
  configs: Record<string, unknown>
}
export type Diff = {
  missing: string[]
  mismatched: string[]
  extra: string[]
  disabled: string[]
  configs: string[]
  matched: number
  details: Record<string, string>
}
type PathStatus = { data: boolean; install: boolean; workshop: boolean }
type Network = { online: boolean; ip: string | null; detail: string }
type NetworkOverview = {
  installed: Record<Provider, boolean>
  status: Network
  adapterIp: string | null
}
export const initial: Settings = {
  dataDir: '',
  installDir: '',
  workshopDir: '',
  branch: '',
  provider: 'tailscale',
  networkId: '',
  port: 7777,
}
export const native = isTauri()
export const netName = (provider: Provider) =>
  provider === 'tailscale' ? 'Tailscale' : 'ZeroTier'
export const issueCount = (d: Diff) =>
  d.missing.length +
  d.mismatched.length +
  d.extra.length +
  d.disabled.length +
  d.configs.length
export function parseEndpoint(text: string, defaultPort: number) {
  const input = text.trim()
  if (!input) return null
  let host = input,
    port = defaultPort
  const joined =
    input.match(/^\[([^\]]+)\](?::(\d+))?$/) ??
    input.match(/^(\d[\d.]*):(\d+)$/)
  if (joined) {
    host = joined[1]
    if (joined[2]) port = Number(joined[2])
  }
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null
  if (host.includes(':')) {
    if (!/^[0-9a-f:.]+$/i.test(host)) return null
    try {
      new URL('http://[' + host + ']')
    } catch {
      return null
    }
  } else {
    const octets = host.split('.')
    if (
      octets.length !== 4 ||
      !octets.every((x) => /^(0|[1-9]\d{0,2})$/.test(x) && +x <= 255)
    )
      return null
  }
  return {
    host,
    port,
    address: (host.includes(':') ? '[' + host + ']' : host) + ':' + port,
  }
}
export function useWorkspace() {
  const [settings, setSettings] = useState(initial)
  const [paths, setPaths] = useState<PathStatus | null>(null)
  const [network, setNetwork] = useState<Network | null>(null)
  const [discovery, setDiscovery] = useState<NetworkOverview | null>(null)
  const [role, setRole] = useState<Role>('host')
  const [view, setView] = useState<View>('overview')
  const [roomName, setRoomName] = useState('我的联机房间')
  const [session, setSession] = useState<{ raw: string; room: Room } | null>(
    null,
  )
  const [diff, setDiff] = useState<Diff | null>(null)
  const [peer, setPeer] = useState('')
  const [checked, setChecked] = useState('')
  const [netTick, setNetTick] = useState(0)
  const [loaded, setLoaded] = useState(false)
  const [server, setServer] = useState<ServerStatus | null>(null)
  const [serverError, setServerError] = useState('')
  useEffect(() => {
    if (!native || !loaded) return
    let active = true,
      timer: ReturnType<typeof setTimeout>
    async function poll() {
      try {
        const value = await invoke<ServerStatus>('server_status')
        if (active) {
          setServer(value)
          setServerError('')
        }
      } catch (e) {
        if (active) setServerError(String(e))
      } finally {
        if (active) timer = setTimeout(poll, view === 'server' ? 750 : 3000)
      }
    }
    void poll()
    return () => {
      active = false
      clearTimeout(timer)
    }
  }, [loaded, view])
  const [busy, setBusy] = useState('')
  const locked = useRef(false)
  const [notice, setNotice] = useState<{
    text: string
    error?: boolean
  } | null>(null)
  const msg = (text: string) => setNotice({ text })
  const refreshNetwork = () => setNetTick((t) => t + 1)

  useEffect(() => {
    if (!native) return
    let alive = true
    async function init() {
      try {
        const [s, p] = await Promise.all([
          invoke<Settings>('settings'),
          invoke<PathStatus>('path_status'),
        ])
        if (alive) {
          setSettings(s)
          setPaths(p)
        }
      } catch (e) {
        if (alive) setNotice({ text: String(e), error: true })
      } finally {
        if (alive) setLoaded(true)
      }
    }
    void init()
    return () => {
      alive = false
    }
  }, [])
  useEffect(() => {
    if (!loaded) return
    let active = true,
      timer: ReturnType<typeof setTimeout>
    setNetwork(null)
    setDiscovery(null)
    async function poll() {
      try {
        const snapshot = await invoke<NetworkOverview>('network_overview', {
          provider: settings.provider,
          networkId: settings.networkId,
        })
        if (active) {
          setDiscovery(snapshot)
          setNetwork(snapshot.status)
        }
      } catch (e) {
        if (active) {
          setDiscovery(null)
          setNetwork({ online: false, ip: null, detail: String(e) })
        }
      } finally {
        if (active) {
          setChecked(
            new Date().toLocaleTimeString('zh-CN', {
              hour: '2-digit',
              minute: '2-digit',
              second: '2-digit',
              hour12: false,
            }),
          )
          timer = setTimeout(poll, 6000)
        }
      }
    }
    void poll()
    return () => {
      active = false
      clearTimeout(timer)
    }
  }, [loaded, settings.provider, settings.networkId, netTick])

  async function action(label: string, fn: () => Promise<void>) {
    if (locked.current) return
    if (!native) {
      setNotice({ text: '请在 tModSync 桌面客户端中执行本机操作。' })
      return
    }
    locked.current = true
    setBusy(label)
    setNotice(null)
    try {
      await fn()
    } catch (e) {
      setNotice({ text: String(e), error: true })
    } finally {
      locked.current = false
      setBusy('')
    }
  }
  function chooseRole(next: Role) {
    if (next === role || locked.current) return
    setRole(next)
    setSession(null)
    setDiff(null)
    setPeer('')
    setNotice(null)
  }
  async function compare(raw: string) {
    setDiff(null)
    const result = await invoke<Diff>('compare', {
      room: raw,
      requireLocal: role === 'guest',
    })
    setDiff(result)
    return result
  }
  async function select(raw: string) {
    setSession({ room: JSON.parse(raw) as Room, raw })
    await compare(raw)
  }
  const loadProfile = () =>
    action(role === 'host' ? '读取本机清单' : '导入房间清单', async () => {
      if (role === 'host') {
        setSession(null)
        setDiff(null)
        await select(
          await invoke<string>('capture', {
            roomName: roomName.trim() || '我的联机房间',
          }),
        )
        msg('已从本机生成房间清单。')
      } else {
        const imported = await invoke<string | null>('import_room')
        if (imported) {
          await select(imported)
          msg('已导入房间清单并比对本机环境。')
        }
      }
    })
  const refresh = () =>
    action('刷新状态', async () => {
      refreshNetwork()
      setDiff(null)
      setPaths(await invoke<PathStatus>('path_status'))
      if (session) await compare(session.raw)
      msg('已刷新本机状态。')
    })
  async function persist(value: Settings) {
    await invoke('save_settings', { value })
    const changed =
      value.dataDir !== settings.dataDir ||
      value.installDir !== settings.installDir ||
      value.workshopDir !== settings.workshopDir ||
      value.branch !== settings.branch
    if (changed) {
      setDiff(null)
      if (role === 'host') setSession(null)
      setPaths(null)
    }
    setSettings(value)
    setPaths(await invoke<PathStatus>('path_status'))
    if (changed && session && role === 'guest') await compare(session.raw)
  }
  const save = (value: Settings) =>
    action('保存设置', async () => {
      await persist(value)
      msg('设置已保存到 config.json。')
    })
  async function detect(onDetected: (s: Settings) => void) {
    await action('探测路径', async () => {
      onDetected(await invoke<Settings>('detect_paths'))
      msg('探测完成，请核对路径后保存。')
    })
  }
  const browse = (onPicked: (path: string) => void) =>
    action('选择目录', async () => {
      const path = await invoke<string | null>('pick_folder')
      if (path) onPicked(path)
    })
  const copy = (text: string) =>
    action('复制', async () => {
      await invoke('copy_text', { text })
      msg('已复制到剪贴板。')
    })
  const exportRoom = () =>
    action('导出清单', async () => {
      if (
        session &&
        (await invoke<boolean>('export_room', { room: session.raw }))
      )
        msg('已导出房间清单。')
    })
  const apply = () =>
    action('同步配置', async () => {
      if (!session) return
      setDiff(null)
      const { backup, diff: result } = await invoke<{
        backup: string | null
        diff: Diff
      }>('sync_mods', {
        room: session.raw,
      })
      setDiff(result)
      const pending = result.missing.length + result.mismatched.length
      msg(
        (pending
          ? '仍有 ' + pending + ' 个模组待处理，请查看差异列表。'
          : '模组与配置已同步。') + (backup ? '备份目录为 ' + backup : ''),
      )
    })
  const connect = (requestedId?: string) =>
    action('接入网络', async () => {
      const networkId = requestedId?.trim() ?? settings.networkId
      if (
        settings.provider === 'zerotier' &&
        !networkId &&
        view !== 'network'
      ) {
        setView('network')
        msg('请输入房主提供的网络 ID，或在本机客户端中选择网络。')
        return
      }
      if (networkId !== settings.networkId)
        await persist({ ...settings, networkId })
      try {
        msg(
          await invoke<string>('connect_network', {
            provider: settings.provider,
            networkId,
          }),
        )
      } finally {
        refreshNetwork()
      }
    })
  const workshop = (id: string | undefined, name: string) =>
    action('打开订阅页', async () => {
      await invoke('open_workshop', { id: id ?? null, name })
    })
  const endpoint = parseEndpoint(
    role === 'host' ? (network?.ip ?? '') : peer,
    role === 'host' && server?.running && server.port
      ? server.port
      : settings.port,
  )
  const aligned = !!diff && issueCount(diff) === 0
  const ready =
    aligned && !!network?.online && !!paths?.data && !!paths?.install
  const launch = (target: 'client' | 'server') =>
    action('启动预检', async () => {
      if (!session || !ready || (role === 'guest' && !endpoint)) return
      try {
        if (role === 'guest' && endpoint && endpoint.port !== settings.port)
          await persist({ ...settings, port: endpoint.port })
        await invoke('launch_game', {
          room: session.raw,
          target,
          host: role === 'guest' ? endpoint!.host : null,
        })
        msg(
          target === 'client'
            ? '客户端启动指令已发送。'
            : '独立服务端已在终端中启动。',
        )
      } catch (e) {
        setDiff(null)
        refreshNetwork()
        throw e
      }
    })
  return {
    discovery,
    refreshNetwork,
    downloadNetwork: (provider: Provider) =>
      action('打开官网下载页', async () => {
        await invoke('download_network', { provider })
      }),
    server,
    serverError,
    action,
    settings,
    paths,
    network,
    role,
    view,
    setView,
    roomName,
    setRoomName,
    session,
    diff,
    peer,
    setPeer,
    checked,
    loaded,
    busy,
    notice,
    setNotice,
    chooseRole,
    loadProfile,
    refresh,
    save,
    detect,
    browse,
    copy,
    exportRoom,
    apply,
    connect,
    workshop,
    endpoint,
    aligned,
    ready,
    launch,
  }
}
export type Workspace = ReturnType<typeof useWorkspace>
