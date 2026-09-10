import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Activity, CirclePause, CirclePlay, Network, RefreshCw, Route, Settings2, ShieldCheck, Trash2, Plus, CheckCircle2 } from 'lucide-react';

type TrafficMode = 'PROXY' | 'DIRECT' | 'MIXED' | 'LOCAL' | 'SYSTEM' | 'XRAY';
type RouteBucket = 'direct' | 'proxy' | 'block';
type ProxyMode = 'pac' | 'manual' | 'global' | 'tun';

type AppStatus = {
  v2rayu_running: boolean;
  xray_running: boolean;
  core_enabled: boolean;
  config_path: string;
  routing_db_path?: string | null;
  active_routing_uuid: string;
  proxy_mode: string;
};

type TrafficRow = {
  process: string;
  pid: number;
  mode: TrafficMode;
  proxy_connections: number;
  direct_connections: number;
  local_connections: number;
  down_bps: number;
  up_bps: number;
  direct_targets: string[];
};

type RoutingProfile = {
  uuid: string;
  name: string;
  remark: string;
  domain_strategy: string;
  domain_matcher: string;
  block: string[];
  proxy: string[];
  direct: string[];
  sort: number;
  active: boolean;
};

const formatRate = (value: number) => {
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  let n = value;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i += 1; }
  return `${n.toFixed(n >= 100 ? 0 : n >= 10 ? 1 : 2)} ${units[i]}`;
};

function RuleList({ title, bucket, values, onChange }: { title: string; bucket: RouteBucket; values: string[]; onChange: (bucket: RouteBucket, values: string[]) => void }) {
  const [draft, setDraft] = useState('');
  const add = () => {
    const value = draft.trim();
    if (!value || values.includes(value)) return;
    onChange(bucket, [...values, value]);
    setDraft('');
  };

  return <section className="rule-card">
    <div className="rule-card-head"><div><div className="eyebrow">{bucket.toUpperCase()}</div><h3>{title}</h3></div><span className="count-pill">{values.length}</span></div>
    <div className="rule-list">
      {values.map((value, index) => <div className="rule-row" key={`${value}-${index}`}><input className="rule-edit" value={value} onChange={(e) => onChange(bucket, values.map((v, i) => i === index ? e.target.value : v))}/><button className="icon-button danger" onClick={() => onChange(bucket, values.filter((_, i) => i !== index))}><Trash2 size={14}/></button></div>)}
      {!values.length && <div className="empty">No rules</div>}
    </div>
    <div className="rule-add"><input value={draft} onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && add()} placeholder="example.com, geosite:cn, 1.1.1.1 …"/><button className="icon-button" onClick={add}><Plus size={15}/></button></div>
  </section>;
}

export default function App() {
  const [tab, setTab] = useState<'connections' | 'routing' | 'control'>('connections');
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [traffic, setTraffic] = useState<TrafficRow[]>([]);
  const [profiles, setProfiles] = useState<RoutingProfile[]>([]);
  const [selectedUuid, setSelectedUuid] = useState('');
  const [draft, setDraft] = useState<RoutingProfile | null>(null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');

  const loadStatus = async () => {
    try { setStatus(await invoke<AppStatus>('get_status_async')); setError(''); }
    catch (e) { setError(String(e)); }
  };

  const loadProfiles = async () => {
    try {
      const rows = await invoke<RoutingProfile[]>('list_routing_profiles_async');
      setProfiles(rows);
      const active = rows.find((x) => x.active);
      const chosen = rows.find((x) => x.uuid === selectedUuid) ?? active ?? rows[0];
      if (chosen && !dirty) { setSelectedUuid(chosen.uuid); setDraft(structuredClone(chosen)); }
      setError('');
    } catch (e) { setError(String(e)); }
  };

  const loadTraffic = async () => {
    try { setTraffic(await invoke<TrafficRow[]>('get_traffic_snapshot_async')); }
    catch (e) { setError(String(e)); }
  };

  useEffect(() => {
    loadStatus(); loadProfiles(); loadTraffic();
    const trafficId = window.setInterval(loadTraffic, 1800);
    const statusId = window.setInterval(loadStatus, 5000);
    return () => { window.clearInterval(trafficId); window.clearInterval(statusId); };
  }, []);

  const totalDown = useMemo(() => traffic.reduce((s, x) => s + x.down_bps, 0), [traffic]);
  const totalUp = useMemo(() => traffic.reduce((s, x) => s + x.up_bps, 0), [traffic]);
  const publicBypass = useMemo(() => traffic.filter((x) => x.mode === 'DIRECT' || x.mode === 'MIXED').length, [traffic]);

  const chooseProfile = (uuid: string) => {
    if (dirty && !window.confirm('Discard unsaved routing edits?')) return;
    const profile = profiles.find((x) => x.uuid === uuid);
    if (!profile) return;
    setSelectedUuid(uuid); setDraft(structuredClone(profile)); setDirty(false); setMessage('');
  };

  const mutateBucket = (bucket: RouteBucket, values: string[]) => { setDraft((x) => x ? { ...x, [bucket]: values } : x); setDirty(true); };
  const patchDraft = (patch: Partial<RoutingProfile>) => { setDraft((x) => x ? { ...x, ...patch } : x); setDirty(true); };

  const save = async (activate = false) => {
    if (!draft) return;
    setBusy(true); setError('');
    try {
      const result = await invoke<string>('save_routing_profile', { profile: draft, activate });
      setMessage(result); setDirty(false);
      await Promise.all([loadStatus(), loadProfiles()]);
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const activate = async () => {
    if (!draft) return;
    if (dirty) { await save(true); return; }
    setBusy(true);
    try {
      await invoke('activate_routing_profile', { uuid: draft.uuid });
      setMessage(`Activated ${draft.remark || draft.name}`);
      window.setTimeout(() => { loadStatus(); loadProfiles(); }, 900);
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const createProfile = async () => {
    const name = window.prompt('Routing profile name', 'New routing');
    if (name === null) return;
    setBusy(true);
    try {
      const result = await invoke<string>('create_routing_profile', { name });
      const uuid = result.split('|')[0];
      setDirty(false); setSelectedUuid(uuid); setMessage('Routing profile created');
      await loadProfiles();
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const deleteProfile = async () => {
    if (!draft || draft.active) return;
    if (!window.confirm(`Delete routing profile “${draft.remark || draft.name}”? A database backup is created first.`)) return;
    setBusy(true);
    try {
      const result = await invoke<string>('delete_routing_profile', { uuid: draft.uuid });
      setMessage(result); setDraft(null); setSelectedUuid(''); setDirty(false);
      await loadProfiles();
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const control = async (action: 'start' | 'stop' | 'restart') => {
    setBusy(true);
    try { await invoke('control_v2rayu', { action }); setMessage(`V2ray core ${action} requested`); window.setTimeout(loadStatus, 900); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const switchMode = async (mode: ProxyMode) => {
    setBusy(true); setError('');
    try { await invoke('set_proxy_mode', { mode }); setMessage(`Mode switched to ${mode.toUpperCase()}`); window.setTimeout(loadStatus, 900); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  return <div className="app-shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-mark"><Network size={18}/></div><div><strong>V2Proxy Shell</strong><span>V2rayU control shell</span></div></div>
      <nav>
        <button className={tab === 'connections' ? 'active' : ''} onClick={() => setTab('connections')}><Activity size={17}/>Connections</button>
        <button className={tab === 'routing' ? 'active' : ''} onClick={() => setTab('routing')}><Route size={17}/>Routing</button>
        <button className={tab === 'control' ? 'active' : ''} onClick={() => setTab('control')}><Settings2 size={17}/>Control</button>
      </nav>
      <div className="side-status"><span className={`status-dot ${status?.xray_running ? 'online' : ''}`}/><div><strong>{status?.xray_running ? 'Xray online' : 'Xray offline'}</strong><span>{status?.proxy_mode || 'Unknown mode'}</span></div></div>
    </aside>

    <main>
      <header className="topbar"><div><div className="eyebrow">LOCAL MAC CONTROL</div><h1>{tab === 'connections' ? 'Network activity' : tab === 'routing' ? 'Routing' : 'Runtime control'}</h1></div><button className="secondary" onClick={() => { loadStatus(); loadProfiles(); loadTraffic(); }}><RefreshCw size={15}/>Refresh</button></header>
      {error && <div className="error-banner">{error}</div>}
      {message && <div className="notice-banner" onClick={() => setMessage('')}>{message}</div>}

      {tab === 'connections' && <>
        <section className="stats-grid"><div className="stat"><span>Download</span><strong>{formatRate(totalDown)}</strong></div><div className="stat"><span>Upload</span><strong>{formatRate(totalUp)}</strong></div><div className="stat"><span>Processes</span><strong>{traffic.length}</strong></div><div className="stat warning"><span>Direct / mixed</span><strong>{publicBypass}</strong></div></section>
        <section className="panel"><div className="panel-head"><div><h2>Live connections</h2><p>Per-process nettop traffic with observed proxy/direct socket path.</p></div><span className="live-pill"><span/>Live</span></div>
          <div className="traffic-table"><div className="traffic-row header"><span>Process</span><span>Mode</span><span>Down</span><span>Up</span><span>Direct target</span></div>{traffic.map((row) => <div className="traffic-row" key={`${row.pid}-${row.process}`}><span className="process"><strong>{row.process}</strong><small>PID {row.pid} · PX {row.proxy_connections} · DIR {row.direct_connections}</small></span><span><span className={`mode mode-${row.mode.toLowerCase()}`}>{row.mode}</span></span><span className="mono">{formatRate(row.down_bps)}</span><span className="mono">{formatRate(row.up_bps)}</span><span className="target mono" title={row.direct_targets.join(', ')}>{row.direct_targets.join(', ') || '—'}</span></div>)}</div>
        </section>
      </>}

      {tab === 'routing' && <div className="routing-layout">
        <section className="panel profile-list-panel"><div className="panel-head"><div><h2>Routing profiles</h2><p>{status?.routing_db_path || 'V2rayU database'}</p></div><button className="icon-button" onClick={createProfile} disabled={busy} title="New routing"><Plus size={15}/></button></div><div className="profile-list">{profiles.map((p) => <button key={p.uuid} className={`profile-item ${p.uuid === selectedUuid ? 'selected' : ''}`} onClick={() => chooseProfile(p.uuid)}><div><strong>{p.remark || p.name || 'Untitled routing'}</strong><span>{p.domain_strategy} · {p.domain_matcher}</span></div>{p.active && <CheckCircle2 size={16}/>}</button>)}</div></section>
        <div className="routing-editor">{draft ? <>
          <section className="panel routing-summary"><div className="routing-fields"><label>Remark<input value={draft.remark} onChange={(e) => patchDraft({ remark: e.target.value })}/></label><label>Domain strategy<select value={draft.domain_strategy} onChange={(e) => patchDraft({ domain_strategy: e.target.value })}><option>AsIs</option><option>IPIfNonMatch</option><option>IPOnDemand</option></select></label><label>Domain matcher<select value={draft.domain_matcher} onChange={(e) => patchDraft({ domain_matcher: e.target.value })}><option>hybrid</option><option>linear</option></select></label></div><div className="routing-actions"><button className="danger-button" disabled={busy || draft.active} onClick={deleteProfile}><Trash2 size={15}/>Delete</button><button className="secondary" disabled={busy || draft.active} onClick={activate}>Activate</button><button className="primary" disabled={busy || !dirty} onClick={() => save(draft.active)}><ShieldCheck size={16}/>{busy ? 'Applying…' : draft.active ? 'Apply & reload' : 'Save'}</button></div></section>
          <section className="rules-grid"><RuleList title="Direct" bucket="direct" values={draft.direct} onChange={mutateBucket}/><RuleList title="Proxy" bucket="proxy" values={draft.proxy} onChange={mutateBucket}/><RuleList title="Block" bucket="block" values={draft.block} onChange={mutateBucket}/></section>
        </> : <section className="panel empty-editor">No routing profile found.</section>}</div>
      </div>}

      {tab === 'control' && <section className="panel control-panel">
        <div className="control-status"><div className={`large-indicator ${status?.xray_running ? 'online' : ''}`}><Network size={28}/></div><div><div className="eyebrow">RUNTIME</div><h2>{status?.xray_running ? 'Xray is running' : 'Xray is stopped'}</h2><p>V2rayU: {status?.v2rayu_running ? 'running' : 'stopped'} · desired core: {status?.core_enabled ? 'on' : 'off'} · mode: {status?.proxy_mode || 'unknown'}</p></div></div>
        <div className="control-actions"><button className="primary" disabled={busy} onClick={() => control('start')}><CirclePlay size={17}/>Start core</button><button className="secondary" disabled={busy} onClick={() => control('restart')}><RefreshCw size={17}/>Restart V2rayU</button><button className="danger-button" disabled={busy} onClick={() => control('stop')}><CirclePause size={17}/>Stop core</button></div>
        <div className="mode-section"><div><div className="eyebrow">PROXY MODE</div><h3>V2rayU mode</h3><p>Updates V2rayU's persisted runMode and relaunches it so its own runtime applies the change.</p></div><div className="mode-switcher">{(['pac','manual','global','tun'] as ProxyMode[]).map((mode) => <button key={mode} className={status?.proxy_mode?.toLowerCase() === mode ? 'active' : ''} disabled={busy} onClick={() => switchMode(mode)}>{mode.toUpperCase()}</button>)}</div></div>
        <div className="notice"><ShieldCheck size={16}/><div><strong>V2rayU remains the source of runtime behavior</strong><span>Routing is edited in V2rayU's persisted routing database; generated ~/.V2rayU/config.json is treated as runtime evidence and may be regenerated by V2rayU.</span></div></div>
      </section>}
    </main>
  </div>;
}