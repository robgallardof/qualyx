"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useTheme } from "next-themes";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { Activity, ArrowDownLeft, ArrowRight, Check, CheckCheck, ChevronRight, CircleHelp, CircleX, Copy as CopyIcon, Eye, EyeOff, FileText, Folder, Globe2, LayoutList, Loader2, LockKeyhole, Monitor, Moon, Plug, RefreshCw, Search, ShieldCheck, Sun, Terminal, Unplug, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { messages, type Locale } from "@/lib/i18n";
import { filterRuns, parseRuns, quotePath, summarize, type Run } from "@/lib/runs";

type Credentials = { project: string; token: string };
type Guide = "mcp" | "help" | null;

export function Console() {
  const [locale, setLocale] = useState<Locale>("es");
  const [ready, setReady] = useState(false);
  const [windows, setWindows] = useState(false);
  const [api, setApi] = useState<"checking" | "online" | "offline">("checking");
  const [version, setVersion] = useState("");
  const [connected, setConnected] = useState(false);
  const [credentials, setCredentials] = useState<Credentials | null>(null);
  const [runs, setRuns] = useState<Run[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<"authError" | "forbidden" | "networkError" | "serverError" | "invalidResponse" | null>(null);
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState("all");
  const [showToken, setShowToken] = useState(false);
  const [guide, setGuide] = useState<Guide>(null);
  const [selected, setSelected] = useState<Run | null>(null);
  const [report, setReport] = useState<string | null>(null);
  const [reportError, setReportError] = useState(false);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);
  const request = useRef<AbortController | null>(null);
  const artifactRequest = useRef<AbortController | null>(null);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const { theme, setTheme } = useTheme();
  const t = messages[locale];
  const schema = useMemo(() => z.object({ project: z.string().trim().min(1, t.pathError), token: z.string().min(32, t.tokenError).regex(/^[\x21-\x7e]+$/, t.tokenError) }), [t]);
  const form = useForm<Credentials>({ resolver: zodResolver(schema), defaultValues: { project: "", token: "" } });

  useEffect(() => {
    let saved: string | null = null;
    try { saved = localStorage.getItem("qualyx-locale"); } catch { /* storage is optional */ }
    setLocale(saved === "en" || saved === "es" ? saved : navigator.language.toLowerCase().startsWith("es") ? "es" : "en");
    setWindows(navigator.userAgent.includes("Windows"));
    setReady(true);
    const controller = new AbortController();
    fetch("/health", { signal: AbortSignal.any([controller.signal, AbortSignal.timeout(5000)]), redirect: "error" })
      .then(async (r) => { if (!r.ok) throw new Error(); const data = await r.json(); setApi("online"); if (typeof data.version === "string") setVersion(data.version); })
      .catch(() => { if (!controller.signal.aborted) setApi("offline"); });
    return () => { controller.abort(); request.current?.abort(); artifactRequest.current?.abort(); if (copyTimer.current) clearTimeout(copyTimer.current); };
  }, []);
  useEffect(() => { document.documentElement.lang = locale; }, [locale]);

  function changeLanguage() {
    const next = locale === "es" ? "en" : "es";
    setLocale(next);
    try { localStorage.setItem("qualyx-locale", next); } catch { /* nonessential preference */ }
    form.clearErrors();
  }

  async function load(values: Credentials) {
    request.current?.abort();
    const controller = new AbortController(); request.current = controller;
    setLoading(true); setError(null); setRuns([]); setSelected(null);
    try {
      const response = await fetch("/v1/runs?" + new URLSearchParams({ project: values.project, limit: "100" }), {
        headers: { Authorization: "Bearer " + values.token }, cache: "no-store", redirect: "error",
        signal: AbortSignal.any([controller.signal, AbortSignal.timeout(15000)]),
      });
      if (!response.ok) { setError(response.status === 401 ? "authError" : response.status === 403 ? "forbidden" : "serverError"); return; }
      let parsed: Run[];
      try { parsed = parseRuns(await response.json()); } catch { setError("invalidResponse"); return; }
      if (controller.signal.aborted) return;
      setRuns(parsed); setCredentials(values); setConnected(true); setSearch(""); setStatus("all"); setApi("online"); setShowToken(false);
    } catch { if (!controller.signal.aborted) { setError("networkError"); setApi("offline"); } }
    finally { if (!controller.signal.aborted) setLoading(false); }
  }

  function disconnect() {
    request.current?.abort(); artifactRequest.current?.abort();
    setCredentials(null); setConnected(false); setRuns([]); setError(null); setLoading(false); setSelected(null); setReport(null); setShowToken(false);
    form.reset({ project: credentials?.project ?? "", token: "" });
  }

  async function openReport(run: Run) {
    if (!credentials) return;
    artifactRequest.current?.abort(); const controller = new AbortController(); artifactRequest.current = controller;
    setSelected(run); setReport(null); setReportError(false);
    try {
      const response = await fetch(`/v1/runs/${encodeURIComponent(run.run_id)}/artifacts/report.md?` + new URLSearchParams({ project: credentials.project }), {
        headers: { Authorization: "Bearer " + credentials.token }, redirect: "error", cache: "no-store",
        signal: AbortSignal.any([controller.signal, AbortSignal.timeout(15000)]),
      });
      if (!response.ok) throw new Error();
      const text = await response.text(); if (!controller.signal.aborted) setReport(text);
    } catch { if (!controller.signal.aborted) setReportError(true); }
  }

  async function copyCommand(value: string) {
    try { await navigator.clipboard.writeText(value); setCopied(true); setCopyError(false); if (copyTimer.current) clearTimeout(copyTimer.current); copyTimer.current = setTimeout(() => setCopied(false), 2000); }
    catch { setCopyError(true); }
  }

  const visible = filterRuns(runs, search, status);
  const summary = summarize(runs);
  const hasData = connected && !error;
  const path = credentials?.project || (windows ? "C:/projects/my-app" : "/work/my-app");
  const command = `qualyx mcp-config codex --root ${quotePath(path, windows)}`;
  const testCommand = `qualyx test ${quotePath(path, windows)}`;
  function statusLabel(value: string) { return t.results[value as keyof typeof t.results] ?? t.unknown; }
  function date(value?: string | null) { if (!value || Number.isNaN(Date.parse(value))) return t.noDate; return new Intl.DateTimeFormat(locale === "es" ? "es-MX" : "en-US", { dateStyle: "medium", timeStyle: "short" }).format(new Date(value)); }

  return <div className="console-shell" data-ready={ready} data-loading={loading} data-connected={connected}>
    <aside className="sidebar">
      <div className="brand"><span className="brand-mark" aria-hidden="true"><span /><span /><span /><span /></span><span>qualyx<span className="brand-dot">.</span></span><span className="brand-label">console</span></div>
      <div className="workspace-box"><span className="workspace-icon"><Terminal size={17} /></span><div><strong>Qualyx</strong><span>{t.local}</span></div><LockKeyhole size={13} /></div>
      <p className="nav-label">{t.workspace}</p>
      <nav aria-label={t.workspace}>
        <button className="nav-item active" aria-current="page"><LayoutList size={17} />{t.runs}<span className="nav-dot" /></button>
        <button className="nav-item" onClick={() => setGuide("mcp")}><Plug size={17} />{t.mcp}<ArrowDownLeft size={14} className="nav-tail" /></button>
        <button className="nav-item" onClick={() => setGuide("help")}><CircleHelp size={17} />{t.help}</button>
      </nav>
      <div className="sidebar-bottom"><div className="mini-note"><Terminal size={19} /><strong>CLI + MCP</strong><p>{locale === "es" ? "Ejecuta desde tu agente. Revisa aquí." : "Run from your agent. Review here."}</p><button onClick={() => setGuide("mcp")}>{t.mcp}<ArrowRight size={14} /></button></div>
        <div className="api-state"><span className={`status-dot ${api}`} /><span>{api === "online" ? t.api : api === "offline" ? t.apiOffline : t.apiUnknown}</span>{version && <code>v{version}</code>}</div></div>
    </aside>

    <div className="main-shell">
      <header className="topbar"><div className="breadcrumbs"><span className="mobile-brand">qualyx.</span><span className="desktop-crumb">{t.workspace}</span><ChevronRight size={13} /><strong>{t.runs}</strong></div>
        <div className="top-actions"><Badge variant="outline" className="read-only"><LockKeyhole size={11} />{t.readOnly}</Badge><span className="top-divider" />
          <Button variant="ghost" size="sm" onClick={changeLanguage} aria-label={t.language} data-testid="language-toggle"><Globe2 size={15} /><span>{locale.toUpperCase()}</span></Button>
          <Select value={ready ? theme : "system"} onValueChange={setTheme}><SelectTrigger aria-label={t.theme} className="theme-select"><span className="theme-icon">{ready && theme === "dark" ? <Moon size={15} /> : ready && theme === "light" ? <Sun size={15} /> : <Monitor size={15} />}</span><SelectValue /></SelectTrigger><SelectContent><SelectItem value="light">{t.light}</SelectItem><SelectItem value="dark">{t.dark}</SelectItem><SelectItem value="system">{t.system}</SelectItem></SelectContent></Select>
        </div></header>

      <main className="main-content">
        <div className="page-heading"><div><div className="eyebrow"><span className="tiny-square" />QUALYX / {t.readOnly.toUpperCase()}</div><h1>{t.title}<span className="title-period">.</span></h1><p>{t.subtitle}</p></div><Button variant="outline" onClick={() => credentials && load(credentials)} disabled={!connected || loading} data-testid="refresh"><RefreshCw size={15} className={loading ? "spin" : ""} />{loading ? t.refreshing : t.refresh}</Button></div>

        {!connected ? <section className="connection-card" aria-labelledby="connection-title"><div className="connection-heading"><span className="connection-icon"><Folder size={20} /></span><div><h2 id="connection-title">{t.connectionTitle}</h2><p>{t.connectionHint}</p></div><Badge variant="secondary">01</Badge></div>
          <form id="f" onSubmit={form.handleSubmit(load)} noValidate className="connection-form"><div className="form-field"><Label htmlFor="project">{t.project}</Label><div className="input-wrap"><Folder size={15} /><Input id="project" placeholder={t.pathPlaceholder} autoComplete="off" spellCheck={false} {...form.register("project")} aria-invalid={!!form.formState.errors.project} aria-describedby={form.formState.errors.project ? "project-error" : undefined} /></div>{form.formState.errors.project && <span id="project-error" className="field-error" role="alert">{form.formState.errors.project.message}</span>}</div>
            <div className="form-field"><Label htmlFor="token">{t.token}<LockKeyhole size={11} /></Label><div className="input-wrap"><LockKeyhole size={15} /><Input id="token" type={showToken ? "text" : "password"} placeholder={t.tokenPlaceholder} autoComplete="off" spellCheck={false} {...form.register("token")} aria-invalid={!!form.formState.errors.token} aria-describedby={form.formState.errors.token ? "token-error token-help" : "token-help"} /><button type="button" className="show-token" onClick={() => setShowToken(!showToken)} aria-label={showToken ? t.hideToken : t.showToken}>{showToken ? <EyeOff size={15} /> : <Eye size={15} />}</button></div>{form.formState.errors.token && <span id="token-error" className="field-error" role="alert">{form.formState.errors.token.message}</span>}</div>
            <Button id="load" type="submit" disabled={loading} className="connect-button">{loading ? <Loader2 size={15} className="spin" /> : <Plug size={15} />}{t.connect}<ArrowRight size={14} /></Button>
            <div id="token-help" className="form-note"><ShieldCheck size={13} />{t.tokenHelp}</div>
          </form></section> : <section className="connected-strip"><span className="connection-icon"><Folder size={19} /></span><div><span className="connected-label"><span className="status-dot online" />{t.connected}</span><code title={credentials?.project}>{credentials?.project}</code></div><Button variant="ghost" size="sm" onClick={disconnect}><Unplug size={14} /><span>{t.disconnect}</span></Button></section>}

        <div id="msg" data-error={error ?? ""} role={error ? "alert" : "status"} aria-live="polite" className={error ? "message error-message" : "sr-only"}>{error ? <><CircleX size={17} />{t[error]}</> : loading ? t.loading : connected ? t.loaded : ""}</div>

        <section className="stats-grid" aria-label={t.recent}>{[
          { label: t.allRuns, value: summary.total, icon: LayoutList, style: "neutral" },
          { label: t.good, value: summary.passed, icon: CheckCheck, style: "success" },
          { label: t.bad, value: summary.failed, icon: CircleX, style: "failure" },
          { label: t.active, value: summary.active, icon: Activity, style: "progress" },
        ].map((stat) => <div className="stat" key={stat.style}><div className="stat-label">{stat.label}<stat.icon size={16} /></div><div className={`stat-number ${hasData ? stat.style : ""}`}>{hasData ? stat.value.toString().padStart(2, "0") : <span aria-label={t.notMeasured}>—</span>}</div><span className={`stat-rule ${stat.style}`} /></div>)}</section>

        <section className="history" aria-labelledby="history-title"><div className="history-heading"><div><h2 id="history-title">{t.recent}<Badge variant="secondary">{hasData ? runs.length : "—"}</Badge></h2><p>{t.recentHint}</p></div><div className="history-controls"><div className="search-wrap"><Search size={15} /><Input aria-label={t.search} placeholder={t.search} value={search} onChange={(e) => setSearch(e.target.value)} disabled={!hasData} /></div><Select value={status} onValueChange={setStatus} disabled={!hasData}><SelectTrigger aria-label={t.status} className="filter-select"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">{t.all}</SelectItem>{Object.entries(t.results).map(([key, label]) => <SelectItem key={key} value={key}>{label}</SelectItem>)}</SelectContent></Select></div></div>
          <div className="table-scroll"><table><thead><tr><th>{t.run}</th><th>{t.status}</th><th>{t.tests}</th><th>{t.started}</th><th className="right-align">{t.evidence}</th></tr></thead><tbody id="rows">{!loading && !error && visible.map((run) => <tr key={run.run_id}><td><span className="run-id"><span className="run-symbol"><Terminal size={14} /></span><code title={run.run_id}>{run.run_id}</code></span></td><td><span className={`result-badge result-${Object.keys(t.results).includes(run.status) ? run.status : "unknown"}`}><span />{statusLabel(run.status)}</span></td><td>{run.total == null ? <span className="muted">—</span> : <span className="test-count"><strong>{run.total}</strong><span><span className="count-passed">{run.passed ?? "—"} {t.passedTests}</span> · <span className={run.failed ? "count-failed" : ""}>{run.failed ?? "—"} {t.failedTests}</span></span></span>}</td><td><span className="date-cell">{date(run.started_at)}</span></td><td className="right-align"><Button variant="ghost" size="sm" onClick={() => openReport(run)} disabled={run.status === "queued" || run.status === "running"} aria-label={`${t.view}: ${run.run_id}`}><FileText size={14} /><span>{t.view}</span><ChevronRight size={13} /></Button></td></tr>)}</tbody></table></div>
          {loading ? <div className="loading-state" aria-label={t.loading}><Loader2 size={24} className="spin" /><p>{t.loading}</p><div className="skeleton-lines"><i /><i /><i /></div></div> : (visible.length === 0 || !!error) && <div className="empty-state" data-kind={!hasData ? "disconnected" : runs.length === 0 ? "no-runs" : "no-match"}><div className="empty-art" aria-hidden="true"><div className="empty-paper"><span /><span /><span /><i><Check size={13} /></i></div><div className="empty-orbit"><Folder size={19} /></div></div><h3>{!hasData ? t.emptyTitle : runs.length === 0 ? t.noRunsTitle : t.noMatchTitle}</h3><p>{!hasData ? t.emptyText : runs.length === 0 ? t.noRunsText : t.noMatchText}</p>{hasData && runs.length === 0 ? <div className="command-snippet"><Terminal size={14} /><code>{testCommand}</code><Button variant="ghost" size="icon" aria-label={t.copy} onClick={() => copyCommand(testCommand)}>{copied ? <Check size={14} /> : <CopyIcon size={14} />}</Button></div> : hasData && <Button variant="outline" size="sm" onClick={() => { setSearch(""); setStatus("all"); }}>{t.clearFilters}</Button>}</div>}
          <div className="table-footer"><span><span className="status-dot" />{hasData ? `${visible.length} ${t.showing}` : t.notConnected}</span><span><LockKeyhole size={11} />{t.readOnly}</span></div>
        </section>
        <div className="bottom-note"><span>qualyx<span className="brand-dot">.</span> <span>{t.footer}</span></span><div><button onClick={() => setGuide("mcp")}><Plug size={13} />{t.mcp}</button><button onClick={() => setGuide("help")}><CircleHelp size={13} />{t.help}<ArrowRight size={13} /></button></div></div>
        {copyError && <p role="alert" className="field-error">{t.copyError}</p>}
      </main>
    </div>

    <Dialog open={guide !== null} onOpenChange={(open) => { if (!open) setGuide(null); }}><DialogContent className="guide-dialog" closeLabel={t.close}><DialogHeader><span className="dialog-icon">{guide === "mcp" ? <Plug size={22} /> : <CircleHelp size={22} />}</span><DialogTitle>{guide === "mcp" ? t.mcpTitle : t.helpTitle}</DialogTitle><DialogDescription>{guide === "mcp" ? t.mcpText : t.helpText}</DialogDescription></DialogHeader>
      <div className="guide-steps">{(guide === "mcp" ? [[t.mcpStep1,t.mcpStep1Text], [t.mcpStep2,t.mcpStep2Text], [t.mcpStep3,t.mcpStep3Text]] : [[t.howLocal,t.howLocalText], [t.howApi,t.howApiText], [t.howPanel,t.howPanelText]]).map(([title, text], index) => <div className="guide-step" key={title}><span>{String(index + 1).padStart(2,"0")}</span><div><h3>{title}</h3><p>{text}</p></div></div>)}</div>
      {guide === "mcp" && <div className="code-box"><span>{windows ? "PowerShell" : "Terminal"}</span><pre>{command}</pre><Button variant="outline" size="sm" onClick={() => copyCommand(command)}>{copied ? <Check size={14} /> : <CopyIcon size={14} />}{copied ? t.copied : t.copy}</Button>{copyError && <p role="alert">{t.copyError}</p>}</div>}
      <a className="docs-link" href={`https://github.com/robgallardof/qualyx/blob/main/docs/${guide === "mcp" ? "m2/clients.md" : "m7/team.md"}`} target="_blank" rel="noreferrer">{t.docs}<ArrowRight size={14} /></a>
    </DialogContent></Dialog>
    <Dialog open={selected !== null} onOpenChange={(open) => { if (!open) { artifactRequest.current?.abort(); setSelected(null); setReport(null); } }}><DialogContent className="report-dialog" closeLabel={t.close}><DialogHeader><DialogTitle>{t.report}</DialogTitle><DialogDescription>{t.reportHint}</DialogDescription></DialogHeader><code className="report-id">{selected?.run_id}</code>{reportError ? <p role="alert" className="error-message">{t.reportError}</p> : report === null ? <div className="loading-state" role="status"><Loader2 className="spin" />{t.loading}</div> : <pre className="report-content" data-testid="report-content">{report}</pre>}</DialogContent></Dialog>
  </div>;
}
