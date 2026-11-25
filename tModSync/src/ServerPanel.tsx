import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Copy, Play, RefreshCw, Square, Terminal } from 'lucide-react'
import { native, type Workspace } from './workspace'

export function ServerPanel({ w }: { w: Workspace }) {
  const [worlds, setWorlds] = useState<string[]>([]),
    [world, setWorld] = useState('')
  const [players, setPlayers] = useState('8'),
    [port, setPort] = useState(String(w.settings.port)),
    [password, setPassword] = useState('')
  const [tick, refresh] = useState(0),
    [error, setError] = useState(''),
    [loading, setLoading] = useState(true)
  const status = w.server
  useEffect(() => {
    if (!native || !w.loaded) return
    let active = true
    setLoading(true)
    setError('')
    invoke<string[]>('list_worlds')
      .then((names) => {
        if (active) {
          setWorlds(names)
          setWorld((old) => (names.includes(old) ? old : (names[0] ?? '')))
        }
      })
      .catch((e) => {
        if (active) {
          setWorlds([])
          setWorld('')
          setError(String(e))
        }
      })
      .finally(() => {
        if (active) setLoading(false)
      })
    return () => {
      active = false
    }
  }, [w.settings.dataDir, w.loaded, tick])
  useEffect(() => {
    setPort(String(w.settings.port))
  }, [w.settings.port])
  const start = () =>
    w.action('开服预检', async () => {
      await invoke('start_server', {
        room: w.session?.raw ?? null,
        options: {
          world,
          maxPlayers: Number(players),
          password,
          port: Number(port),
          dataDir: w.settings.dataDir,
        },
      })
      setPassword('')
    })
  return (
    <>
      <div className="page-title">
        <div>
          <h1>独立服务端</h1>
          <p>选择本地世界，后台开服并查看控制台输出。</p>
        </div>
      </div>
      <section className="card panel">
        <div className="card-top">
          <h2>开服设置</h2>
          <button
            className="text-btn"
            disabled={loading || !!w.busy}
            onClick={() => refresh((t) => t + 1)}
          >
            <RefreshCw size={15} />
            刷新存档
          </button>
        </div>
        <form
          className="settings-form"
          onSubmit={(e) => {
            e.preventDefault()
            void start()
          }}
        >
          <fieldset
            className="server-fields"
            disabled={!!w.busy || status?.running}
          >
            <label>
              世界存档
              <select
                required
                value={world}
                onChange={(e) => setWorld(e.target.value)}
              >
                {!worlds.length && (
                  <option value="">
                    {loading ? '正在读取…' : '没有可选世界'}
                  </option>
                )}
                {worlds.map((name) => (
                  <option key={name}>{name}</option>
                ))}
              </select>
            </label>
            {error ? (
              <p role="alert">{error}</p>
            ) : (
              !loading &&
              !worlds.length && (
                <p>
                  当前数据目录的 Worlds 中没有 .wld
                  文件。请检查路径，或在游戏中创建世界后刷新。
                </p>
              )
            )}
            <div className="server-numbers">
              <label>
                最大人数
                <input
                  type="number"
                  required
                  min="1"
                  max="255"
                  value={players}
                  onChange={(e) => setPlayers(e.target.value)}
                />
              </label>
              <label>
                端口
                <input
                  type="number"
                  required
                  min="1"
                  max="65535"
                  value={port}
                  onChange={(e) => setPort(e.target.value)}
                />
              </label>
            </div>
            <label>
              房间密码
              <input
                type="password"
                value={password}
                autoComplete="new-password"
                placeholder="留空允许免密加入"
                onChange={(e) => setPassword(e.target.value)}
              />
            </label>
          </fieldset>
          <div className="inline-actions">
            <button
              className="btn primary"
              disabled={
                !native || !world || loading || !!w.busy || !!status?.running
              }
            >
              <Play />
              一键开服
            </button>
            {status?.running && (
              <button
                type="button"
                className="btn secondary"
                disabled={status.stopping || !!w.busy}
                onClick={() =>
                  w.action('保存并停服', () => invoke('stop_server'))
                }
              >
                <Square />
                保存并停服
              </button>
            )}
          </div>
        </form>
      </section>
      <section className="card panel">
        <div className="card-top">
          <div className="title-pair">
            <Terminal size={18} />
            <h2>服务端日志</h2>
          </div>
          <span className="counter">
            {w.serverError
              ? '读取失败'
              : status?.stopping
                ? '正在停服'
                : status?.running
                  ? '进程运行中'
                  : status?.world
                    ? '已退出 · ' + (status.exitCode ?? '未知')
                    : '未启动'}
          </span>
        </div>
        {status?.world && (
          <p className="network-detail">
            {status.world} · 端口 {status.port}
          </p>
        )}
        {w.serverError && <p role="alert">{w.serverError}</p>}
        <pre className="server-log" aria-label="最近 10 行服务端日志">
          {status?.lines.length ? status.lines.join('\n') : '等待服务端输出…'}
        </pre>
        <div className="card-footer">
          <small>最近 10 行 · 自动刷新</small>
          {status?.running && w.endpoint && (
            <button
              className="text-btn"
              onClick={() => w.copy(w.endpoint!.address)}
            >
              <Copy size={14} />
              {w.endpoint.address}
            </button>
          )}
        </div>
      </section>
    </>
  )
}
