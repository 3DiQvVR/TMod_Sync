import { useEffect, useRef, useState, type ReactNode } from 'react'
import { ServerPanel } from './ServerPanel'
import {
  Activity,
  ArrowDownToLine,
  ArrowRight,
  Box,
  Boxes,
  Check,
  CheckCheck,
  ChevronRight,
  Circle,
  CircleAlert,
  Clipboard,
  Copy,
  Crown,
  Download,
  ExternalLink,
  FileJson,
  FolderOpen,
  Gamepad2,
  Globe2,
  HardDrive,
  LayoutDashboard,
  Layers3,
  ListChecks,
  LoaderCircle,
  Monitor,
  Network,
  Play,
  PlugZap,
  Radio,
  RefreshCw,
  Save,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Upload,
  Users,
  X,
  type LucideIcon,
} from 'lucide-react'
import {
  initial,
  issueCount,
  native,
  netName,
  useWorkspace,
  type Diff,
  type Settings,
  type Provider,
  type View,
  type Workspace,
} from './workspace'

const navigation: { id: View; name: string; icon: LucideIcon }[] = [
  { id: 'overview', name: '联机工作台', icon: LayoutDashboard },
  { id: 'profile', name: '房间清单', icon: Layers3 },
  { id: 'network', name: '虚拟局域网', icon: Network },
  { id: 'server', name: '独立服务端', icon: Server },
  { id: 'paths', name: '路径设置', icon: FolderOpen },
]
function Pill({
  children,
  green = false,
  amber = false,
}: {
  children: ReactNode
  green?: boolean
  amber?: boolean
}) {
  return (
    <span className={'pill ' + (green ? 'green' : amber ? 'amber' : '')}>
      <i />
      {children}
    </span>
  )
}
function CardTop({
  icon: Icon,
  title,
  children,
}: {
  icon: LucideIcon
  title: string
  children?: ReactNode
}) {
  return (
    <div className="card-top">
      <div className="title-pair">
        <span className="icon-box">
          <Icon size={19} />
        </span>
        <h2>{title}</h2>
      </div>
      {children}
    </div>
  )
}
function OpenView({ w, to, label }: { w: Workspace; to: View; label: string }) {
  return (
    <button
      className="icon-btn arrow"
      aria-label={label}
      title={label}
      onClick={() => w.setView(to)}
    >
      <ArrowRight size={17} />
    </button>
  )
}
function RoleTabs({ w }: { w: Workspace }) {
  return (
    <div className="role-tabs" aria-label="房间角色">
      <button
        aria-pressed={w.role === 'host'}
        disabled={!!w.busy}
        onClick={() => w.chooseRole('host')}
      >
        <Crown size={14} />
        房主模式
      </button>
      <button
        aria-pressed={w.role === 'guest'}
        disabled={!!w.busy}
        onClick={() => w.chooseRole('guest')}
      >
        <Users size={14} />
        队友模式
      </button>
    </div>
  )
}
function ProfileCard({ w }: { w: Workspace }) {
  const room = w.session?.room
  return (
    <section className="card summary-card">
      <CardTop icon={Layers3} title="房间清单">
        <OpenView w={w} to="profile" label="查看房间清单" />
      </CardTop>
      <div className="file-preview">
        <div className="file-icon">
          <FileJson size={22} />
        </div>
        <div className="truncate">
          <b>
            {room?.roomName ??
              (w.role === 'host' ? '本机模组清单' : '房主模组清单')}
          </b>
          <small className="mono">room-profile.json</small>
        </div>
        <Pill green={!!room}>{room ? '已载入' : '待载入'}</Pill>
      </div>
      <div className="metrics">
        {[
          [room?.mods.length, '启用模组'],
          [room && Object.keys(room.configs).length, '配置文件'],
          [w.diff && issueCount(w.diff), '待处理差异'],
        ].map(([v, k]) => (
          <div key={String(k)}>
            <strong>{v ?? '—'}</strong>
            <span>{k}</span>
          </div>
        ))}
      </div>
      <div className="card-footer">
        <button
          disabled={!!w.busy}
          onClick={w.loadProfile}
          className="btn secondary"
        >
          {w.role === 'host' ? <RefreshCw /> : <Upload />}
          {w.role === 'host' ? '读取本机清单' : '导入房间清单'}
        </button>
        {w.role === 'host' ? (
          <button
            className="text-btn"
            disabled={!room || !!w.busy}
            onClick={w.exportRoom}
          >
            <Download />
            导出
          </button>
        ) : (
          <button
            className="text-btn"
            disabled={!room}
            onClick={() => w.setView('profile')}
          >
            查看差异
            <ChevronRight />
          </button>
        )}
      </div>
    </section>
  )
}
function NetworkGuide({ w }: { w: Workspace }) {
  const selected = w.settings.provider
  const other: Provider = selected === 'tailscale' ? 'zerotier' : 'tailscale'
  const [joinId, setJoinId] = useState(w.settings.networkId)
  useEffect(() => setJoinId(w.settings.networkId), [w.settings.networkId])
  const discovery = w.discovery
  if (!discovery || w.network?.online) return null
  const installed = discovery.installed[selected]
  const download = (provider: Provider) => (
    <a
      className="btn secondary"
      href={
        provider === 'tailscale'
          ? 'https://tailscale.com/download'
          : 'https://www.zerotier.com/download/'
      }
      onClick={(e) => {
        e.preventDefault()
        void w.downloadNetwork(provider)
      }}
    >
      {netName(provider)} 官网下载
      <ExternalLink size={14} />
    </a>
  )
  return (
    <section className="net-guide" aria-label="组网引导">
      <span className="icon-box">
        <Network size={19} />
      </span>
      <div className="guide-body">
        <h2>
          {installed
            ? netName(selected) + ' 尚未连接'
            : discovery.installed[other]
              ? '检测到 ' + netName(other)
              : '先安装一个组网客户端'}
        </h2>
        <p>
          {installed
            ? w.network?.detail
            : discovery.installed[other]
              ? '当前选择的 ' +
                netName(selected) +
                ' 尚未检测到。可直接使用本机已有的客户端。'
              : '房主和队友选择同一种工具。安装并连接后，这里会自动显示直连地址。'}
        </p>
        {discovery.adapterIp && (
          <p>
            虚拟网卡地址 <code>{discovery.adapterIp}</code> · 服务连接待确认
          </p>
        )}
        <div className="guide-actions">
          {installed ? (
            selected === 'zerotier' && !w.settings.networkId ? (
              <form
                onSubmit={(e) => {
                  e.preventDefault()
                  void w.connect(joinId)
                }}
              >
                <input
                  aria-label="加入的 ZeroTier 网络 ID"
                  placeholder="16 位 ZeroTier 网络 ID"
                  value={joinId}
                  pattern="[a-fA-F0-9]{16}"
                  required
                  maxLength={16}
                  onChange={(e) => setJoinId(e.target.value)}
                />
                <button className="btn primary" disabled={!!w.busy}>
                  <PlugZap />
                  加入网络
                </button>
              </form>
            ) : (
              <button
                className="btn primary"
                disabled={!!w.busy}
                onClick={() => w.connect()}
              >
                <PlugZap />
                连接 {netName(selected)}
              </button>
            )
          ) : (
            <>
              {discovery.installed[other] && (
                <button
                  className="btn primary"
                  disabled={!!w.busy}
                  onClick={() => w.save({ ...w.settings, provider: other })}
                >
                  使用 {netName(other)}
                </button>
              )}
              {download(selected)}
              {!discovery.installed[other] && download(other)}
            </>
          )}
          <button
            className="text-btn"
            disabled={!!w.busy}
            onClick={w.refreshNetwork}
          >
            <RefreshCw />
            重新检测
          </button>
        </div>
      </div>
    </section>
  )
}
function NetworkCard({ w }: { w: Workspace }) {
  const label = netName(w.settings.provider),
    online = !!w.network?.online
  return (
    <section className="card summary-card">
      <CardTop icon={Network} title="虚拟局域网">
        <OpenView w={w} to="network" label="配置虚拟局域网" />
      </CardTop>
      <div className="net-route">
        <div className="route-end">
          <Monitor />
          <span>本机</span>
        </div>
        <i className={online ? 'connected' : ''} />
        <div className={'route-middle ' + (online ? 'active' : '')}>
          <Radio />
        </div>
        <i className={online ? 'connected' : ''} />
        <div className="route-end">
          <Globe2 />
          <span>{label}</span>
        </div>
      </div>
      <div className="addr-row">
        <span>虚拟 IP</span>
        <code>{w.network?.ip ?? '—'}</code>
        <Pill green={online}>
          {online
            ? '在线'
            : w.discovery && !w.discovery.installed[w.settings.provider]
              ? '未安装'
              : w.network
                ? '离线'
                : '待检测'}
        </Pill>
      </div>
      <div className="card-footer">
        <button
          disabled={!!w.busy}
          className="btn secondary"
          onClick={() => w.setView('network')}
        >
          <SlidersHorizontal />
          网络设置
        </button>
        {online && w.network?.ip ? (
          <button
            className="text-btn"
            disabled={!!w.busy}
            onClick={() => w.copy(w.network!.ip!)}
          >
            <Copy />
            复制 IP
          </button>
        ) : (
          <button
            className="text-btn"
            disabled={!!w.busy || !w.discovery?.installed[w.settings.provider]}
            onClick={() => w.connect()}
          >
            <PlugZap />
            接入网络
          </button>
        )}
      </div>
    </section>
  )
}
function CheckLine({
  good,
  title,
  detail,
  go,
}: {
  good: boolean
  title: string
  detail: string
  go: () => void
}) {
  return (
    <button className="check-line" onClick={go}>
      <span className={'check-icon ' + (good ? 'good' : '')}>
        {good ? <Check size={15} /> : <Circle size={12} />}
      </span>
      <b>{title}</b>
      <span>{detail}</span>
      <ChevronRight size={14} />
    </button>
  )
}
function Address({ w }: { w: Workspace }) {
  return (
    <div className="address-field">
      <label htmlFor="peer-ip">
        {w.role === 'host' ? '房间直连地址' : '房主直连地址'}
      </label>
      <div className="input-action">
        <input
          id="peer-ip"
          className="mono"
          readOnly={w.role === 'host'}
          value={w.role === 'host' ? (w.endpoint?.address ?? '') : w.peer}
          onChange={(e) => w.setPeer(e.target.value)}
          placeholder={
            w.role === 'host' ? '组网在线后生成地址' : '输入 IP 或 IP:端口'
          }
          spellCheck={false}
        />
        <button
          className="icon-btn"
          disabled={!w.endpoint || !!w.busy}
          aria-label="一键复制直连地址"
          title="一键复制直连地址"
          onClick={() => w.copy(w.endpoint!.address)}
        >
          <Copy size={16} />
        </button>
      </div>
      {w.role === 'guest' && w.peer.trim() && !w.endpoint && (
        <small className="warn">
          请输入有效 IP，IPv6 携带端口时使用方括号。
        </small>
      )}
    </div>
  )
}
function Preflight({ w }: { w: Workspace }) {
  const files = !!w.paths?.data && !!w.paths?.install
  const checks = Number(files) + Number(w.aligned) + Number(!!w.network?.online)
  return (
    <section className="card preflight">
      <CardTop icon={ShieldCheck} title="启动预检">
        <span className="counter">
          <b>{checks}</b> / 3 项就绪
        </span>
      </CardTop>
      <div className="pre-grid">
        <div className="pre-checks">
          <CheckLine
            title="游戏路径"
            good={files}
            detail={files ? '已定位' : w.paths ? '待配置' : '待检测'}
            go={() => w.setView('paths')}
          />
          <CheckLine
            title="模组与配置"
            good={w.aligned}
            detail={w.aligned ? '已对齐' : w.diff ? '存在差异' : '待比对'}
            go={() => w.setView('profile')}
          />
          <CheckLine
            title="虚拟局域网"
            good={!!w.network?.online}
            detail={
              w.network?.online ? '已连接' : w.network ? '离线' : '待检测'
            }
            go={() => w.setView('network')}
          />
        </div>
        <div className="launch-pane">
          <Address w={w} />
          <div className="launch-buttons">
            <button
              className="btn primary"
              onClick={() => w.launch('client')}
              disabled={
                !!w.busy || !w.ready || (w.role === 'guest' && !w.endpoint)
              }
            >
              <Play />
              启动客户端
            </button>
            <button
              className="btn secondary"
              disabled={!!w.busy || w.role !== 'host'}
              onClick={() => w.setView('server')}
            >
              <Server />
              独立服务端
            </button>
          </div>
        </div>
      </div>
    </section>
  )
}
function Overview({ w }: { w: Workspace }) {
  return (
    <>
      <section className="hero">
        <div className="landscape" />
        <div className="hero-content">
          <h1>联机工作台</h1>
          <p>同步模组与房间配置，使用虚拟局域网直连。</p>
          <RoleTabs w={w} />
        </div>
      </section>
      <div className="overview-grid">
        <ProfileCard w={w} />
        <NetworkCard w={w} />
      </div>
      <Preflight w={w} />
    </>
  )
}
const categories: {
  key: keyof Pick<
    Diff,
    'missing' | 'mismatched' | 'extra' | 'disabled' | 'configs'
  >
  name: string
}[] = [
  { key: 'missing', name: '缺失模组' },
  { key: 'mismatched', name: '版本或校验值不一致' },
  { key: 'extra', name: '多余已启用模组' },
  { key: 'disabled', name: '已安装但未启用' },
  { key: 'configs', name: '配置差异' },
]
function Profile({ w }: { w: Workspace }) {
  const room = w.session?.room
  const [confirm, setConfirm] = useState(false)
  const [search, setSearch] = useState('')
  useEffect(() => {
    setConfirm(false)
    setSearch('')
  }, [w.session])
  return (
    <>
      <div className="page-title">
        <div>
          <h1>房间清单</h1>
          <p>读取启用模组、版本与 SHA-256，核对房间配置。</p>
        </div>
        <RoleTabs w={w} />
      </div>
      <section className="card panel">
        <CardTop icon={FileJson} title={room?.roomName ?? '房间配置文件'}>
          <Pill green={w.aligned}>
            {w.aligned ? '已对齐' : room ? '已载入' : '待载入'}
          </Pill>
        </CardTop>
        <div className="room-input">
          {w.role === 'host' && (
            <label>
              房间名称
              <input
                maxLength={100}
                value={w.roomName}
                onChange={(e) => w.setRoomName(e.target.value)}
                placeholder="输入房间名称"
              />
            </label>
          )}
          <button
            className="btn primary"
            disabled={!!w.busy}
            onClick={w.loadProfile}
          >
            {w.role === 'host' ? <Layers3 /> : <Upload />}
            {w.role === 'host' ? '生成房间清单' : '导入 room-profile.json'}
          </button>
          {room && (
            <button
              className="btn secondary"
              disabled={!!w.busy}
              onClick={w.refresh}
            >
              <RefreshCw />
              重新比对
            </button>
          )}
        </div>
        <div className="inline-actions">
          {w.role === 'host' && (
            <button
              className="text-btn"
              disabled={!room || !!w.busy}
              onClick={w.exportRoom}
            >
              <ArrowDownToLine />
              导出 JSON
            </button>
          )}
          <button
            className="text-btn"
            disabled={!room || !!w.busy}
            onClick={() => w.copy(w.session!.raw)}
          >
            <Clipboard />
            复制清单 JSON
          </button>
        </div>
      </section>
      <section className="card panel">
        <CardTop icon={ListChecks} title="本机差异">
          <span className="counter">
            {w.diff ? issueCount(w.diff) + ' 项差异' : '待比对'}
          </span>
        </CardTop>
        {!w.diff ? (
          <p className="empty-state">
            载入房间清单后，在这里查看模组与配置的差异。
          </p>
        ) : (
          <>
            {categories.map((c) => (
              <details
                className="diff-group"
                key={c.key}
                open={w.diff![c.key].length > 0}
              >
                <summary>
                  <ChevronRight size={14} />
                  {c.name}
                  <b className={w.diff![c.key].length ? 'warn' : ''}>
                    {w.diff![c.key].length}
                  </b>
                </summary>
                {w.diff![c.key].length === 0 ? (
                  <small className="diff-row">本项已对齐</small>
                ) : (
                  w.diff![c.key].map((key) => {
                    const id = room?.mods.find(
                      (m) => m.name === key,
                    )?.workshopId
                    return (
                      <div className="diff-row" key={key}>
                        <div>
                          <strong>{key}</strong>
                          {w.diff!.details[key] && (
                            <small>{w.diff!.details[key]}</small>
                          )}
                        </div>
                        {(c.key === 'missing' || c.key === 'mismatched') && (
                          <button
                            className="text-btn"
                            disabled={!!w.busy}
                            onClick={() => w.workshop(id, key)}
                          >
                            {id ? '打开订阅页' : '搜索创意工坊'}
                            <ExternalLink />
                          </button>
                        )}
                      </div>
                    )
                  })
                )}
              </details>
            ))}
          </>
        )}
        {w.role === 'guest' && room && (
          <div className="sync-row">
            {confirm ? (
              <>
                <p>
                  从工坊复制校验一致的模组，备份待替换文件，再按房间清单写入配置与启用列表。
                </p>
                <button
                  className="btn primary"
                  disabled={!!w.busy}
                  onClick={() => {
                    setConfirm(false)
                    void w.apply()
                  }}
                >
                  <Check />
                  确认同步
                </button>
                <button
                  className="btn secondary"
                  onClick={() => setConfirm(false)}
                >
                  取消
                </button>
              </>
            ) : (
              <button
                className="btn primary"
                disabled={!!w.busy}
                onClick={() => setConfirm(true)}
              >
                <CheckCheck />
                同步模组与配置
              </button>
            )}
          </div>
        )}
      </section>
      {room && (
        <section className="card panel">
          <CardTop icon={Boxes} title="清单中的模组">
            <span className="counter">{room.mods.length} 个模组</span>
          </CardTop>
          {!!room.mods.length && (
            <input
              className="filter"
              aria-label="筛选模组"
              value={search}
              placeholder="按内部名称筛选"
              onChange={(e) => setSearch(e.target.value)}
            />
          )}
          {!room.mods.length && (
            <p className="empty-state">此房间未启用模组。</p>
          )}
          {room.mods
            .filter((m) => m.name.toLowerCase().includes(search.toLowerCase()))
            .map((m) => (
              <div className="mod-row" key={m.name}>
                <Box size={16} />
                <div>
                  <b>{m.name}</b>
                  <code title={m.sha256}>{m.sha256}</code>
                </div>
                <span className="version mono">v{m.version}</span>
              </div>
            ))}
        </section>
      )}
    </>
  )
}
function NetworkView({ w }: { w: Workspace }) {
  const [provider, setProvider] = useState(w.settings.provider),
    [id, setId] = useState(w.settings.networkId),
    [port, setPort] = useState(String(w.settings.port))
  useEffect(() => {
    setProvider(w.settings.provider)
    setId(w.settings.networkId)
    setPort(String(w.settings.port))
  }, [w.settings])
  const dirty =
    provider !== w.settings.provider ||
    id !== w.settings.networkId ||
    +port !== w.settings.port
  return (
    <>
      <div className="page-title">
        <div>
          <h1>虚拟局域网</h1>
          <p>选择组网服务，获取本机 IP 与房间直连地址。</p>
        </div>
      </div>
      <section className="card panel">
        <CardTop icon={Radio} title="组网服务">
          <Pill green={!!w.network?.online}>
            {netName(w.settings.provider)} ·{' '}
            {w.network?.online ? '在线' : w.network ? '离线' : '待检测'}
          </Pill>
        </CardTop>
        <div className="provider-list">
          {(['tailscale', 'zerotier'] as const).map((p) => (
            <button
              key={p}
              aria-pressed={p === provider}
              onClick={() => setProvider(p)}
            >
              <Network size={17} />
              {netName(p)}
              <span>{p === provider ? <Check size={15} /> : null}</span>
            </button>
          ))}
        </div>
        <form
          className="settings-form"
          onSubmit={(e) => {
            e.preventDefault()
            void w.save({
              ...w.settings,
              provider,
              networkId: id.trim(),
              port: +port,
            })
          }}
        >
          {provider === 'zerotier' && (
            <label>
              ZeroTier 网络 ID
              <input
                value={id}
                onChange={(e) => setId(e.target.value)}
                placeholder="16 位十六进制网络 ID"
                maxLength={16}
                pattern="([0-9a-fA-F]{16})?"
                spellCheck={false}
              />
            </label>
          )}
          <label>
            游戏端口
            <input
              type="number"
              required
              min="1"
              max="65535"
              value={port}
              onChange={(e) => setPort(e.target.value)}
            />
          </label>
          <div className="inline-actions">
            <button
              className="btn secondary"
              type="submit"
              disabled={!dirty || !!w.busy}
            >
              <Save />
              保存设置
            </button>
            <button
              type="button"
              className="btn primary"
              disabled={dirty || !!w.busy || !w.discovery?.installed[provider]}
              onClick={() => w.connect()}
            >
              <PlugZap />
              接入 {netName(provider)}
            </button>
          </div>
        </form>
      </section>
      <section className="card panel">
        <CardTop icon={Activity} title="连接状态">
          <button className="text-btn" disabled={!!w.busy} onClick={w.refresh}>
            <RefreshCw />
            刷新
          </button>
        </CardTop>
        <p className="network-detail">
          {w.network?.detail ||
            (native
              ? '正在检测本机组网服务。'
              : '桌面客户端会在此显示本机组网状态。')}
        </p>
        <div className="detail-grid">
          <div>
            <small>本机虚拟 IP</small>
            <code>{w.network?.ip || '—'}</code>
          </div>
          <div>
            <small>最近检测</small>
            <code>{w.checked || '—'}</code>
          </div>
        </div>
        <Address w={w} />
      </section>
    </>
  )
}
const pathFields = [
  {
    key: 'dataDir',
    status: 'data',
    name: '游戏数据目录',
    hint: 'tModLoader 存档根目录，包含 Mods 与 ModConfigs。',
  },
  {
    key: 'installDir',
    status: 'install',
    name: '游戏安装目录',
    hint: '包含客户端与独立服务端启动脚本的 tModLoader 文件夹。',
  },
  {
    key: 'workshopDir',
    status: 'workshop',
    name: 'Steam 创意工坊目录',
    hint: 'steamapps/workshop/content/1281930，可在仅使用本地模组时留空。',
  },
] as const
function Paths({ w }: { w: Workspace }) {
  const [draft, setDraft] = useState<Settings>(w.settings)
  useEffect(() => setDraft(w.settings), [w.settings])
  const edit = (key: keyof typeof initial, value: string) =>
    setDraft((d) => ({ ...d, [key]: value }))
  return (
    <>
      <div className="page-title">
        <div>
          <h1>路径设置</h1>
          <p>定位游戏、存档与创意工坊文件。</p>
        </div>
        <button
          className="btn secondary"
          disabled={!!w.busy}
          onClick={() =>
            w.detect((d) =>
              setDraft((s) => ({
                ...s,
                dataDir: d.dataDir || s.dataDir,
                installDir: d.installDir || s.installDir,
                workshopDir: d.workshopDir || s.workshopDir,
              })),
            )
          }
        >
          <Search />
          自动探测
        </button>
      </div>
      <section className="card panel">
        <CardTop icon={HardDrive} title="本地目录" />
        <form
          className="path-form"
          onSubmit={(e) => {
            e.preventDefault()
            void w.save(draft)
          }}
        >
          {pathFields.map((f) => (
            <div key={f.key} className="path-field">
              <div className="label-row">
                <label htmlFor={f.key}>{f.name}</label>
                <Pill
                  green={
                    w.paths?.[f.status] && draft[f.key] === w.settings[f.key]
                  }
                >
                  {draft[f.key] !== w.settings[f.key]
                    ? '待保存'
                    : w.paths?.[f.status]
                      ? '已定位'
                      : '待配置'}
                </Pill>
              </div>
              <div className="input-action">
                <input
                  id={f.key}
                  value={draft[f.key]}
                  spellCheck={false}
                  placeholder="输入绝对路径"
                  onChange={(e) => edit(f.key, e.target.value)}
                />
                <button
                  type="button"
                  className="icon-btn"
                  disabled={!!w.busy}
                  onClick={() => w.browse((path) => edit(f.key, path))}
                  aria-label={'选择' + f.name}
                >
                  <FolderOpen size={17} />
                </button>
              </div>
              <small>{f.hint}</small>
            </div>
          ))}
          <label>
            tModLoader 分支月份
            <input
              placeholder="自动识别，可指定 YYYY.MM"
              value={draft.branch}
              onChange={(e) => edit('branch', e.target.value)}
              pattern="(20[0-9]{2}\.(0?[1-9]|1[0-2]))?"
            />
          </label>
          <div className="inline-actions">
            <button type="submit" className="btn primary" disabled={!!w.busy}>
              <Save />
              保存路径
            </button>
          </div>
        </form>
      </section>
    </>
  )
}
export default function App() {
  const w = useWorkspace(),
    scroll = useRef<HTMLElement>(null)
  useEffect(() => {
    scroll.current?.scrollTo({ top: 0 })
  }, [w.view])
  const item = navigation.find((x) => x.id === w.view)!
  const go = (v: View) => w.setView(v)
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <img src="/mark.png" alt="" />
          <span>
            tMod<span>Sync</span>
          </span>
        </div>
        <div className="nav-caption">工作空间</div>
        <nav>
          {navigation
            .filter(
              (n) =>
                n.id !== 'paths' && (n.id !== 'server' || w.role === 'host'),
            )
            .map((n) => (
              <button
                key={n.id}
                aria-current={w.view === n.id ? 'page' : undefined}
                onClick={() => go(n.id)}
              >
                <n.icon />
                {n.name}
              </button>
            ))}
        </nav>
        <div className="nav-caption second-caption">设置</div>
        <nav>
          <button
            aria-current={w.view === 'paths' ? 'page' : undefined}
            onClick={() => go('paths')}
          >
            <Settings2 />
            路径设置
          </button>
        </nav>
        <div className="sidebar-foot">
          <Gamepad2 size={17} />
          <div>
            <b>tModLoader</b>
            <small className="mono">APP 1281930</small>
          </div>
        </div>
      </aside>
      <main ref={scroll}>
        <header>
          <div className="breadcrumb">
            <LayoutDashboard size={15} />
            <span>工作空间</span>
            <ChevronRight size={13} />
            <b>{item.name}</b>
          </div>
          <div className="header-actions">
            <span className="desktop-status">
              <i />
              {native ? '桌面客户端' : '浏览器预览'}
            </span>
            <div className="header-separator" />
            <button
              className="text-btn"
              disabled={!!w.busy}
              onClick={w.refresh}
            >
              <RefreshCw
                size={14}
                className={w.busy === '刷新状态' ? 'spinning' : ''}
              />
              刷新状态
            </button>
          </div>
        </header>
        <div className="body-wrap">
          <NetworkGuide w={w} />
          {w.view === 'overview' ? (
            <Overview w={w} />
          ) : w.view === 'profile' ? (
            <Profile w={w} />
          ) : w.view === 'network' ? (
            <NetworkView w={w} />
          ) : w.view === 'server' ? (
            <ServerPanel w={w} />
          ) : (
            <Paths w={w} />
          )}
          <div className="workspace-foot">
            <span>
              <i />{' '}
              {w.busy || (native ? '本地工作空间' : '桌面功能需在客户端中运行')}
            </span>
            <span className="mono">tModSync v0.1.0</span>
          </div>
        </div>
      </main>
      {w.busy && (
        <div className="busy-toast" role="status">
          <LoaderCircle className="spinning" size={16} />
          {w.busy}中…
        </div>
      )}
      {w.notice && (
        <div
          className={'notice ' + (w.notice.error ? 'error' : '')}
          role={w.notice.error ? 'alert' : 'status'}
        >
          {w.notice.error ? <CircleAlert size={18} /> : <Check size={18} />}
          <span>{w.notice.text}</span>
          <button aria-label="关闭提示" onClick={() => w.setNotice(null)}>
            <X size={15} />
          </button>
        </div>
      )}
    </div>
  )
}
